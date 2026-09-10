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
        ContextCategory,
        image_based_context::{FramePath, evaluate_image_into_categories},
        location_based_context::evaluate_location_into_categories,
    },
    location::coordinate_system::Coordinate,
};
use std::collections::HashSet;
use std::sync::Arc;

/// TODO
///
/// # Errors
/// TODO
#[allow(clippy::needless_pass_by_value)]
#[uniffi::export]
pub async fn evaluate_context(location: Coordinate, frame: Arc<FramePath>) -> Vec<ContextCategory> {
    evaluate_context_impl(location, frame.as_ref())
        .await
        .into_iter()
        .collect()
}

// === Implementations ===

async fn evaluate_context_impl(
    location: impl Into<Coordinate>,
    frame: &FramePath,
) -> HashSet<ContextCategory> {
    let mut set = JoinSet::new();

    let location = location.into();
    let frame = frame.clone();
    set.spawn(async move { evaluate_location_into_categories(location).await });
    set.spawn(async move {
        evaluate_image_into_categories(frame)
            .await
            .unwrap_or_default()
    });

    let mut collection_categories = HashSet::new();
    while let Some(category_from_set) = set
        .join_next()
        .await
    {
        if let Ok(category_from_set) = category_from_set {
            collection_categories.extend(category_from_set);
        }
    }

    let mut categories: HashSet<ContextCategory> = collection_categories
        .into_iter()
        .flat_map(|val| {
            val.implied_additional_categories()
                .iter()
                .copied()
        })
        .collect();

    if categories.contains(&ContextCategory::Building) {
        categories = ContextCategory::remove_outside_categories(categories);
    }

    categories
}
