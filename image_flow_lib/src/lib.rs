#![doc = include_str!("../README.md")]
#![recursion_limit = "512"]
#![feature(const_trait_impl)]
#![cfg_attr(test, feature(const_cmp))]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

#[allow(clippy::too_long_first_doc_paragraph)]
pub mod context;
pub mod error;
pub mod image;
pub mod location;
pub mod requesters;
#[cfg(feature = "video")]
pub mod video_slicer;

use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

pub use crate::context::image_based_context::FramePath;
use crate::context::image_based_context::ImageRecognizer;
pub use crate::error::{Error, Result};
use crate::image::depth::DepthMap;
use crate::image::detections::ImageDetections;
use crate::location::tag::UniqueLocationTags;
use crate::{
    context::image_based_context::evaluate_image_into_location_tags,
    image_processor::DominantColors,
};
use crate::{
    context::location_based_context::evaluate_image_and_location_as_location_tags,
    image_processor::dominant_colors,
};

pub(crate) mod image_processor;

#[cfg(test)]
#[allow(dead_code)]
pub(crate) mod tests;

uniffi::setup_scaffolding!();

use crate::{
    error::ImageFlowError, location::coordinate_system::Coordinate, requesters::AuthToHost,
};
use std::fmt::Display;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::{result::Result as DefaultResult, sync::Arc};

/// Location tags and depth-enriched object detections for a single image.
///
/// Rust callers can access and mutate [`Self::location_tags`] and
/// [`Self::image_detections`] directly. The accessor methods return clones.
/// Use [`Self::save_as_json`] to write one evaluation.
#[derive(Debug, Clone, uniffi::Object, Deserialize, Serialize)]
pub struct ImageFlowEvaluation {
    /// Geographic context tags for this image.
    pub location_tags: UniqueLocationTags,
    /// Detected objects with bounding boxes, confidence scores, and estimated depth.
    pub image_detections: ImageDetections,
}

/// One evaluated recording frame with its relative timestamp and GPS coordinate.
///
/// A JSON array of these objects is the input to
/// [`image_flow_evaluation_read_from_json`].
#[derive(Serialize, Deserialize, Clone, Debug, uniffi::Object)]
pub struct ImageFlowEvaluationForRecording {
    /// Evaluation for this frame; serialized with the plural key `evaluations`.
    pub evaluations: ImageFlowEvaluation,
    /// Elapsed time from the start of the recording.
    pub time_diff: Duration,
    /// GPS coordinate paired with this frame.
    pub location: Coordinate,
}

/// Read a JSON array of recording evaluations, preserving array order.
///
/// This expects [`ImageFlowEvaluationForRecording`] objects, rather than the
/// single evaluation written by [`ImageFlowEvaluation::save_as_json`].
///
/// # Errors
/// Returns [`ImageFlowError::Serialization`] if the file cannot be read or its
/// JSON does not match the recording-array schema.
#[uniffi::export(async_runtime = "tokio")]
pub async fn image_flow_evaluation_read_from_json(
    path: &str,
) -> DefaultResult<Vec<Arc<ImageFlowEvaluationForRecording>>, ImageFlowError> {
    let json_str = tokio::fs::read_to_string(path)
        .await
        .map_err(crate::error::serialization)?;
    let data: Vec<ImageFlowEvaluationForRecording> =
        serde_json::from_str(&json_str).map_err(crate::error::serialization)?;

    Ok(data
        .into_iter()
        .map(Arc::new)
        .collect())
}

