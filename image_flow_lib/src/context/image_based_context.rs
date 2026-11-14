//! Reusable ONNX model sessions and temporary frame-file ownership.
//!
//! Construct an [`ImageRecognizer`] once and reuse it. Model files are loaded
//! from an existing directory or downloaded if missing. [`FramePath`] is also
//! available without the `video` feature and deletes its file on drop by default.

use std::path::{Path, PathBuf};
use std::result::Result as DefaultResult;
use std::sync::Arc;
use std::time::Duration as DefaultDuration;

use chrono::Duration;
use reqwest::Url;
use tokio::io::{AsyncWriteExt as _, BufWriter};
use tokio::sync::Mutex;
use tokio_stream::StreamExt;
use tracing::instrument;
use ultralytics_inference::{Results as DetectionResults, YOLOModel};

use crate::error::ImageFlowError;
use crate::image::depth::DepthMap;
use crate::image::detections::{DetectionType, ImageDetections};
use crate::image_processor::DominantColors;
use crate::location::tag::{LocationTag, UniqueLocationTags};

const TRAFFIC_LIGHT_THRESHOLD: usize = 8;
// const OUTSIDE_THRESHOLD: &[(DetectionType, usize)] = &[
//     (DetectionType::Person, 7),
//     (DetectionType::TrafficLight, 1),
//     (DetectionType::Car, 1),
//     (DetectionType::Bench, 2),
//     (DetectionType::Bicycle, 1),
//     (DetectionType::DiningTable, 2),
// ];

#[instrument]
#[must_use]
pub(crate) fn evaluate_image_into_location_tags(
    dominant_colors: &DominantColors,
    detections: &ImageDetections,
) -> UniqueLocationTags {
    let mut categories = UniqueLocationTags::new();

    // let is_outside = detections.is_outside(OUTSIDE_THRESHOLD);
    // if is_outside {
    //     categories.insert(LocationTag::Outside);
    // }

    let intersection_count = detections
        .get_by_detected_object(DetectionType::TrafficLight)
        .count();
    if intersection_count > TRAFFIC_LIGHT_THRESHOLD {
        categories.extend([LocationTag::Intersection, LocationTag::Outside]);
    }

    let in_forest = dominant_colors.any_close_to_forest_green();
    if in_forest {
        categories.extend([LocationTag::Outside, LocationTag::Forest]);
    }
    categories
}

/// Internal state for an object-detection session and its model file.
///
/// Managed by [`ImageRecognizer`]; this type has no public constructor.
#[derive(Debug)]
pub struct ObjectDetectionModel {
    path: PathBuf,
    model: Arc<Mutex<YOLOModel>>,
}

/// Internal state for a depth-estimation session and its model file.
///
/// Managed by [`ImageRecognizer`]; this type has no public constructor.
#[derive(Debug)]
pub struct DepthDetectionModel {
    path: PathBuf,
    model: Arc<Mutex<YOLOModel>>,
}

/// Reusable object and depth models with independently locked inference sessions.
///
/// Construction always loads both models. Inference runs on Tokio blocking
/// workers; requests sharing this recognizer serialize access to each model.
#[derive(Debug, uniffi::Object)]
pub struct ImageRecognizer {
    /// Variant selected at construction; changing this field does not reload models.
    pub variant: ModelVariant,
    obj_detection: Arc<ObjectDetectionModel>,
    depth_recognition: Arc<DepthDetectionModel>,
}

#[uniffi::export(async_runtime = "tokio")]
impl ImageRecognizer {
    /// Load both models of `model_variant` from an existing directory.
    ///
    /// Missing files are downloaded. The parameter `downlaod_path` retains its
    /// current API spelling. See [`Self::new_impl`] for filenames and behavior.
    ///
    /// # Errors
    /// Returns an error if the directory does not exist, downloads or file
    /// operations fail, or either model cannot be loaded.
    #[uniffi::constructor]
    pub async fn new(
        model_variant: ModelVariant,
        downlaod_path: String,
    ) -> DefaultResult<Self, ImageFlowError> {
        let img_recognizer = Self::new_impl(model_variant, downlaod_path).await?;
        Ok(img_recognizer)
    }

    /// Return the loaded object model's filesystem path as a string.
    ///
    /// # Panics
    /// Panics if the path contains non-UTF-8 bytes.
    #[must_use]
    pub fn object_detection_model_path(&self) -> String {
        let tmp = self
            .obj_detection
            .path
            .clone();
        tmp.into_os_string()
            .into_string()
            .expect("Invalid UTF-8")
    }

