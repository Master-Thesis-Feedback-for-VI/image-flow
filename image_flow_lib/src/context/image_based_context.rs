use std::path::{Path, PathBuf};
use std::time::Duration as DefaultDuration;

use chrono::Duration;

use crate::context::location_based_context::UniqueGeographicCategories;

/// TODO
///
/// # Errors
/// TODO
pub fn evaluate_image_into_categories() -> crate::Result<UniqueGeographicCategories> {
    todo!();

    let ctx = UniqueGeographicCategories::new();
    Ok(ctx)
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
        let dur = Duration::from_std(offset).expect("The time conversion should not fail");
        Self::new_impl(dur, path)
    }
}

impl FramePath {
    pub fn new_impl(offset: Duration, path: impl AsRef<Path>) -> Self {
        Self {
            time_offset_from_first_frame: offset,
            path: path
                .as_ref()
                .to_path_buf(),
            should_drop: true,
        }
    }
}

impl AsRef<Path> for FramePath {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl Drop for FramePath {
    fn drop(&mut self) {
        if self.should_drop {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
