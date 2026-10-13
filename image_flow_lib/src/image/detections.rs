use std::{fmt::Display, ops::Deref};

use serde::{Deserialize, Serialize};
use tracing::instrument;
use ultralytics_inference::Results as DetectionResults;

use crate::image::depth::DepthMap;

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
    Cyclist,

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

impl BoundingBox {
    fn depth_at_center(self, depth_map: &DepthMap) -> f32 {
        let x = f32::midpoint(self.x1, self.x2).floor() as u32;
        let y = f32::midpoint(self.y1, self.y2).floor() as u32;

        depth_map
            .depth_at(x, y)
            .expect("This in theory should not be possible")
    }

    const fn join(self, other: Self) -> Self {
        Self {
            x1: self
                .x1
                .min(other.x1),
            x2: self
                .x2
                .max(other.x2),
            y1: self
                .y1
                .min(other.y1),
            y2: self
                .y2
                .max(other.y2),
        }
    }

    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.x1 <= other.x2 && self.x2 >= other.x1 && self.y1 <= other.y2 && self.y2 >= other.y1
    }

    #[must_use]
    pub const fn width(self) -> f32 {
        (self.x2 - self.x1).max(0.0)
    }

    #[must_use]
    pub const fn height(self) -> f32 {
        (self.y2 - self.y1).max(0.0)
    }

    #[must_use]
    pub const fn area(self) -> f32 {
        self.width() * self.height()
    }

    #[must_use]
    pub const fn intersection_area(self, other: Self) -> f32 {
        let width = (self
            .x2
            .min(other.x2)
            - self
                .x1
                .max(other.x1))
        .max(0.0);
        let height = (self
            .y2
            .min(other.y2)
            - self
                .y1
                .max(other.y1))
        .max(0.0);

        width * height
    }

    #[must_use]
    pub const fn union_area(self, other: Self) -> f32 {
        self.area() + other.area() - self.intersection_area(other)
    }

    #[must_use]
    pub const fn iou(self, other: Self) -> f32 {
        let intersection = self.intersection_area(other);
        let union = self.union_area(other);

        if union <= 0.0 {
            return 0.0;
        }

        intersection / union
    }

    #[must_use]
    pub const fn is_similar_bb(self, other: Self, threshold: f32) -> bool {
        self.iou(other) >= threshold
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, uniffi::Record)]
pub struct Detection {
    pub(crate) name: DetectionType,
    xyxy: BoundingBox,
    distance: f32,
    confidence: f32,
}

impl Detection {
    const fn join_detections(det1: &Self, det2: &Self, new_detection_type: DetectionType) -> Self {
        let joined_bb = det1
            .xyxy
            .join(det2.xyxy);

        let conf_average = f32::midpoint(det1.confidence, det2.confidence);
        let distance_average = f32::midpoint(det1.distance, det2.distance);
        Self {
            confidence: conf_average,
            name: new_detection_type,
            xyxy: joined_bb,
            distance: distance_average,
        }
    }
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
    pub fn from_detection_result(res: &DetectionResults, depth_map: &DepthMap) -> Self {
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
                    distance: xyxy.depth_at_center(depth_map),
                };
                detections.push(detection);
            }
        }

        detections = Self::cyclist_merge(detections);
        detections = Self::remove_similar_bb(detections);

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

    fn cyclist_merge<I>(detections: I) -> Vec<Detection>
    where
        I: IntoIterator<Item = Detection>,
    {
        let mut collector = Vec::new();
        let mut bicycles = Vec::new();
        let mut persons = Vec::new();

        for detection in detections {
            match detection.name {
                DetectionType::Bicycle => bicycles.push(detection),
                DetectionType::Person => persons.push(detection),
                _ => collector.push(detection),
            }
        }

        let mut bicycle_riders = Vec::new();
        bicycles.retain(|bicycle| {
            let person_idx = persons
                .iter()
                .position(|person| {
                    bicycle
                        .xyxy
                        .intersects(person.xyxy)
                });

            person_idx.is_none_or(|idx| {
                let person = persons.remove(idx);

                bicycle_riders.push(Detection::join_detections(
                    bicycle,
                    &person,
                    DetectionType::Cyclist,
                ));

                false
            })
        });

        collector.extend(bicycles);
        collector.extend(persons);
        collector.extend(bicycle_riders);

        collector
    }

    fn remove_similar_bb<I>(detections: I) -> Vec<Detection>
    where
        I: IntoIterator<Item = Detection>,
    {
        const IOU_THRESHOLD: f32 = 0.9;

        let mut result: Vec<Detection> = Vec::new();
        for detection in detections {
            let is_duplicate = result
                .iter()
                .any(|existing| {
                    existing
                        .xyxy
                        .is_similar_bb(detection.xyxy, IOU_THRESHOLD)
                        && existing.name == detection.name
                });

            if !is_duplicate {
                result.push(detection);
            }
        }

        result
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

#[cfg(test)]
mod tests {
    use ndarray::Array2;
    use ultralytics_inference::DepthMap as UltralyticsDepthMap;

    use super::BoundingBox;
    use crate::image::depth::DepthMap;

    #[test]
    fn depth_at_center_uses_x_as_column_and_y_as_row() {
        let mut pixels = Array2::zeros((5, 3));
        pixels[(4, 1)] = 2.5;
        let depth_map = DepthMap::from(UltralyticsDepthMap::new(pixels, (5, 3)));
        let box_at_bottom = BoundingBox {
            x1: 0.0,
            x2: 2.0,
            y1: 4.0,
            y2: 5.0,
        };

        assert_eq!(box_at_bottom.depth_at_center(&depth_map), 2.5);
    }
}
