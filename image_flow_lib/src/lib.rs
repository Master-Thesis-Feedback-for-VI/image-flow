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

#[cfg(test)]
#[allow(dead_code)]
pub(crate) mod tests;

use std::{path::PathBuf, sync::Arc};

pub use crate::error::{Error, Result};

use crate::{context::ContextCategory, location::coordinate_system::Coordinate};

uniffi::setup_scaffolding!();

#[allow(clippy::needless_pass_by_value)]
#[uniffi::export]
#[must_use]
pub fn evaluate_context(location: Coordinate, frame: Arc<FramePath>) -> ContextCategory {
    let _ = location;
    let _ = frame;
    todo!()
}

#[derive(Debug, uniffi::Object)]
pub struct FramePath {
    pub time_offset_from_first_frame: chrono::Duration,
    path: PathBuf,
    should_drop: bool,
}

impl Drop for FramePath {
    fn drop(&mut self) {
        if self.should_drop {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