    /// Return the loaded depth model's filesystem path as a string.
    ///
    /// # Panics
    /// Panics if the path contains non-UTF-8 bytes.
    #[must_use]
    pub fn depth_detection_model_path(&self) -> String {
        let tmp = self
            .depth_recognition
            .path
            .clone();
        tmp.into_os_string()
            .into_string()
            .expect("Invalid UTF-8")
    }
}

impl ImageRecognizer {
    /// Rust path-based constructor for both inference models.
    ///
    /// `download_path` must already be a directory. Existing files are loaded
    /// unchanged; missing files are downloaded from the configured Ultralytics
    /// `v8.4.0` release through `.part` files. Both models load concurrently.
    /// See [`ModelVariant`] for the expected local filenames. To avoid network
    /// requests, provide both compatible ONNX files before calling this method.
    ///
    /// # Errors
    /// Returns an error for a missing directory, failed HTTP/download validation,
    /// file-access failures, failed worker tasks, or invalid/unsupported models.
    #[instrument(skip(download_path), fields(download_path = tracing::field::Empty))]
    pub async fn new_impl(
        variant: ModelVariant,
        download_path: impl AsRef<Path>,
    ) -> crate::Result<Self> {
        let path = download_path
            .as_ref()
            .to_path_buf();

        let span = tracing::Span::current();
        span.record("download_path", tracing::field::debug(&path));

        if !path.is_dir() {
            let msg = format!("The dir: {}, does not exist", path.display());
            return Err(crate::error::io(msg));
        }

        let (obj_url, depth_url) = variant.build_urls();

        let obj_detection_file_path = path.join(variant.object_detection_model_name());
        let obj_detection_model_fut = Self::load_or_download(&obj_detection_file_path, obj_url);

        let depth_detection_file_path = path.join(variant.depth_detection_model_name());
        let depth_detection_model_fut =
            Self::load_or_download(&depth_detection_file_path, depth_url);

        let (obj_detection_model, depth_detection_model) =
            tokio::try_join!(obj_detection_model_fut, depth_detection_model_fut)?;

        let obj_detection = ObjectDetectionModel {
            path: obj_detection_file_path,
            model: Arc::new(Mutex::new(obj_detection_model)),
        };
        let depth_detection = DepthDetectionModel {
            path: depth_detection_file_path,
            model: Arc::new(Mutex::new(depth_detection_model)),
        };

        tracing::info!("Sucessfully Loaded both models");
        Ok(Self {
            variant,
            obj_detection: obj_detection.into(),
            depth_recognition: depth_detection.into(),
        })
    }

    async fn load_or_download(
        file_path: impl AsRef<Path>,
        download_url: Url,
    ) -> crate::Result<YOLOModel> {
        let file_path = file_path.as_ref();
        if file_path.is_file() {
            let load_path = file_path.to_path_buf();
            let model = tokio::task::spawn_blocking(move || -> crate::Result<YOLOModel> {
                Ok(YOLOModel::load(load_path)?)
            })
            .await??;

            return Ok(model);
        }

        let tmp_path = file_path.with_extension("part");

        if let Err(e) = Self::download_to(&tmp_path, download_url).await {
            // Best-effort cleanup of any partial temp file.
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(e);
        }

        tokio::fs::rename(&tmp_path, &file_path)
            .await
            .map_err(|e| crate::error::io(format!("Failed to finalize downloaded file: {e}")))?;

        let load_path = file_path.to_path_buf();
        let model = tokio::task::spawn_blocking(move || -> crate::Result<YOLOModel> {
            Ok(YOLOModel::load(load_path)?)
        })
        .await??;

        Ok(model)
    }

    /// Load or download the medium object and depth models in the current directory.
    ///
    /// This is an async convenience function, not a `Default` trait implementation.
    ///
    /// # Errors
    /// Returns the directory, download, or model-loading errors from [`Self::new_impl`].
    pub async fn default() -> crate::Result<Self> {
        let model_variant = ModelVariant::Medium;
        let download_path = Path::new(".");

        Self::new_impl(model_variant, download_path).await
    }

