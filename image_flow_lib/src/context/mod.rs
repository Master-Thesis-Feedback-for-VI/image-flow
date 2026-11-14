//! Geographic classification and image inference.
//!
//! [`image_based_context::ImageRecognizer`] owns the object and depth models.
//! [`location_based_context`] combines OpenStreetMap evidence with image-color
//! hints; most callers should use the top-level evaluation functions.

pub mod image_based_context;
pub mod location_based_context;
