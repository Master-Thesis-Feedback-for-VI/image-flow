use std::path::{Path, PathBuf};
use std::time::Duration as DefaultDuration;

use chrono::Duration;
use tokio::sync::{Mutex, MutexGuard, OnceCell};
use tokio::task::JoinHandle;
use ultralytics_inference::{Results as DetectionResults, YOLOModel};

static MODEL: OnceCell<Mutex<YOLOModel>> = OnceCell::const_new();

/// TODO
///
/// # Errors
/// TODO
pub async fn evaluate_image_into_categories(
    frame_path: impl AsRef<Path>,
) -> crate::Result<Vec<DetectionResults>> {
    let path = frame_path
        .as_ref()
        .to_path_buf();
    let handle: JoinHandle<crate::Result<Vec<DetectionResults>>> = tokio::spawn(async move {
        let results: Vec<DetectionResults> = lock_model()
            .await?
            .predict(path)?;

        Ok(results)
    });

    handle
        .await
        .map_err(crate::error::io)?
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

struct Detection {
    confidence: f64,
    name: DetectionType,
}

impl Detection {
    // TODO
}

#[allow(clippy::significant_drop_tightening)]
#[cfg(test)]
mod tests {
    use crate::{
        context::image_based_context::evaluate_image_into_categories, tests::TESTING_FILE_MANAGER,
    };

    #[tokio::test]
    async fn check_detections() {
        let mut binding = TESTING_FILE_MANAGER
            .lock()
            .await;
        let image_files = binding.all_images();

        for file_path in image_files {
            let evaluations = evaluate_image_into_categories(file_path)
                .await
                .unwrap();

            for res in &evaluations {
                if let Some(boxes) = &res.boxes {
                    println!("Found {} detections", boxes.len());
                    for i in 0..boxes.len() {
                        let cls = boxes.cls()[i] as usize;
                        let conf = boxes.conf()[i];
                        let name = res
                            .names
                            .get(&cls)
                            .map_or("unknown", |s| s.as_str());

                        println!("  {name} {conf:.2}");
                    }
                }
            }
        }

        panic!()
    }
}
