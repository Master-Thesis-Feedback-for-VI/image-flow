#![feature(const_trait_impl)]

#[allow(clippy::too_long_first_doc_paragraph)]
pub mod context_recognition;
pub mod error;
pub mod requesters;
pub mod video_slicer;

pub use error::{Error, Result};

#[cfg(test)]
#[allow(dead_code)]
pub(crate) mod tests;

uniffi::setup_scaffolding!();