    /// Run object and depth inference concurrently, then postprocess detections.
    ///
    /// `confidence_limit` is clamped to `0.0..=1.0` before filtering. Objects below
    /// the clamped threshold are discarded. Retained objects receive depth
    /// sampled at their box centers, then person/bicycle pairs are merged into
    /// cyclists and same-class duplicate boxes are removed.
    ///
    /// # Errors
    /// Returns an error for failed predictions or worker tasks, or missing depth
    /// data in the depth model's result.
    ///
    /// # Panics
    /// Panics if either backend returns other than exactly one image result, or
    /// if a retained box center is outside the depth map.
    pub async fn predict_objects(
        &self,
        path: &Path,
        confidence_limit: f32,
    ) -> crate::Result<ImageDetections> {
        let path_to_pred = path.to_path_buf();
        let obj_detection_model = Arc::clone(&self.obj_detection);

        let object_prediction_fut =
            tokio::task::spawn_blocking(move || -> crate::Result<Vec<DetectionResults>> {
                let predictions = obj_detection_model
                    .model
                    .blocking_lock()
                    .predict(path_to_pred)?;

                Ok(predictions)
            });

        let depth_pred_fut = self.predict_depth(path);

        let (object_prediction, depth_pred) = tokio::join!(object_prediction_fut, depth_pred_fut);
        let mut object_prediction = object_prediction??;
        assert_eq!(object_prediction.len(), 1);
        let result = object_prediction.remove(0);
        let depth_pred = depth_pred?;

        let image_detections =
            ImageDetections::from_detection_result(&result, &depth_pred, confidence_limit);
        Ok(image_detections)
    }

    /// Run depth inference and return the per-pixel values at image resolution.
    ///
    /// Values are passed through from the model without further calibration.
    ///
    /// # Errors
    /// Returns an error if prediction or its worker task fails, or the result
    /// does not contain depth data.
    ///
    /// # Panics
    /// Panics if the backend does not return exactly one image result.
    pub async fn predict_depth(&self, path: impl AsRef<Path>) -> crate::Result<DepthMap> {
        let path = path
            .as_ref()
            .to_path_buf();
        let depth_detection_model = Arc::clone(&self.depth_recognition);

        let mut predictions =
            tokio::task::spawn_blocking(move || -> crate::Result<Vec<DetectionResults>> {
                let predictions = depth_detection_model
                    .model
                    .blocking_lock()
                    .predict(path)?;

                Ok(predictions)
            })
            .await??;
        assert_eq!(predictions.len(), 1);

        let result = predictions.remove(0);
        let depth_map = DepthMap::try_from(result)?;

        Ok(depth_map)
    }

    /// Streams the model to `tmp_path`. Verifies size when the server reports
    /// `Content-Length`, but doesn't hard-fail if that header is absent (some
    /// CDNs/chunked responses omit it).
    async fn download_to(tmp_path: &Path, download_url: Url) -> crate::Result<()> {
        let client = reqwest::Client::builder()
            .timeout(DefaultDuration::from_mins(5))
            .build()
            .map_err(crate::error::request)?;

        let response = client
            .get(download_url)
            .send()
            .await
            .map_err(crate::error::request)?;

        if !response
            .status()
            .is_success()
        {
            let status = response.status();
            return Err(crate::error::request(format!(
                "Download failed with status: {status}"
            )));
        }

        let expected_size = response.content_length();

        let file = tokio::fs::File::create(tmp_path).await?;
        let mut writer = BufWriter::new(file);

        let mut stream = response.bytes_stream();
        let mut written: u64 = 0;

        while let Some(chunk) = stream
            .next()
            .await
        {
            let chunk = chunk.map_err(crate::error::request)?;
            writer
                .write_all(&chunk)
                .await
                .map_err(|e| crate::error::io(format!("Failed to write chunk: {e}")))?;
            written += chunk.len() as u64;
        }

        writer
            .flush()
            .await
            .map_err(|e| crate::error::io(format!("Failed to flush file: {e}")))?;

        if let Some(expected) = expected_size
            && written != expected
        {
            return Err(crate::error::io(format!(
                "Downloaded file size mismatch: expected {expected}, got {written}"
            )));
        }

        if written == 0 {
            return Err(crate::error::io("Downloaded file is empty".to_string()));
        }

        Ok(())
    }
}

/// Model size and the corresponding filenames loaded by [`ImageRecognizer`].
///
/// | Variant | Object model | Depth model |
/// | --- | --- | --- |
/// | `Nano` | `yolo26n.onnx` | `yolo26n-depth.onnx` |
/// | `Small` | `yolo26s.onnx` | `yolo26s-depth.onnx` |
/// | `Medium` | `yolo26m.onnx` | `yolo26m-depth.onnx` |
/// | `Large` | `yolo26l.onnx` | `yolo26l-depth.onnx` |
/// | `XLarge` | `yolo26x.onnx` | `yolo26x-depth.onnx` |
///
/// Missing assets are requested from the configured release; availability is
/// checked only when a download is attempted.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum ModelVariant {
    /// Nano model pair.
    Nano,
    /// Small model pair.
    Small,
    /// Medium model pair, used by [`ImageRecognizer::default`].
    Medium,
    /// Large model pair.
    Large,
    /// Extra-large model pair.
    XLarge,
}

