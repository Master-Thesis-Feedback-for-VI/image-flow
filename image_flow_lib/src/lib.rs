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
use std::{result::Result as DefaultResult, sync::Arc};

#[derive(Debug, uniffi::Record)]
pub struct ImageFlowEvaluation {
    location_tags: UniqueLocationTags,
    image_detections: ImageDetections,
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

// TODO improve performance by parallalizing here
async fn image_flow_evaluation_impl(
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

        insta::assert_debug_snapshot!(evaluation);
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

        insta::assert_debug_snapshot!(evaluation, @"
        UniqueLocationTags {
            categories: {
                Inside,
            },
            is_inside: true,
        }
        ");

        let luetzeken = fetch_img("luetzekenstrasse.png").await;

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
        assert!(evaluation.contains(LocationTag::Inside));
    }
}
