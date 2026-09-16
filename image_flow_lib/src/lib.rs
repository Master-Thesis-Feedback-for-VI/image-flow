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

use tokio::join;
use tracing::{Span, instrument};

pub use crate::error::{Error, Result};

pub(crate) mod image_processor;

#[cfg(test)]
#[allow(dead_code)]
pub(crate) mod tests;

uniffi::setup_scaffolding!();

use crate::{
    context::{
        ContextCategory, UniqueGeographicCategories,
        image_based_context::{FramePath, ImageRecognizer, evaluate_image_into_categories},
        location_based_context::evaluate_location_into_categories,
    },
    error::ImageFlowError,
    location::coordinate_system::Coordinate,
    requesters::AuthToHost,
};
use std::path::Path;
use std::{result::Result as DefaultResult, sync::Arc};

/// TODO
///
/// # Errors
/// TODO
#[allow(clippy::needless_pass_by_value)]
#[uniffi::export(async_runtime = "tokio")]
pub async fn evaluate_context(
    location: Coordinate,
    frame: String,
    image_model: Arc<ImageRecognizer>,
    auth: Option<AuthToHost>,
) -> DefaultResult<Vec<ContextCategory>, ImageFlowError> {
    let img_model = image_model.as_ref();
    let auth = auth.map(Arc::new);
    let categories = evaluate_context_impl(location, frame, img_model, auth)
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
#[instrument(
    skip_all,
    fields(
        location = tracing::field::Empty,
        frame = tracing::field::Empty,
    )
)]
pub async fn evaluate_context_impl(
    location: impl Into<Coordinate>,
    frame: impl AsRef<Path>,
    model: &ImageRecognizer,
    auth: Option<Arc<AuthToHost>>,
) -> crate::Result<UniqueGeographicCategories> {
    let location = location.into();

    let span = Span::current();
    span.record("location", tracing::field::debug(&location));
    span.record(
        "frame",
        tracing::field::display(
            frame
                .as_ref()
                .display(),
        ),
    );

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

    let loc_fut = async {
        tracing::info!("Running location evaluation!");
        let loc_fut = evaluate_location_into_categories(location, auth).await;
        tracing::info!("Finished location evaluation");
        loc_fut
    };

    let img_fut = async {
        tracing::info!("Running image evaluation!");
        let img_fut = evaluate_image_into_categories(frame, model)
            .await
            .unwrap_or_default();
        tracing::info!("Finished image evaluation");
        img_fut
    };

    let (loc, img) = join!(loc_fut, img_fut);

    let collection_categories = UniqueGeographicCategories::merge(img, loc).extend_by_implied();
    Ok(collection_categories)
}