impl ModelVariant {
    fn base_url() -> Url {
        let base_url = "https://github.com/ultralytics/assets/releases/download/v8.4.0/";

        Url::parse(base_url).expect("We know this cant fail since the above url is valid")
    }

    const fn object_detection_model_name(&self) -> &str {
        match self {
            Self::Nano => "yolo26n.onnx",
            Self::Small => "yolo26s.onnx",
            Self::Medium => "yolo26m.onnx",
            Self::Large => "yolo26l.onnx",
            Self::XLarge => "yolo26x.onnx",
        }
    }

    const fn depth_detection_model_name(&self) -> &str {
        match self {
            Self::Nano => "yolo26n-depth.onnx",
            Self::Small => "yolo26s-depth.onnx",
            Self::Medium => "yolo26m-depth.onnx",
            Self::Large => "yolo26l-depth.onnx",
            Self::XLarge => "yolo26x-depth.onnx",
        }
    }

    fn build_urls(&self) -> (Url, Url) {
        let base_url = Self::base_url();
        let url_obj = base_url
            .join(self.object_detection_model_name())
            .expect("The model filename is a valid relative URL");
        let url_depth = base_url
            .join(self.depth_detection_model_name())
            .expect("The model filename is a valid relative URL");

        (url_obj, url_depth)
    }
}

/// A frame path and its recording-relative offset, with automatic file cleanup.
///
/// Dropping this object attempts to delete the file unless [`Self::should_drop`]
/// is `false`. Clones copy both the path and cleanup flag; they do not coordinate
/// ownership, so dropping any clone with cleanup enabled can delete the file.
/// Access the path through `AsRef<Path>`.
#[derive(Debug, uniffi::Object, Clone)]
pub struct FramePath {
    /// Offset relative to the first timestamped video frame.
    pub time_offset_from_first_frame: Duration,
    path: PathBuf,
    /// Whether dropping this value attempts to delete the file; defaults to `true`.
    pub should_drop: bool,
}

#[uniffi::export]
impl FramePath {
    /// Wrap a path with a standard-library duration, enabling deletion on drop.
    ///
    /// Does not create or validate the file. The path may refer to an existing
    /// caller-owned file, so disable cleanup to preserve it.
    ///
    /// # Panics
    /// Panics if `offset` cannot be represented by `chrono::Duration`.
    #[uniffi::constructor]
    #[must_use]
    pub fn new(offset: DefaultDuration, path: &str) -> Self {
        let dur = Duration::from_std(offset).expect("The time conversion should not fail");
        Self::new_impl(dur, path)
    }
}

impl FramePath {
    /// Wrap a path with a Chrono offset, enabling deletion on drop.
    ///
    /// Does not create or validate the file. Negative offsets are accepted.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::http::TestServer;

    #[test]
    fn every_model_url_uses_its_configured_filename() {
        for variant in [
            ModelVariant::Nano,
            ModelVariant::Small,
            ModelVariant::Medium,
            ModelVariant::Large,
            ModelVariant::XLarge,
        ] {
            let (object, depth) = variant.build_urls();
            for (url, name) in [
                (object, variant.object_detection_model_name()),
                (depth, variant.depth_detection_model_name()),
            ] {
                assert_eq!(url.scheme(), "https");
                assert_eq!(url.host_str(), Some("github.com"));
                assert_eq!(
                    url.as_str(),
                    format!(
                        "https://github.com/ultralytics/assets/releases/download/v8.4.0/{name}"
                    )
                );
            }
        }
    }

