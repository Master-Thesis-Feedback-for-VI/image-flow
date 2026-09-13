use std::fmt::Display;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::time::Duration as DefaultDuration;

use chrono::Duration;
use tokio::sync::{Mutex, MutexGuard, OnceCell};
use tokio::task::{JoinHandle, JoinSet};
use tracing::instrument;
use ultralytics_inference::{Results as DetectionResults, YOLOModel};

use crate::context::{ContextCategory, UniqueGeographicCategories};
use crate::image_processor::{DominantColors, dominant_colors};

const DOMINANT_COLOR_AMOUNT: usize = 4;
const TRAFFIC_LIGHT_THRESHOLD: usize = 8;
const OUTSIDE_THRESHOLD: &[(DetectionType, usize)] = &[
    (DetectionType::Person, 7),
    (DetectionType::TrafficLight, 1),
    (DetectionType::Car, 1),
    (DetectionType::Bench, 2),
    (DetectionType::Bicycle, 1),
    (DetectionType::DiningTable, 2),
];

static MODEL: OnceCell<Mutex<YOLOModel>> = OnceCell::const_new();

/// TODO
///
/// # Errors
/// TODO
pub async fn evaluate_image_into_categories(
    frame_path: impl AsRef<Path>,
) -> crate::Result<UniqueGeographicCategories> {
    let frame_path_image = frame_path
        .as_ref()
        .to_path_buf();
    let frame_path_color = frame_path_image.clone();

    let detections_fut = async {
        tracing::info!("Detecting objects in image...");
        let image_detections = detect_objects_in_image(&frame_path_image).await;
        tracing::info!("Finished detecting objects in image...");
        image_detections
    };

    let colors_fut = async {
        tracing::info!("Detecting colors in image...");
        let dominant_colors = dominant_colors(&frame_path_color, DOMINANT_COLOR_AMOUNT).await;
        tracing::info!("Finished detecting colors in image...");
        dominant_colors
    };

    let (image_detections, dominant_colors) = tokio::try_join!(detections_fut, colors_fut)?;

    let categories = evaluate_image_into_categories_impl(&dominant_colors, &image_detections);
    Ok(categories)
}

/// TODO
/// # Errors
/// TODO
///
/// # Panics
/// TODO
pub async fn detect_objects_in_image(
    frame_path: impl AsRef<Path>,
) -> crate::Result<ImageDetections> {
    let frame_path = frame_path
        .as_ref()
        .to_path_buf();
    let mut results =
        tokio::task::spawn_blocking(move || -> crate::Result<Vec<DetectionResults>> {
            let handle = tokio::runtime::Handle::current();
            let mut model = handle.block_on(lock_model())?;
            model
                .predict(frame_path)
                .map_err(crate::error::image)
        })
        .await??;
    assert_eq!(results.len(), 1);

    let result = results.remove(0);
    let image_detections = ImageDetections::from_detection_result(result);

    Ok(image_detections)
}

/// TODO
#[instrument]
#[must_use]
pub fn evaluate_image_into_categories_impl(
    dominant_colors: &DominantColors,
    detections: &ImageDetections,
) -> UniqueGeographicCategories {
    let mut categories = UniqueGeographicCategories::new();
    categories.detections = Some(detections.clone());

    let is_outside = detections.is_outside(OUTSIDE_THRESHOLD);
    if is_outside {
        categories.insert(ContextCategory::Outside);
    }

    let intersection_count = detections
        .get_by_detected_object(DetectionType::TrafficLight)
        .count();
    if intersection_count > TRAFFIC_LIGHT_THRESHOLD {
        categories.insert(ContextCategory::Intersection);
    }

    let in_forest = dominant_colors
        .iter()
        .any(|color| color.is_close_to_forest_green());
    if in_forest {
        categories.insert(ContextCategory::Outside);
        categories.insert(ContextCategory::Forest);
    }

    if !categories.contains(ContextCategory::Outside) {
        categories.insert(ContextCategory::Inside);
    }

    categories
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

async fn model() -> crate::Result<&'static Mutex<YOLOModel>> {
    MODEL
        .get_or_try_init(async || {
            let model = tokio::task::spawn_blocking(|| YOLOModel::load("yolo26m.onnx")).await??;
            Ok(Mutex::new(model))
        })
        .await
}

async fn lock_model() -> crate::Result<MutexGuard<'static, YOLOModel>> {
    let model = model()
        .await?
        .lock()
        .await;

    Ok(model)
}

#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone)]
pub struct Detection {
    confidence: f32,
    name: DetectionType,
}

#[derive(Debug, Clone, Default)]
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
    fn from_detection_result(res: DetectionResults) -> Self {
        let mut detections = Vec::new();

        if let Some(boxes) = res.boxes {
            for i in 0..boxes.len() {
                let cls = boxes.cls()[i] as usize;
                let conf = boxes.conf()[i];
                let name = res
                    .names
                    .get(&cls)
                    .map_or("unknown", |s| s.as_str());

                let detection = Detection {
                    name: DetectionType::from_str(name),
                    confidence: conf,
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
        Self::from_detection_result(value)
    }
}

impl Deref for ImageDetections {
    type Target = Vec<Detection>;

    fn deref(&self) -> &Self::Target {
        &self.detections
    }
}

impl DerefMut for ImageDetections {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.detections
    }
}

#[allow(clippy::significant_drop_tightening)]
#[cfg(test)]
mod tests {
    use crate::{
        context::image_based_context::{
            DOMINANT_COLOR_AMOUNT, detect_objects_in_image, evaluate_image_into_categories_impl,
        },
        image_processor::dominant_colors,
        tests::{TESTING_FILE_MANAGER, TestingImage},
    };

    #[tokio::test]
    async fn check_detections() {
        let image_files = {
            let mut binding = TESTING_FILE_MANAGER
                .lock()
                .await;
            binding.all_images()
        };

        for TestingImage {
            img_path,
            categories,
        } in &image_files
        {
            let dominate_colors = dominant_colors(&img_path, DOMINANT_COLOR_AMOUNT)
                .await
                .unwrap();
            let detected_obj = detect_objects_in_image(img_path)
                .await
                .unwrap();

            let evaluation = evaluate_image_into_categories_impl(&dominate_colors, &detected_obj);

            for category in categories {
                if !evaluation.contains(*category) {
                    println!(
                        "This image: {}\nIt contained these colors:",
                        img_path.display()
                    );
                    for color in dominate_colors.iter() {
                        println!("h, s, v = {color}");
                    }
                    println!("Additionally these objects where detected: {detected_obj:?}");

                    panic!();
                }
            }
        }
    }
}
