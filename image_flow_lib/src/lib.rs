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
use crate::context::image_based_context::ImageDetections;
pub use crate::error::{Error, Result};

pub(crate) mod image_processor;

#[cfg(test)]
#[allow(dead_code)]
pub(crate) mod tests;

uniffi::setup_scaffolding!();

use crate::{
    context::{
        ContextCategory, UniqueGeographicCategories,
        image_based_context::{ImageRecognizer, evaluate_image_into_categories},
        location_based_context::evaluate_location_into_categories,
    },
    error::ImageFlowError,
    location::coordinate_system::Coordinate,
    requesters::AuthToHost,
};
use std::fmt::Display;
use std::path::Path;
use std::{result::Result as DefaultResult, sync::Arc};

/// # Errors
/// TODO
#[uniffi::export]
pub async fn recognize_objects_in_image(
    path: &str,
    image_recognizer: Arc<ImageRecognizer>,
) -> DefaultResult<ImageDetections, ImageFlowError> {
    let detections = recognize_objects_in_image_impl(path, image_recognizer).await?;

    Ok(detections)
}

/// # Errors
/// TODO
#[uniffi::export]
pub async fn recognize_location_tags(
    img_path: &str,
    image_recognizer: Arc<ImageRecognizer>,
) -> DefaultResult<Vec<ContextCategory>, ImageFlowError> {
    let location_tags = recognize_location_tags_impl(img_path, &image_recognizer)
        .await?
        .into_iter()
        .collect();

    Ok(location_tags)
}

// === IMPLEMENTATIONS ===

/// # Errors
/// TODO
pub async fn recognize_objects_in_image_impl(
    path: impl AsRef<Path>,
    image_recognizer: Arc<ImageRecognizer>,
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
    img_path: impl AsRef<Path>,
    image_recognizer: &ImageRecognizer,
) -> crate::Result<UniqueGeographicCategories> {
    todo!()
}

// === DEPRECATED ===

/// TODO
///
/// # Errors
/// TODO
#[allow(clippy::needless_pass_by_value)]
#[uniffi::export(async_runtime = "tokio")]
pub async fn evaluate_context(
    location: Coordinate,
    frame: String,
    image_model: Arc<ImageRecognizer>,
    auth: Option<AuthToHost>,
) -> DefaultResult<Vec<ContextCategory>, ImageFlowError> {
    let img_model = image_model.as_ref();
    let auth = auth.map(Arc::new);
    let categories = evaluate_context_impl(location, frame, img_model, auth)
        .await?
        .into_iter()
        .collect();

    Ok(categories)
}

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
) -> crate::Result<UniqueGeographicCategories> {
    let location = location.into();

    let span = Span::current();
    span.record("location", tracing::field::debug(&location));
    span.record(
        "frame",
        tracing::field::display(
            frame
                .as_ref()
                .display(),
        ),
    );

    if !frame
        .as_ref()
        .exists()
    {
        let err_msg = format!(
            "the file {} does not exist",
            frame
                .as_ref()
                .display()
        );
        return Err(crate::error::file_does_not_exist(err_msg));
    }

    let loc_fut = async {
        tracing::info!("Running location evaluation!");
        let loc_fut = evaluate_location_into_categories(location, auth).await;
        tracing::info!("Finished location evaluation");
        loc_fut
    };

    let img_fut = async {
        tracing::info!("Running image evaluation!");
        let img_fut = evaluate_image_into_categories(frame, model)
            .await
            .unwrap_or_default();
        tracing::info!("Finished image evaluation");
        img_fut
    };

    let (loc, img) = join!(loc_fut, img_fut);

    let mut collection_categories = UniqueGeographicCategories::merge(img, loc).extend_by_implied();
    if collection_categories.is_empty() {
        collection_categories.insert(ContextCategory::Outside);
    }
    Ok(collection_categories)
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