    #[tokio::test]
    async fn recognizer_rejects_non_directory_before_downloading() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let file = dir
            .path()
            .join("file");
        std::fs::write(&file, b"fixture").expect("write fixture");
        for path in [
            file,
            dir.path()
                .join("missing"),
        ] {
            assert!(matches!(
                ImageFlowError::from(
                    ImageRecognizer::new_impl(ModelVariant::Nano, path)
                        .await
                        .expect_err("invalid directory")
                ),
                ImageFlowError::Io { .. }
            ));
        }
    }

    #[tokio::test]
    async fn download_writes_exact_response_bytes() {
        let mut server = TestServer::new("200 OK", "model bytes").await;
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir
            .path()
            .join("model.part");
        ImageRecognizer::download_to(&path, Url::parse(&server.url).expect("fixture URL"))
            .await
            .expect("download model fixture");
        assert_eq!(
            tokio::fs::read(path)
                .await
                .expect("read download"),
            b"model bytes"
        );
        assert!(
            server
                .request()
                .await
                .starts_with("GET / HTTP/1.1")
        );
    }

    #[tokio::test]
    async fn download_rejects_http_errors_without_creating_file() {
        let mut server = TestServer::new("404 Not Found", "missing").await;
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir
            .path()
            .join("model.part");
        let error =
            ImageRecognizer::download_to(&path, Url::parse(&server.url).expect("fixture URL"))
                .await
                .expect_err("HTTP error");
        assert!(matches!(
            ImageFlowError::from(error),
            ImageFlowError::Request { .. }
        ));
        assert!(!path.exists());
        server
            .request()
            .await;
    }

    #[tokio::test]
    async fn failed_empty_model_download_removes_partial_file() {
        let mut server = TestServer::new("200 OK", "").await;
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir
            .path()
            .join("model.onnx");
        let error =
            ImageRecognizer::load_or_download(&path, Url::parse(&server.url).expect("fixture URL"))
                .await
                .expect_err("empty model download");
        assert!(matches!(
            ImageFlowError::from(error),
            ImageFlowError::Io { .. }
        ));
        assert!(!path.exists());
        assert!(
            !path
                .with_extension("part")
                .exists()
        );
        server
            .request()
            .await;
    }

    #[tokio::test]
    async fn download_accepts_chunked_response_without_content_length() {
        let mut server = TestServer::raw_response(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nmodel\r\n6\r\n bytes\r\n0\r\n\r\n".into()
        ).await;
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir
            .path()
            .join("model.part");
        ImageRecognizer::download_to(&path, Url::parse(&server.url).expect("fixture URL"))
            .await
            .expect("chunked download");
        assert_eq!(
            tokio::fs::read(path)
                .await
                .expect("read download"),
            b"model bytes"
        );
        server
            .request()
            .await;
    }

    #[tokio::test]
    async fn truncated_model_download_is_rejected_and_partial_file_removed() {
        let mut server = TestServer::raw_response(
            "HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nshort".into(),
        )
        .await;
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir
            .path()
            .join("model.onnx");
        assert!(
            ImageRecognizer::load_or_download(&path, Url::parse(&server.url).expect("fixture URL"))
                .await
                .is_err()
        );
        assert!(!path.exists());
        assert!(
            !path
                .with_extension("part")
                .exists()
        );
        server
            .request()
            .await;
    }

    #[test]
    fn frame_path_converts_offset_and_removes_owned_file_on_drop() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir
            .path()
            .join("frame.png");
        std::fs::write(&path, b"fixture").expect("write frame");
        let frame = FramePath::new(
            DefaultDuration::from_millis(400),
            path.to_str()
                .expect("UTF-8 path"),
        );
        assert_eq!(
            frame.time_offset_from_first_frame,
            Duration::milliseconds(400)
        );
        assert_eq!(frame.as_ref(), path);
        assert!(frame.should_drop);
        drop(frame);
        assert!(!path.exists());
    }

    #[test]
    fn frame_path_can_preserve_file_and_tolerates_missing_file() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir
            .path()
            .join("frame.png");
        std::fs::write(&path, b"fixture").expect("write frame");
        let mut frame = FramePath::new_impl(Duration::zero(), &path);
        frame.should_drop = false;
        drop(frame);
        assert!(path.exists());
        drop(FramePath::new_impl(
            Duration::zero(),
            dir.path()
                .join("missing.png"),
        ));
    }

    #[test]
    fn image_intersection_heuristic_has_strict_traffic_light_threshold() {
        let colors: DominantColors = std::iter::empty().collect();
        for count in [0, 8, 9] {
            let detections: ImageDetections = serde_json::from_value(serde_json::json!({
                "detections": (0..count).map(|_| serde_json::json!({
                    "name": "TrafficLight", "xyxy": {"x1": 0, "x2": 1, "y1": 0, "y2": 1},
                    "distance": 1, "confidence": 0.9
                })).collect::<Vec<_>>()
            }))
            .expect("fixture detections");
            let tags = evaluate_image_into_location_tags(&colors, &detections);
            assert_eq!(tags.contains(LocationTag::Intersection), count > 8);
            assert_eq!(tags.contains(LocationTag::Outside), count > 8);
            assert!(!tags.contains(LocationTag::Forest));
        }
    }
}
