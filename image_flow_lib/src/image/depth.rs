use ndarray::Array2;
use serde::{Deserialize, Serialize};
use ultralytics_inference::{DepthMap as UltralyticsDepthMap, Results as DetectionResults};

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record, Serialize, Deserialize)]
pub struct ImageDim {
    height: u32,
    width: u32,
}

#[derive(Debug, Clone, uniffi::Object, Serialize, Deserialize)]
pub struct DepthMap {
    pub shape: ImageDim,
    pixel_data: Array2<f32>,
}

#[uniffi::export]
impl DepthMap {
    #[must_use]
    pub fn depth_at(&self, x: u32, y: u32) -> Option<f32> {
        self.pixel_data
            .get((y as usize, x as usize))
            .copied()
    }

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
