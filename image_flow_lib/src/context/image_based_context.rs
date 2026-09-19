use std::fmt::Display;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::result::Result as DefaultResult;
use std::sync::{Arc, LazyLock};
use std::time::Duration as DefaultDuration;

use chrono::Duration;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncWriteExt as _, BufWriter};
use tokio::sync::Mutex;
use tokio_stream::StreamExt;
use tracing::instrument;
use ultralytics_inference::{Results as DetectionResults, YOLOModel};

use crate::context::{LocationTag, UniqueLocationTags};
use crate::error::ImageFlowError;
use crate::image_processor::DominantColors;

const TRAFFIC_LIGHT_THRESHOLD: usize = 8;
const OUTSIDE_THRESHOLD: &[(DetectionType, usize)] = &[
    (DetectionType::Person, 7),
    (DetectionType::TrafficLight, 1),
    (DetectionType::Car, 1),
    (DetectionType::Bench, 2),
    (DetectionType::Bicycle, 1),
    (DetectionType::DiningTable, 2),
];

/// TODO
#[instrument]
#[must_use]
pub(crate) fn evaluate_image_into_location_tags(
    dominant_colors: &DominantColors,
    detections: &ImageDetections,
) -> UniqueLocationTags {
    let mut categories = UniqueLocationTags::new();

    let is_outside = detections.is_outside(OUTSIDE_THRESHOLD);
    if is_outside {
        categories.insert(LocationTag::Outside);
    }

    let intersection_count = detections
        .get_by_detected_object(DetectionType::TrafficLight)
        .count();
    if intersection_count > TRAFFIC_LIGHT_THRESHOLD {
        categories.insert(LocationTag::Intersection);
    }

    let in_forest = dominant_colors
        .iter()
        .any(|color| color.is_close_to_forest_green());
    if in_forest {
        categories.insert(LocationTag::Outside);
        categories.insert(LocationTag::Forest);
    }
    categories
}

#[derive(Debug, uniffi::Object)]
pub struct ImageRecognizer {
    pub model_variant: ModelVariant,
    model_path: PathBuf,

    model: Arc<Mutex<YOLOModel>>,
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
    pub fn model_path(&self) -> String {
        let tmp = self
            .model_path
            .clone();
        tmp.into_os_string()
            .into_string()
            .expect("Invalid UTF-8")
    }
}

impl ImageRecognizer {
    /// # Errors
    /// TODO
    pub async fn new_impl(
        model_variant: ModelVariant,
        download_path: impl AsRef<Path>,
    ) -> crate::Result<Self> {
        let path = download_path
            .as_ref()
            .to_path_buf();

        if !path.is_dir() {
            let msg = format!("The dir: {}, does not exist", path.display());
            return Err(crate::error::io(msg));
        }

        let file_path = path.join(model_variant.model_name());

        if file_path.is_file() {
            let load_path = file_path.clone();
            let model = tokio::task::spawn_blocking(move || -> crate::Result<YOLOModel> {
                Ok(YOLOModel::load(load_path)?)
            })
            .await??;

            return Ok(Self {
                model_variant,
                model_path: file_path,
                model: Arc::new(Mutex::new(model)),
            });
        }

        let tmp_path = file_path.with_extension("part");

        if let Err(e) = Self::download_to(&model_variant, &tmp_path).await {
            // Best-effort cleanup of any partial temp file.
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(e);
        }

        tokio::fs::rename(&tmp_path, &file_path)
            .await
            .map_err(|e| crate::error::io(format!("Failed to finalize downloaded file: {e}")))?;

        let load_path = file_path.clone();
        let model = tokio::task::spawn_blocking(move || -> crate::Result<YOLOModel> {
            Ok(YOLOModel::load(load_path)?)
        })
        .await??;

        Ok(Self {
            model_variant,
            model_path: file_path,
            model: Arc::new(Mutex::new(model)),
        })
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
    pub async fn predict(&self, path: impl AsRef<Path>) -> crate::Result<ImageDetections> {
        let path = path
            .as_ref()
            .to_path_buf();
        let model = Arc::clone(&self.model);

        let mut predictions =
            tokio::task::spawn_blocking(move || -> crate::Result<Vec<DetectionResults>> {
                let predictions = model
                    .blocking_lock()
                    .predict(path)?;

                Ok(predictions)
            })
            .await??;
        assert_eq!(predictions.len(), 1);

        let result = predictions.remove(0);
        let image_detections = ImageDetections::from_detection_result(&result);

        Ok(image_detections)
    }

    /// Streams the model to `tmp_path`. Verifies size when the server reports
    /// `Content-Length`, but doesn't hard-fail if that header is absent (some
    /// CDNs/chunked responses omit it).
    async fn download_to(model_variant: &ModelVariant, tmp_path: &Path) -> crate::Result<()> {
        let download_url = model_variant.build_url();

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

static BASE_URL: LazyLock<Url> = LazyLock::new(|| {
    let base_url = "https://github.com/ultralytics/assets/releases/download/v8.4.0/";

    Url::parse(base_url).expect("We know this cant fail since the above url is valid")
});

impl ModelVariant {
    fn build_url(&self) -> Url {
        let mut url = BASE_URL.clone();

        let model_name = self.model_name();
        url.path_segments_mut()
            .expect("This cannot fail since the url is valid")
            .push(model_name);
        url
    }

    const fn model_name(&self) -> &str {
        match self {
            Self::Nano => "yolo26n.onnx",
            Self::Small => "yolo26s.onnx",
            Self::Medium => "yolo26m.onnx",
            Self::Large => "yolo26l.onnx",
            Self::XLarge => "yolo26x.onnx",
        }
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Enum)]
enum DetectionType {
    Person,
    TrafficLight,
    Car,
    Bicycle,
    Bench,
    Chair,
    DiningTable,
    Unknown(String),
}

impl DetectionType {
    fn from_str(name: &str) -> Self {
        let name = name.to_lowercase();
        match name.as_str() {
            "person" => Self::Person,
            "traffic light" => Self::TrafficLight,
            "car" => Self::Car,
            "bicycle" => Self::Bicycle,
            "bench" => Self::Bench,
            "chair" => Self::Chair,
            "dining table" => Self::DiningTable,
            _ => Self::Unknown(name),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, uniffi::Record)]
pub struct BoundingBox {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, uniffi::Record)]
pub struct Detection {
    confidence: f32,
    name: DetectionType,
    xyxy: BoundingBox,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, uniffi::Record)]
pub struct ImageDetections {
    detections: Vec<Detection>,
}

impl ImageDetections {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            detections: Vec::new(),
        }
    }

