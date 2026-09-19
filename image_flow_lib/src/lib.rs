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

use tokio::join;
use tracing::{Span, instrument};

pub use crate::context::image_based_context::FramePath;
use crate::context::{
    image_based_context::ImageDetections,
    location_based_context::evaluate_image_and_location_as_location_tags,
};
pub use crate::error::{Error, Result};

pub(crate) mod image_processor;

#[cfg(test)]
#[allow(dead_code)]
pub(crate) mod tests;

uniffi::setup_scaffolding!();

use crate::{
    context::{ContextCategory, UniqueGeographicCategories, image_based_context::ImageRecognizer},
    error::ImageFlowError,
    location::coordinate_system::Coordinate,
    requesters::AuthToHost,
};
use std::fmt::Display;
use std::path::Path;
use std::{result::Result as DefaultResult, sync::Arc};

#[derive(Debug, uniffi::Record)]
pub struct ImageFlowEvaluation {
    location_tags: UniqueGeographicCategories,
    image_detections: ImageDetections,
}

/// # Errors
/// TODO
#[uniffi::export]
pub async fn recognize_objects_in_image(
    path: &str,
    image_recognizer: Arc<ImageRecognizer>,
) -> DefaultResult<ImageDetections, ImageFlowError> {
    let detections = recognize_objects_in_image_impl(path, &image_recognizer).await?;

    Ok(detections)
}

/// # Errors
/// TODO
#[uniffi::export]
pub async fn recognize_location_tags(
    location: Coordinate,
    auth: Option<AuthToHost>,
    img_path: &str,
    amount: u32,
) -> UniqueGeographicCategories {
    recognize_location_tags_impl(location, auth, img_path, amount as usize).await
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

async fn recognize_location_tags_impl(
    location: impl Into<Coordinate>,
    auth: Option<AuthToHost>,
    img_path: impl AsRef<Path>,
    amount: usize,
) -> UniqueGeographicCategories {
    let auth = auth.map(Arc::new);
    evaluate_image_and_location_as_location_tags(location, auth, img_path, amount).await
}

// === DEPRECATED ===

/// TODO
///
/// # Errors
/// TODO
// #[allow(clippy::needless_pass_by_value)]
// #[uniffi::export(async_runtime = "tokio")]
// pub async fn evaluate_context(
//     location: Coordinate,
//     frame: String,
//     image_model: Arc<ImageRecognizer>,
//     auth: Option<AuthToHost>,
// ) -> DefaultResult<ImageFlowEvaluation, ImageFlowError> {
//     let img_model = image_model.as_ref();
//     let auth = auth.map(Arc::new);
//     let categories = evaluate_context_impl(location, frame, img_model, auth).await?;

//     Ok(categories)
// }

// === Implementations ===

/// TODO
///
/// # Errors
/// TODO
#[instrument(
    skip_all,
    fields(
        location = tracing::field::Empty,
        frame = tracing::field::Empty,
    )
)]
pub async fn evaluate_context_impl(
    location: impl Into<Coordinate>,
    frame: impl AsRef<Path>,
    model: &ImageRecognizer,
    auth: Option<Arc<AuthToHost>>,
) -> crate::Result<ImageFlowEvaluation> {
    let location = location.into();
    let frame = frame.as_ref();

    let span = Span::current();
    span.record("location", tracing::field::debug(&location));
    span.record("frame", tracing::field::display(frame.display()));

    if !frame.exists() {
        let err_msg = format!("the file {} does not exist", frame.display());
        return Err(crate::error::file_does_not_exist(err_msg));
    }

    let loc_fut = async {
        tracing::info!("Running location evaluation!");
        let loc_fut = evaluate_image_and_location_as_location_tags(location, auth, frame, 4).await;
        tracing::info!("Finished location evaluation");
        loc_fut
    };

    let img_fut = async {
        tracing::info!("Running image evaluation!");
        let img_fut = recognize_objects_in_image_impl(frame, model)
            .await
            .unwrap_or_default();
        tracing::info!("Finished image evaluation");
        img_fut
    };

    let (loc, img) = join!(loc_fut, img_fut);

    let mut collection_categories = loc.extend_by_implied();
    if collection_categories.is_empty() {
        collection_categories.insert(ContextCategory::Outside);
    }

    Ok(ImageFlowEvaluation {
        location_tags: collection_categories,
        image_detections: img,
    })
}

#[must_use]
#[uniffi::export]
pub fn enable_tracing(rust_log: Option<FilterLevel>) -> bool {
    let rust_log = rust_log.unwrap_or_default();
    let tag = rust_log.to_string();

    tracing_subscriber::fmt()
        .with_env_filter(tag)
        .with_target(true)
        .with_thread_ids(true)
        .try_init()
        .is_ok()
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
