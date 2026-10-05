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
pub mod location;
pub mod requesters;
#[cfg(feature = "video")]
pub mod video_slicer;

use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

pub use crate::context::image_based_context::FramePath;
pub use crate::error::{Error, Result};
use crate::{
    context::image_based_context::evaluate_image_into_location_tags,
    image_processor::DominantColors,
};
use crate::{
    context::{
        image_based_context::ImageDetections,
        location_based_context::evaluate_image_and_location_as_location_tags,
    },
    image_processor::dominant_colors,
};

pub(crate) mod image_processor;

#[cfg(test)]
#[allow(dead_code)]
pub(crate) mod tests;

uniffi::setup_scaffolding!();

use crate::{
    context::{UniqueLocationTags, image_based_context::ImageRecognizer},
    error::ImageFlowError,
    location::coordinate_system::Coordinate,
    requesters::AuthToHost,
};
use std::fmt::Display;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::{result::Result as DefaultResult, sync::Arc};

#[derive(Debug, Clone, uniffi::Object, Deserialize, Serialize)]
pub struct ImageFlowEvaluation {
    location_tags: UniqueLocationTags,
    image_detections: ImageDetections,
}

#[derive(Serialize, Deserialize, Clone, Debug, uniffi::Object)]
pub struct ImageFlowEvaluationForRecording {
    pub evaluations: ImageFlowEvaluation,
    pub time_diff: Duration,
    pub location: Coordinate,
}

/// # Errors
/// TODO
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

/// # Errors
/// TODO
#[uniffi::export(async_runtime = "tokio")]
pub async fn image_flow_evaluation(
    location: Coordinate,
    auth: Option<AuthToHost>,
    img_path: &str,
    image_recognizer: Arc<ImageRecognizer>,
    amount: u32,
) -> DefaultResult<ImageFlowEvaluation, ImageFlowError> {
    let img_flow =
        image_flow_evaluation_impl(location, auth, img_path, &image_recognizer, amount as usize)
            .await?;

    Ok(img_flow)
}

/// # Errors
/// TODO
#[uniffi::export(async_runtime = "tokio")]
pub async fn recognize_objects_in_image(
    path: &str,
    image_recognizer: Arc<ImageRecognizer>,
) -> DefaultResult<ImageDetections, ImageFlowError> {
    let detections = recognize_objects_in_image_impl(path, &image_recognizer).await?;

    Ok(detections)
}

/// # Errors
/// TODO
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

#[derive(uniffi::Enum, Clone, Copy, Debug, Default)]
pub enum FilterLevel {
    #[default]
    Off,
    Warn,
    Info,
    Debug,
    Trace,
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

/// # Errors
/// TODO
pub async fn recognize_objects_in_image_impl(
    path: impl AsRef<Path>,
    image_recognizer: &ImageRecognizer,
) -> crate::Result<ImageDetections> {
    let path = path.as_ref();
    if !path.exists() {
        let err_msg = format!("the file {} does not exist", path.display());
        return Err(crate::error::file_does_not_exist(err_msg));
    }

    image_recognizer
        .predict(path)
        .await
}

/// # Errors
/// TODO
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

/// # Errors
/// TODO
pub async fn image_flow_evaluation_impl(
    location: impl Into<Coordinate>,
    auth: Option<AuthToHost>,
    img_path: impl AsRef<Path>,
    image_recognizer: &ImageRecognizer,
    amount: usize,
) -> crate::Result<ImageFlowEvaluation> {
    let img_path = img_path
        .as_ref()
        .to_owned();
    let location = location.into();
    if !img_path.exists() {
        let err_msg = format!("the file {} does not exist", img_path.display());
        return Err(crate::error::file_does_not_exist(err_msg));
    }

    let detections_fut = recognize_objects_in_image_impl(img_path.clone(), image_recognizer);

    let location_tags_dom_colors_fut: JoinHandle<
        crate::Result<(UniqueLocationTags, DominantColors)>,
    > = tokio::spawn(async move {
        let dominant_colors = dominant_colors(img_path, amount).await?;
        let location_tags = evaluate_image_and_location_as_location_tags(
            location,
            auth.map(Arc::new),
            &dominant_colors,
        )
        .await;

        Ok((location_tags, dominant_colors))
    });

    let (image_detections, location_tags_dom_colors) =
        tokio::join!(detections_fut, location_tags_dom_colors_fut);
    let image_detections = image_detections?;
    let (mut location_tags, dominant_colors) = location_tags_dom_colors??;

    let additional_categories =
        evaluate_image_into_location_tags(&dominant_colors, &image_detections);
    location_tags.extend(additional_categories);

    Ok(ImageFlowEvaluation {
        location_tags,
        image_detections,
    })
}

#[uniffi::export(async_runtime = "tokio")]
impl ImageFlowEvaluation {
    #[uniffi::constructor]
    #[must_use]
    pub const fn new(location_tags: UniqueLocationTags, image_detections: ImageDetections) -> Self {
        Self {
            location_tags,
            image_detections,
        }
    }

    /// # Errors
    /// TODO
    pub async fn save_as_json(&self, path: &str) -> DefaultResult<(), ImageFlowError> {
        let json = serde_json::to_string_pretty(self).map_err(crate::error::serialization)?;

        tokio::fs::write(path, json)
            .await
            .map_err(crate::error::serialization)?;

        Ok(())
    }

    #[must_use]
    pub fn location_tags(&self) -> UniqueLocationTags {
        self.location_tags
            .clone()
    }

    #[must_use]
    pub fn image_detections(&self) -> ImageDetections {
        self.image_detections
            .clone()
    }
}

#[uniffi::export]
impl ImageFlowEvaluationForRecording {
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

    #[must_use]
    pub fn evaluation(&self) -> ImageFlowEvaluation {
        self.evaluations
            .clone()
    }

    #[must_use]
    pub const fn time_diff(&self) -> Duration {
        self.time_diff
    }

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
        context::{LocationTag, image_based_context::ImageRecognizer},
        image_flow_evaluation, recognize_location_tags, recognize_objects_in_image,
        tests::{AUTH_AGENT, DOMINANT_COLOR_AMOUNT, fetch_img},
    };

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
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

        let evaluation = image_flow_evaluation(location, auth, img_path, model, amount)
            .await
            .unwrap();

        insta::assert_snapshot!(evaluation);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
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
        )
        .await
        .unwrap();

        insta::assert_debug_snapshot!(evaluation);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
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
