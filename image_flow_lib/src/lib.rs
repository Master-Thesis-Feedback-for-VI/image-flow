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

pub use crate::error::{Error, Result};

#[cfg(test)]
#[allow(dead_code)]
pub(crate) mod tests;

uniffi::setup_scaffolding!();

use crate::{
    context::{
        ContextCategory, image_based_context::FramePath,
        location_based_context::evaulate_location_into_categories,
    },
    error::ImageFlowError,
    location::coordinate_system::Coordinate,
};
use std::{collections::HashSet, result::Result as DefaultResult};
use std::sync::Arc;

/// TODO
///
/// # Errors
/// TODO
#[allow(clippy::needless_pass_by_value)]
#[uniffi::export]
pub async fn evaluate_context(
    location: Coordinate,
    frame: Arc<FramePath>,
) -> DefaultResult<Vec<ContextCategory>, ImageFlowError> {
    evaluate_context_impl(location, frame.as_ref())
        .await
        .map(Vec::from_iter)
        .map_err(ImageFlowError::from)
}

// === Implementations ===

async fn evaluate_context_impl(
    location: impl Into<Coordinate>,
    frame: &FramePath,
) -> crate::Result<HashSet<ContextCategory>> {
    let categories = evaulate_location_into_categories(location).await?;
    let _ = frame;

    Ok(categories)
}