/// Detect objects with depth estimates and classify the image's geographic context.
///
/// `amount` is the number of dominant-color clusters (use a small positive
/// value); it is not validated. `confidence_limit` is clamped to `0.0..=1.0`
/// before filtering. Detections at or above the clamped threshold are retained.
/// `auth` configures map services; `None` selects defaults. Reuse
/// `image_recognizer` across images.
///
/// Map lookup errors are logged and can produce partial tags without failing the
/// evaluation. See [`image_flow_evaluation_impl`] for the Rust borrowing API.
///
/// # Errors
/// Returns an error for missing/unreadable images, inference failures,
/// dominant-color extraction failures, or failed worker tasks. An unknown
/// debug-build Nominatim classification panics in the spawned context task and
/// is returned here as an I/O error.
///
/// # Panics
/// Inherits inference assertions from [`ImageRecognizer::predict_objects`].
#[uniffi::export(async_runtime = "tokio")]
pub async fn image_flow_evaluation(
    location: Coordinate,
    auth: Option<AuthToHost>,
    img_path: &str,
    image_recognizer: Arc<ImageRecognizer>,
    amount: u32,
    confidence_limit: f32,
) -> DefaultResult<ImageFlowEvaluation, ImageFlowError> {
    let img_flow = image_flow_evaluation_impl(
        location,
        auth,
        img_path,
        &image_recognizer,
        amount as usize,
        confidence_limit,
    )
    .await?;

    Ok(img_flow)
}

/// Detect objects and attach depth sampled at each bounding-box center.
///
/// Runs both object and depth inference. `confidence_limit` is clamped to
/// `0.0..=1.0` before filtering. Detections below the clamped threshold are
/// discarded before cyclist merging and duplicate removal.
///
/// # Errors
/// Returns an error if the image is missing, either model cannot predict, or a
/// worker task fails.
///
/// # Panics
/// Inherits inference assertions from [`ImageRecognizer::predict_objects`].
#[uniffi::export(async_runtime = "tokio")]
pub async fn recognize_objects_in_image(
    path: &str,
    image_recognizer: Arc<ImageRecognizer>,
    confidence_limit: f32,
) -> DefaultResult<ImageDetections, ImageFlowError> {
    let detections =
        recognize_objects_in_image_impl(path, &image_recognizer, confidence_limit).await?;

    Ok(detections)
}

/// Estimate per-pixel depth using the recognizer's depth model.
///
/// # Errors
/// Returns an error for a missing image, inference/task failures, or model output
/// without depth data.
///
/// # Panics
/// Panics if the inference backend does not return exactly one result.
#[uniffi::export(async_runtime = "tokio")]
pub async fn recognize_depth_in_image(
    path: &str,
    image_recognizer: Arc<ImageRecognizer>,
) -> DefaultResult<DepthMap, ImageFlowError> {
    let detections = recognize_depth_in_image_impl(path, &image_recognizer).await?;

    Ok(detections)
}

/// Classify a coordinate and image colors without loading inference models.
///
/// `amount` is the number of dominant-color clusters; use a small positive value.
/// This does not use object detections (including the traffic-light heuristic).
/// Map failures are logged and treated as missing evidence.
///
/// # Errors
/// Returns an error if the image is missing, cannot be decoded, or color
/// extraction fails.
///
/// # Panics
/// Unknown Nominatim classifications can panic in debug builds.
#[uniffi::export(async_runtime = "tokio")]
pub async fn recognize_location_tags(
    location: Coordinate,
    auth: Option<AuthToHost>,
    img_path: &str,
    amount: u32,
) -> DefaultResult<UniqueLocationTags, ImageFlowError> {
    let eval = recognize_location_tags_impl(location, auth, img_path, amount as usize).await?;

    Ok(eval)
}

// === LOGGING ===
static TRACING_ENABLED: AtomicBool = AtomicBool::new(false);

/// Try to install a process-wide tracing subscriber at the requested level.
///
/// `None` defaults to [`FilterLevel::Off`]. Returns `true` after a successful
/// installation by this library, including subsequent calls. Returns `false`
/// if initialization fails, for example because the application already installed
/// a subscriber. Later calls do not change an installed subscriber's level.
#[must_use]
#[uniffi::export]
pub fn enable_tracing(rust_log: Option<FilterLevel>) -> bool {
    if TRACING_ENABLED.load(Ordering::Relaxed) {
        return true;
    }

    let rust_log = rust_log.unwrap_or_default();
    let tag = rust_log.to_string();

    let enabled = tracing_subscriber::fmt()
        .with_env_filter(tag)
        .with_target(true)
        .with_thread_ids(true)
        .try_init()
        .is_ok();

    TRACING_ENABLED.store(enabled, Ordering::Relaxed);
    enabled
}

