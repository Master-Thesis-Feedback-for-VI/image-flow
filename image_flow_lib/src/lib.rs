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
    context::{UniqueGeographicCategories, image_based_context::ImageRecognizer},
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
pub async fn image_flow_evaluation(
    location: Coordinate,
    auth: Option<AuthToHost>,
    img_path: &str,
    image_recognizer: Arc<ImageRecognizer>,
    amount: u32,
) -> crate::Result<ImageFlowEvaluation> {
    let detections_fut = recognize_objects_in_image_impl(img_path, &image_recognizer);
    let location_tags_fut = recognize_location_tags_impl(location, auth, img_path, amount as usize);

    let (detections, location_tags) = tokio::join!(detections_fut, location_tags_fut);

    Ok(ImageFlowEvaluation {
        location_tags,
        image_detections: detections?,
    })
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

#[cfg(test)]
mod test {
    use crate::{
        context::{
            image_based_context::{
                ImageRecognizer,
                ModelVariant::{self, Medium},
            },
            location_based_context::evaluate_image_and_location_as_location_tags,
        },
        tests::TESTING_FILE_MANAGER,
    };

    #[tokio::test]
    async fn check_images_with_data() {
        todo!()
    }
}
