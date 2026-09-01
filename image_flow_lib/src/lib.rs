#![feature(const_trait_impl)]
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
    context::{ContextCategory, location_based_context::evaulate_location_into_categories},
    error::ImageFlowError,
    location::coordinate_system::Coordinate,
};
use chrono::Duration;
use std::time::Duration as DefaultDuration;
use std::{collections::HashSet, result::Result as DefaultResult};
use std::{path::PathBuf, sync::Arc};

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

#[derive(Debug, uniffi::Object)]
pub struct FramePath {
    pub time_offset_from_first_frame: Duration,
    path: PathBuf,
    pub should_drop: bool,
}

#[uniffi::export]
impl FramePath {
    /// TODO
    ///
    /// # Panics
    /// TODO
    #[uniffi::constructor]
    #[must_use]
    pub fn new(offset: DefaultDuration, path: &str) -> Self {
        Self {
            time_offset_from_first_frame: Duration::from_std(offset)
                .expect("The time conversion should not fail"),
            path: PathBuf::from(path),
            should_drop: true,
        }
    }
}

// === Implementations ===

async fn evaluate_context_impl(
    location: impl Into<Coordinate>,
    frame: &FramePath,
) -> crate::Result<HashSet<ContextCategory>> {
    let mut categories = evaulate_location_into_categories(location).await?;
    let _ = frame;

    Ok(categories)
}

impl Drop for FramePath {
    fn drop(&mut self) {
        if self.should_drop {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
