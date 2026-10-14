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

    let in_forest = dominant_colors
        .iter()
        .any(|color| color.is_close_to_forest_green());
    if in_forest {
        categories.extend([LocationTag::Outside, LocationTag::Forest]);
    }
    categories
}

#[derive(Debug)]
pub struct ObjectDetectionModel {
    path: PathBuf,
    model: Arc<Mutex<YOLOModel>>,
}

#[derive(Debug)]
pub struct DepthDetectionModel {
    path: PathBuf,
    model: Arc<Mutex<YOLOModel>>,
}

#[derive(Debug, uniffi::Object)]
pub struct ImageRecognizer {
    pub variant: ModelVariant,
    obj_detection: Arc<ObjectDetectionModel>,
    depth_recognition: Arc<DepthDetectionModel>,
}

#[uniffi::export(async_runtime = "tokio")]
impl ImageRecognizer {
    /// # Errors
    /// TODO
    #[uniffi::constructor]
    pub async fn new(
        model_variant: ModelVariant,
        downlaod_path: String,
    ) -> DefaultResult<Self, ImageFlowError> {
        let img_recognizer = Self::new_impl(model_variant, downlaod_path).await?;
        Ok(img_recognizer)
    }

    /// # Panics
    /// TODO
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

    /// # Panics
    /// TODO
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
    /// # Errors
    /// TODO
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

    /// # Errors
    /// TODO
    pub async fn default() -> crate::Result<Self> {
        let model_variant = ModelVariant::Medium;
        let download_path = Path::new(".");

        Self::new_impl(model_variant, download_path).await
    }

    /// # Errors
    /// TODO
    ///
    /// # Panics
    /// The API from Ulatralytics is weird here. Per image 1 Results struct is created containing
    /// all Results and detections from one image. So having multiple makes no sense and there
    /// always only exists one Results even if nothing is detected!
    pub async fn predict_objects(&self, path: impl AsRef<Path>) -> crate::Result<ImageDetections> {
        let path_to_pred = path
            .as_ref()
            .to_path_buf();
        let obj_detection_model = Arc::clone(&self.obj_detection);

        let object_prediction_fut =
            tokio::task::spawn_blocking(move || -> crate::Result<Vec<DetectionResults>> {
                let predictions = obj_detection_model
                    .model
                    .blocking_lock()
                    .predict(path_to_pred)?;

                Ok(predictions)
            });

        let depth_pred_fut = self.predict_depth(&path);

        let (object_prediction, depth_pred) = tokio::join!(object_prediction_fut, depth_pred_fut);
        let mut object_prediction = object_prediction??;
        assert_eq!(object_prediction.len(), 1);
        let result = object_prediction.remove(0);
        let depth_pred = depth_pred?;

        let image_detections = ImageDetections::from_detection_result(&result, &depth_pred);
        Ok(image_detections)
    }

    /// # Errors
    /// TODO
    ///
    /// # Panics
    /// TODO
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

#[derive(Debug, Clone, uniffi::Enum)]
pub enum ModelVariant {
    Nano,
    Small,
    Medium,
    Large,
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
            Self::XLarge => "yolo26xl-depth.onnx",
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

#[derive(Debug, uniffi::Object, Clone)]
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
                    format!("https://github.com/ultralytics/assets/releases/download/v8.4.0/{name}")
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
