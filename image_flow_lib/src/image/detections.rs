use std::{fmt::Display, ops::Deref};

use serde::{Deserialize, Serialize};
use tracing::instrument;
use ultralytics_inference::Results as DetectionResults;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Enum, Eq, PartialOrd, Ord)]
pub enum DetectionType {
    Bench,
    Bicycle,
    Car,
    Chair,
    Cup,
    DiningTable,
    Handbag,
    Laptop,
    Person,
    TrafficLight,
    Tv,
    Keyboard,

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
            "cup" => Self::Cup,
            "handbag" => Self::Handbag,
            "laptop" => Self::Laptop,
            "tv" => Self::Tv,
            "keyboard" => Self::Keyboard,
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
    pub(crate) name: DetectionType,
    xyxy: BoundingBox,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, uniffi::Record)]
pub struct ImageDetections {
    pub(crate) detections: Vec<Detection>,
}

impl ImageDetections {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            detections: Vec::new(),
        }
    }

    #[instrument]
    pub fn from_detection_result(res: &DetectionResults) -> Self {
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

    pub fn get_by_detected_object(
        &self,
        detection_type: DetectionType,
    ) -> impl Iterator<Item = &Detection> {
        self.iter()
            .filter(move |pred| pred.name == detection_type)
    }

    #[must_use]
    pub fn is_outside(&self, minimums: &[(DetectionType, usize)]) -> bool {
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
