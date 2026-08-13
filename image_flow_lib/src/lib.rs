#![feature(const_trait_impl)]

#[allow(clippy::too_long_first_doc_paragraph)]
pub mod context_recognition;
pub mod error;

pub use error::{Error, Result};

uniffi::setup_scaffolding!();