    #[instrument]
    fn from_detection_result(res: &DetectionResults) -> Self {
        let mut detections = Vec::new();

        if let Some(boxes) = &res.boxes {
            for i in 0..boxes.len() {
                let xyxy_tmp = boxes.xyxy();
                let xyxy_row = xyxy_tmp.row(i);
                let xyxy = BoundingBox {
                    x1: xyxy_row[0],
                    x2: xyxy_row[2],
                    y1: xyxy_row[1],
                    y2: xyxy_row[3],
                };

                let cls = boxes.cls()[i] as usize;
                let conf = boxes.conf()[i];
                let name = res
                    .names
                    .get(&cls)
                    .map_or("unknown", |s| s.as_str());

                let detection = Detection {
                    name: DetectionType::from_str(name),
                    confidence: conf,
                    xyxy,
                };
                detections.push(detection);
            }
        }

        Self { detections }
    }

    fn get_by_detected_object(
        &self,
        detection_type: DetectionType,
    ) -> impl Iterator<Item = &Detection> {
        self.iter()
            .filter(move |pred| pred.name == detection_type)
    }

    fn is_outside(&self, minimums: &[(DetectionType, usize)]) -> bool {
        minimums
            .iter()
            .any(|(kind, minimum)| {
                self.iter()
                    .filter(|pred| pred.name == *kind)
                    .count()
                    >= *minimum
            })
    }

    #[must_use]
    pub fn into_inner(self) -> Vec<Detection> {
        self.detections
    }
}

impl Display for ImageDetections {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let amount_detections = self
            .detections
            .len();
        writeln!(f, "Amount of objects detected: {amount_detections}")?;
        for (idx, detection) in self
            .detections
            .iter()
            .enumerate()
        {
            write!(f, "\t{detection:?}")?;

            if idx < amount_detections - 1 {
                writeln!(f)?;
            }
        }

        Ok(())
    }
}

impl From<DetectionResults> for ImageDetections {
    fn from(value: DetectionResults) -> Self {
        Self::from_detection_result(&value)
    }
}

impl Deref for ImageDetections {
    type Target = Vec<Detection>;

    fn deref(&self) -> &Self::Target {
        &self.detections
    }
}

impl IntoIterator for ImageDetections {
    type Item = Detection;
    type IntoIter = std::vec::IntoIter<Detection>;
    fn into_iter(self) -> Self::IntoIter {
        self.detections
            .into_iter()
    }
}

impl Extend<Detection> for ImageDetections {
    fn extend<T: IntoIterator<Item = Detection>>(&mut self, iter: T) {
        self.detections
            .extend(iter);
    }
}
