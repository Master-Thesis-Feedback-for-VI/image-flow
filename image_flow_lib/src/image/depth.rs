//! Per-pixel depth values returned by the inference backend.
//!
//! Coordinates are `(x, y)` in image pixels; the backing array is indexed as
//! `(row, column)`, or `(y, x)`. Values are retained without calibration or scaling.

use ndarray::Array2;
use serde::{Deserialize, Serialize};
use ultralytics_inference::{DepthMap as UltralyticsDepthMap, Results as DetectionResults};

/// Original image dimensions, serialized as `height` and `width`.
///
/// Fields are private in Rust and exposed as record fields in `UniFFI` bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record, Serialize, Deserialize)]
pub struct ImageDim {
    height: u32,
    width: u32,
}

/// Owned depth array and original image dimensions.
///
/// The supported YOLO depth models report meters. This type passes values
/// through from the model without further calibration.
#[derive(Debug, Clone, uniffi::Object, Serialize, Deserialize)]
pub struct DepthMap {
    /// Original image dimensions reported by the backend.
    pub shape: ImageDim,
    pixel_data: Array2<f32>,
}

#[uniffi::export]
impl DepthMap {
    /// Return depth at pixel `(x, y)`, or `None` outside the stored array.
    #[must_use]
    pub fn depth_at(&self, x: u32, y: u32) -> Option<f32> {
        self.pixel_data
            .get((y as usize, x as usize))
            .copied()
    }

    /// Return the original image dimensions reported by the backend.
    #[must_use]
    pub const fn shape(&self) -> ImageDim {
        self.shape
    }
}

impl DepthMap {
    fn from_detection_results(res: DetectionResults) -> crate::Result<Self> {
        res.depth
            .map_or_else(
                || Err(crate::error::image("could not create depth map")),
                |depth| Ok(Self::from(depth)),
            )
    }
}

impl From<UltralyticsDepthMap> for DepthMap {
    fn from(value: UltralyticsDepthMap) -> Self {
        let shape = ImageDim {
            height: value
                .orig_shape
                .0,
            width: value
                .orig_shape
                .1,
        };

        let pixel_data = value
            .data
            .into_owned();

        Self { shape, pixel_data }
    }
}

impl TryFrom<DetectionResults> for DepthMap {
    type Error = crate::Error;

    fn try_from(value: DetectionResults) -> Result<Self, Self::Error> {
        Self::from_detection_results(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    #[test]
    fn rectangular_map_preserves_shape_and_xy_access() {
        let map = DepthMap::from(UltralyticsDepthMap::new(
            array![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]],
            (2, 3),
        ));
        assert_eq!(
            map.shape(),
            ImageDim {
                height: 2,
                width: 3
            }
        );
        assert_eq!(map.depth_at(0, 0), Some(1.0));
        assert_eq!(map.depth_at(2, 1), Some(6.0));
        assert_eq!(map.depth_at(3, 1), None);
        assert_eq!(map.depth_at(1, 2), None);
        assert_eq!(map.depth_at(u32::MAX, u32::MAX), None);
    }

    #[test]
    fn depth_map_serialization_preserves_pixels() {
        let map = DepthMap::from(UltralyticsDepthMap::new(array![[0.0, 2.5]], (1, 2)));
        let restored: DepthMap =
            serde_json::from_value(serde_json::to_value(&map).expect("serialize map"))
                .expect("deserialize map");
        assert_eq!(restored.shape(), map.shape());
        assert_eq!(restored.depth_at(1, 0), Some(2.5));
    }

    #[test]
    fn detection_results_require_depth_and_convert_it_when_present() {
        let missing = crate::tests::empty_detection_result();
        let error = DepthMap::try_from(missing).expect_err("missing depth");
        assert!(matches!(
            crate::error::ImageFlowError::from(error),
            crate::error::ImageFlowError::Image { .. }
        ));
        let mut present = crate::tests::empty_detection_result();
        present.depth = Some(UltralyticsDepthMap::new(array![[1.5]], (1, 1)));
        assert_eq!(
            DepthMap::try_from(present)
                .expect("present depth")
                .depth_at(0, 0),
            Some(1.5)
        );
    }
}
