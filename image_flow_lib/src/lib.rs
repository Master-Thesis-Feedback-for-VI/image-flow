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

use tokio::task::JoinSet;

pub use crate::error::{Error, Result};

pub(crate) mod image_processor;

#[cfg(test)]
#[allow(dead_code)]
pub(crate) mod tests;

uniffi::setup_scaffolding!();

use crate::{
    context::{
        ContextCategory, UniqueGeographicCategories,
        image_based_context::{FramePath, ImageDetections, evaluate_image_into_categories},
        location_based_context::evaluate_location_into_categories,
    },
    error::ImageFlowError,
    location::coordinate_system::Coordinate,
};
use std::path::Path;
use std::result::Result as DefaultResult;

/// TODO
///
/// # Errors
/// TODO
#[allow(clippy::needless_pass_by_value)]
#[uniffi::export]
pub async fn evaluate_context(
    location: Coordinate,
    frame: String,
) -> DefaultResult<Vec<ContextCategory>, ImageFlowError> {
    let categories = evaluate_context_impl(location, frame)
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
pub async fn evaluate_context_impl(
    location: impl Into<Coordinate>,
    frame: impl AsRef<Path>,
) -> crate::Result<UniqueGeographicCategories> {
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

    let mut set = JoinSet::new();

    let location = location.into();
    let frame = frame
        .as_ref()
        .to_path_buf();
    set.spawn(async move {
        tracing::info!("Running location evaluation!");
        let loc = evaluate_location_into_categories(location).await;
        tracing::info!("Finished location evaluation");
        loc
    });

    set.spawn(async move {
        tracing::info!("Running image evaluation!");
        let img = evaluate_image_into_categories(frame)
            .await
            .unwrap_or_default();
        tracing::info!("Finished image evaluation");
        img
    });

    let mut collection_categories = UniqueGeographicCategories::new();
    while let Some(category_from_set) = set
        .join_next()
        .await
    {
        if let Ok(mut category_from_set) = category_from_set {
            let detections = category_from_set
                .detections
                .take();
            collection_categories.extend(category_from_set);

            if let Some(detections) = detections {
                let raw_detections = (*detections).clone();
                collection_categories
                    .detections
                    .get_or_insert(ImageDetections::new())
                    .extend(raw_detections);
            }
        }
    }

    let saved_detections = collection_categories
        .detections
        .take();
    let mut categories: UniqueGeographicCategories = collection_categories
        .into_iter()
        .flat_map(|val| {
            val.implied_additional_categories()
                .iter()
                .copied()
        })
        .collect();
    categories.detections = saved_detections;

    Ok(categories)
}