/// Global log filter used by [`enable_tracing`]; defaults to no events.
#[derive(uniffi::Enum, Clone, Copy, Debug, Default)]
pub enum FilterLevel {
    /// Disable all events.
    #[default]
    Off,
    /// Enable warnings and errors.
    Warn,
    /// Enable informational events, warnings, and errors.
    Info,
    /// Enable debug events and all higher-severity events.
    Debug,
    /// Enable all events.
    Trace,
    /// Enable errors only.
    Error,
}

impl Display for FilterLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Off => "off",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
            Self::Error => "error",
        };

        f.write_str(value)
    }
}

// === IMPLEMENTATIONS ===

/// Rust borrowing/path variant of [`recognize_objects_in_image`].
///
/// `confidence_limit` is clamped to `0.0..=1.0` before filtering detections.
///
/// # Errors
/// Returns an error for a missing image, inference failure, or failed worker task.
///
/// # Panics
/// Inherits inference assertions from [`ImageRecognizer::predict_objects`].
pub async fn recognize_objects_in_image_impl(
    path: impl AsRef<Path>,
    image_recognizer: &ImageRecognizer,
    confidence_limit: f32,
) -> crate::Result<ImageDetections> {
    let path = path.as_ref();
    if !path.exists() {
        let err_msg = format!("the file {} does not exist", path.display());
        return Err(crate::error::file_does_not_exist(err_msg));
    }

    image_recognizer
        .predict_objects(path, confidence_limit)
        .await
}

/// Rust borrowing/path variant of [`recognize_depth_in_image`].
///
/// # Errors
/// Returns an error for a missing image, inference/task failures, or absent depth.
///
/// # Panics
/// Panics if the inference backend does not return exactly one result.
pub async fn recognize_depth_in_image_impl(
    path: impl AsRef<Path>,
    image_recognizer: &ImageRecognizer,
) -> crate::Result<DepthMap> {
    let path = path.as_ref();
    if !path.exists() {
        let err_msg = format!("the file {} does not exist", path.display());
        return Err(crate::error::file_does_not_exist(err_msg));
    }

    image_recognizer
        .predict_depth(path)
        .await
}

/// Rust path variant of [`recognize_location_tags`], taking a `usize` color count.
///
/// # Errors
/// Returns an error for a missing/unreadable image or a failed color-extraction task.
/// Map request errors are logged rather than propagated.
///
/// # Panics
/// Unknown Nominatim classifications can panic in debug builds.
pub async fn recognize_location_tags_impl(
    location: impl Into<Coordinate>,
    auth: Option<AuthToHost>,
    img_path: impl AsRef<Path>,
    amount: usize,
) -> crate::Result<UniqueLocationTags> {
    let img_path = img_path.as_ref();
    if !img_path.exists() {
        let err_msg = format!("the file {} does not exist", img_path.display());
        return Err(crate::error::file_does_not_exist(err_msg));
    }

    let auth = auth.map(Arc::new);

    let dominant_color = dominant_colors(img_path, amount).await?;
    let evaluation =
        evaluate_image_and_location_as_location_tags(location, auth, &dominant_color).await;

    Ok(evaluation)
}

