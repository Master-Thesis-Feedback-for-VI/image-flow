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
        let path = path
            .as_ref()
            .to_path_buf();
        let obj_detection_model = Arc::clone(&self.obj_detection);

        let mut predictions =
            tokio::task::spawn_blocking(move || -> crate::Result<Vec<DetectionResults>> {
                let predictions = obj_detection_model
                    .model
                    .blocking_lock()
                    .predict(path)?;

                Ok(predictions)
            })
            .await??;
        assert_eq!(predictions.len(), 1);

        let result = predictions.remove(0);
        let image_detections = ImageDetections::from(result);

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
        let mut url_obj = Self::base_url();
        let mut url_depth = Self::base_url();

        let obj_name = self.object_detection_model_name();
        let depth_name = self.depth_detection_model_name();

        url_obj
            .path_segments_mut()
            .expect("This cannot fail since the url is valid")
            .push(obj_name);

        url_depth
            .path_segments_mut()
            .expect("This cannot fail since the url is valid")
            .push(depth_name);

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
