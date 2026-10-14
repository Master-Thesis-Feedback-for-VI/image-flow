use std::{sync::Arc, time::Duration};

use crate::{
    FilterLevel, ImageFlowEvaluation, ImageFlowEvaluationForRecording,
    error::ImageFlowError,
    image::detections::ImageDetections,
    image_flow_evaluation_read_from_json,
    location::{
        coordinate_system::Coordinate,
        tag::{LocationTag, UniqueLocationTags},
    },
    recognize_location_tags_impl,
};

fn evaluation() -> ImageFlowEvaluation {
    ImageFlowEvaluation::new(
        UniqueLocationTags::new_singular(LocationTag::Forest),
        ImageDetections::new(),
    )
}

#[test]
fn evaluation_accessors_return_independent_copies() {
    let evaluation = evaluation();
    let mut tags = evaluation.location_tags();
    tags.remove(&LocationTag::Forest);
    assert!(
        evaluation
            .location_tags()
            .contains(LocationTag::Forest)
    );
    assert!(
        evaluation
            .image_detections()
            .is_empty()
    );
}

#[tokio::test]
async fn evaluation_json_round_trip() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let path = dir
        .path()
        .join("evaluation.json");
    let original = evaluation();
    original
        .save_as_json(
            path.to_str()
                .expect("UTF-8 path"),
        )
        .await
        .expect("save evaluation");
    let restored: ImageFlowEvaluation = serde_json::from_slice(
        &tokio::fs::read(path)
            .await
            .expect("read evaluation"),
    )
    .expect("deserialize evaluation");
    assert_eq!(
        original
            .location_tags()
            .categories(),
        restored
            .location_tags()
            .categories()
    );
    assert!(
        restored
            .image_detections()
            .is_empty()
    );
}

#[tokio::test]
async fn saving_to_missing_parent_returns_serialization_error() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let path = dir
        .path()
        .join("missing/evaluation.json");
    assert!(matches!(
        evaluation()
            .save_as_json(
                path.to_str()
                    .expect("UTF-8 path")
            )
            .await,
        Err(ImageFlowError::Serialization { .. })
    ));
}

#[tokio::test]
async fn recording_json_preserves_order_offsets_coordinates_and_evaluations() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let path = dir
        .path()
        .join("content.json");
    let shared = Arc::new(evaluation());
    let recordings: Vec<_> = [0, 400, 1200]
        .into_iter()
        .map(|millis| {
            ImageFlowEvaluationForRecording::new(
                shared.clone(),
                Duration::from_millis(millis),
                Coordinate {
                    lat: 52.5,
                    lon: -9.25,
                },
            )
        })
        .collect();
    tokio::fs::write(
        &path,
        serde_json::to_vec(&recordings).expect("serialize recordings"),
    )
    .await
    .expect("write recordings");
    let restored = image_flow_evaluation_read_from_json(
        path.to_str()
            .expect("UTF-8 path"),
    )
    .await
    .expect("read recordings");
    assert_eq!(restored.len(), 3);
    for (actual, expected) in restored
        .iter()
        .zip(recordings)
    {
        assert_eq!(actual.time_diff(), expected.time_diff());
        assert_eq!(
            actual
                .location()
                .lat,
            52.5
        );
        assert_eq!(
            actual
                .location()
                .lon,
            -9.25
        );
        assert!(
            actual
                .evaluation()
                .location_tags()
                .contains(LocationTag::Forest)
        );
    }
    assert!(
        shared
            .location_tags()
            .contains(LocationTag::Forest)
    );
}

#[tokio::test]
async fn recording_reader_handles_empty_missing_malformed_and_wrong_shape_files() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let path = dir
        .path()
        .join("content.json");
    let path_str = path
        .to_str()
        .expect("UTF-8 path");
    assert!(matches!(
        image_flow_evaluation_read_from_json(path_str).await,
        Err(ImageFlowError::Serialization { .. })
    ));
    for content in ["", "not json", "{}", "[{}]"] {
        tokio::fs::write(&path, content)
            .await
            .expect("write fixture");
        assert!(
            matches!(
                image_flow_evaluation_read_from_json(path_str).await,
                Err(ImageFlowError::Serialization { .. })
            ),
            "content: {content}"
        );
    }
    tokio::fs::write(&path, "[]")
        .await
        .expect("write empty recording");
    assert!(
        image_flow_evaluation_read_from_json(path_str)
            .await
            .expect("empty recording")
            .is_empty()
    );
}

#[tokio::test]
async fn missing_location_tag_image_fails_before_network_request() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let error = recognize_location_tags_impl(
        Coordinate { lat: 0.0, lon: 0.0 },
        None,
        dir.path()
            .join("missing.png"),
        4,
    )
    .await
    .expect_err("missing image must fail");
    assert!(matches!(
        ImageFlowError::from(error),
        ImageFlowError::Io { .. }
    ));
}

#[test]
fn all_log_filters_use_tracing_spellings() {
    for (level, name) in [
        (FilterLevel::Off, "off"),
        (FilterLevel::Warn, "warn"),
        (FilterLevel::Info, "info"),
        (FilterLevel::Debug, "debug"),
        (FilterLevel::Trace, "trace"),
        (FilterLevel::Error, "error"),
    ] {
        assert_eq!(level.to_string(), name);
    }
    assert_eq!(FilterLevel::default().to_string(), "off");
}
