use std::collections::HashSet;
use std::fmt::Display;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::time::Duration as DefaultDuration;

use chrono::Duration;
use image::DynamicImage;
use tokio::sync::{Mutex, MutexGuard, OnceCell};
use tokio::task::JoinHandle;
use ultralytics_inference::{Results as DetectionResults, YOLOModel};

use crate::context::ContextCategory;
use crate::image_processor::dominant_colors;

static MODEL: OnceCell<Mutex<YOLOModel>> = OnceCell::const_new();

/// TODO
///
/// # Errors
/// TODO
///
/// # Panics
/// TODO
pub async fn evaluate_image_into_categories(
    frame_path: impl AsRef<Path>,
) -> crate::Result<HashSet<ContextCategory>> {
    let path = frame_path
        .as_ref()
        .to_path_buf();
    let handle: JoinHandle<crate::Result<(DetectionResults, DynamicImage)>> =
        tokio::spawn(async move {
            let mut results: Vec<DetectionResults> = lock_model()
                .await?
                .predict(&path)?;
            assert_eq!(results.len(), 1);

            let result = results.remove(0);
            let image = image::open(&path)?;
            Ok((result, image))
        });

    let mut categories = HashSet::new();

    let (res, image) = handle
        .await
        .map_err(crate::error::io)??;
    let image_detections = ImageDetections::from_detection_result(res);

    let is_outside = image_detections.is_outside(&[
        (DetectionType::TrafficLight, 1),
        (DetectionType::Car, 1),
        (DetectionType::Bench, 2),
        (DetectionType::Bicycle, 1),
        (DetectionType::DiningTable, 2),
    ]);
    let outside = if is_outside {
        ContextCategory::Outside
    } else {
        ContextCategory::Building
    };
    categories.insert(outside);

    let in_forest = dominant_colors(&image, 7)
        .iter()
        .any(|color| color.is_close_to_forest_green());
    if in_forest {
        categories.remove(&ContextCategory::Building);
        categories.insert(ContextCategory::Outside);
        categories.insert(ContextCategory::Forest);
    } else {
        categories.insert(ContextCategory::Building);
    }

    Ok(categories)
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

async fn model() -> crate::Result<&'static Mutex<YOLOModel>> {
    MODEL
        .get_or_try_init(|| async {
            let model = tokio::task::spawn_blocking(|| YOLOModel::load("yolo26m.onnx"))
                .await
                .map_err(crate::error::io)? // JoinError -> crate::Error, unwrap outer layer
                .map_err(crate::error::io)?; // InferenceError -> crate::Error, unwrap inner layer
            Ok(Mutex::new(model)) // now `model` is a plain YOLOModel
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
struct Detection {
    confidence: f32,
    name: DetectionType,
}

#[derive(Debug, Clone)]
struct ImageDetections {
    detections: Vec<Detection>,
}

impl ImageDetections {
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

#[allow(clippy::significant_drop_tightening)]
#[cfg(test)]
mod tests {
    use crate::{
        context::{ContextCategory, image_based_context::evaluate_image_into_categories},
        image_processor::dominant_colors,
        tests::TESTING_FILE_MANAGER,
    };

    #[tokio::test]
    async fn check_detections() {
        let mut binding = TESTING_FILE_MANAGER
            .lock()
            .await;
        let image_files = binding.all_images();
        let mut missed_collector = Vec::new();

        for image_path in image_files {
            let evaluation = evaluate_image_into_categories(&image_path)
                .await
                .unwrap();

            let img = image::open(image_path).unwrap();
            let dominate_colors = dominant_colors(&img, 4);

            if !evaluation.contains(&ContextCategory::Outside) {
                missed_collector.push((image_path.clone(), dominate_colors));
            }
        }

        for (path, dom_colors) in &missed_collector {
            println!("This image: {}\nIt contained these colors:", path.display());
            for color in dom_colors {
                println!("h, s, v = {color}");
            }
        }

        assert_eq!(missed_collector.len(), 0);
    }
}