/// Rust borrowing/path variant of [`image_flow_evaluation`].
///
/// Object/depth inference and color/map analysis are joined concurrently, then
/// image-derived tags are merged into geographic context without overriding
/// known building containment with weaker outdoor hints. `confidence_limit` is
/// clamped to `0.0..=1.0` before filtering detections.
///
/// # Errors
/// Returns an error for a missing/unreadable image, failed model inference,
/// failed color extraction, or failed worker tasks. Map request errors are logged
/// rather than propagated.
///
/// # Panics
/// Inherits inference assertions from [`ImageRecognizer::predict_objects`].
pub async fn image_flow_evaluation_impl(
    location: impl Into<Coordinate>,
    auth: Option<AuthToHost>,
    img_path: impl AsRef<Path>,
    image_recognizer: &ImageRecognizer,
    amount: usize,
    confidence_limit: f32,
) -> crate::Result<ImageFlowEvaluation> {
    let img_path = img_path
        .as_ref()
        .to_owned();
    let location = location.into();
    if !img_path.exists() {
        let err_msg = format!("the file {} does not exist", img_path.display());
        return Err(crate::error::file_does_not_exist(err_msg));
    }

    let detections_fut =
        recognize_objects_in_image_impl(&img_path, image_recognizer, confidence_limit);

    let send_img_path = img_path.clone();
    let location_tags_dom_colors_fut: JoinHandle<
        crate::Result<(UniqueLocationTags, DominantColors)>,
    > = tokio::spawn(async move {
        let dominant_colors = dominant_colors(send_img_path, amount).await?;
        let location_tags = evaluate_image_and_location_as_location_tags(
            location,
            auth.map(Arc::new),
            &dominant_colors,
        )
        .await;

        Ok((location_tags, dominant_colors))
    });

    let (image_detections_tmp, location_tags_dom_colors_tmp) =
        tokio::join!(detections_fut, location_tags_dom_colors_fut);
    let (mut location_tags, dominant_colors) = location_tags_dom_colors_tmp??;
    let image_detections = image_detections_tmp?;

    let additional_categories =
        evaluate_image_into_location_tags(&dominant_colors, &image_detections);
    location_tags.extend_context_tags(additional_categories);

    Ok(ImageFlowEvaluation {
        location_tags,
        image_detections,
    })
}

#[uniffi::export(async_runtime = "tokio")]
impl ImageFlowEvaluation {
    /// Construct an evaluation from already-computed tags and detections.
    #[uniffi::constructor]
    #[must_use]
    pub const fn new(location_tags: UniqueLocationTags, image_detections: ImageDetections) -> Self {
        Self {
            location_tags,
            image_detections,
        }
    }

    /// Write one evaluation as pretty JSON, overwriting an existing file.
    ///
    /// Parent directories must already exist. This writes a single object, not
    /// the recording array expected by [`image_flow_evaluation_read_from_json`].
    ///
    /// # Errors
    /// Returns [`ImageFlowError::Serialization`] for JSON encoding or file-write
    /// failures.
    pub async fn save_as_json(&self, path: &str) -> DefaultResult<(), ImageFlowError> {
        let json = serde_json::to_string_pretty(self).map_err(crate::error::serialization)?;

        tokio::fs::write(path, json)
            .await
            .map_err(crate::error::serialization)?;

        Ok(())
    }

    /// Return an independent clone of this evaluation's location tags.
    #[must_use]
    pub fn location_tags(&self) -> UniqueLocationTags {
        self.location_tags
            .clone()
    }

    /// Return an independent clone of this evaluation's detections.
    #[must_use]
    pub fn image_detections(&self) -> ImageDetections {
        self.image_detections
            .clone()
    }
}

#[uniffi::export]
impl ImageFlowEvaluationForRecording {
    /// Pair an evaluation with its recording-relative time and coordinate.
    #[uniffi::constructor]
    #[must_use]
    pub fn new(
        evaluations: Arc<ImageFlowEvaluation>,
        time_diff: Duration,
        location: Coordinate,
    ) -> Self {
        let evaluations = Arc::unwrap_or_clone(evaluations);
        Self {
            evaluations,
            time_diff,
            location,
        }
    }

    /// Return an independent clone of the frame's evaluation.
    #[must_use]
    pub fn evaluation(&self) -> ImageFlowEvaluation {
        self.evaluations
            .clone()
    }

    /// Return the elapsed time from the start of the recording.
    #[must_use]
    pub const fn time_diff(&self) -> Duration {
        self.time_diff
    }

    /// Return the coordinate paired with this frame.
    #[must_use]
    pub const fn location(&self) -> Coordinate {
        self.location
    }
}

impl Display for ImageFlowEvaluation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let location_tags = &self.location_tags;
        let mut detections = self
            .image_detections
            .clone()
            .detections;
        detections.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
        });

        write!(
            f,
            "ImageFlowEvaluations:\n\tlocations_tags\t{location_tags}\n\tdetections\t{detections:#?}"
        )
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use crate::{
        context::image_based_context::ImageRecognizer,
        image_flow_evaluation,
        location::tag::LocationTag,
        recognize_location_tags, recognize_objects_in_image,
        tests::{AUTH_AGENT, DOMINANT_COLOR_AMOUNT, fetch_img},
    };

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires live map services and ONNX models"]
    async fn image_flow_check() {
        let amount = DOMINANT_COLOR_AMOUNT as u32;
        let model = Arc::new(
            ImageRecognizer::default()
                .await
                .unwrap(),
        );
        let auth = AUTH_AGENT.clone();

        let mhh_img = fetch_img("MHH.png").await;
        let location = mhh_img
            .location
            .unwrap();
        let img_path = &mhh_img.img_path;

        let evaluation = image_flow_evaluation(
            location,
            auth.clone(),
            img_path
                .to_str()
                .unwrap(),
            model.clone(),
            amount,
            0.3,
        )
        .await
        .unwrap();

        insta::assert_debug_snapshot!(evaluation);

        let erdbeer_img = fetch_img("Erdbeerhof.png").await;
        let location = erdbeer_img
            .location
            .unwrap();
        let img_path = erdbeer_img
            .img_path
            .to_str()
            .unwrap();

        let evaluation = image_flow_evaluation(location, auth, img_path, model, amount, 0.3)
            .await
            .unwrap();

        insta::assert_snapshot!(evaluation);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires ONNX models and sample images"]
    async fn recognize_objects_in_image_test() {
        let bushalte_img = fetch_img("bushalte.jpg").await;

        let model = Arc::new(
            ImageRecognizer::default()
                .await
                .unwrap(),
        );
        let img_path = &bushalte_img.img_path;

        let evaluation = recognize_objects_in_image(
            img_path
                .to_str()
                .unwrap(),
            model,
            0.3,
        )
        .await
        .unwrap();

        insta::assert_debug_snapshot!(evaluation);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires live map services and sample images"]
    async fn recognize_location_tags_test() {
        let mhh_img = fetch_img("MHH.png").await;
        let location = mhh_img
            .location
            .unwrap();
        let auth = AUTH_AGENT.clone();
        let img_path = mhh_img
            .img_path
            .to_str()
            .unwrap();

        let evaluation = recognize_location_tags(
            location,
            auth.clone(),
            img_path,
            DOMINANT_COLOR_AMOUNT as u32,
        )
        .await
        .unwrap();

        let location_display = location.print_location_url();
        assert!(
            evaluation.contains(LocationTag::Inside),
            "Check out the location: {location_display}"
        );
        insta::assert_debug_snapshot!(evaluation);

        let luetzeken = fetch_img("luetzekenstrasse.png").await;
        let location_display = luetzeken
            .location
            .unwrap()
            .as_url_osm();
        let evaluation = recognize_location_tags(
            luetzeken
                .clone()
                .into(),
            auth,
            luetzeken
                .img_path
                .to_str()
                .unwrap(),
            DOMINANT_COLOR_AMOUNT as u32,
        )
        .await
        .unwrap();
        insta::assert_debug_snapshot!(evaluation);
        assert!(
            evaluation.contains(LocationTag::Inside),
            "Check out the location: {location_display}"
        );
    }
}
