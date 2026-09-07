use std::fmt::Display;

use image::DynamicImage;
use kmeans_colors::{Kmeans, Sort, get_kmeans};
use palette::cast::from_component_slice;
use palette::{Hsv, IntoColor, Lab, Srgb};

#[derive(Debug, Clone, Copy)]
pub struct DominantColor {
    rgb: Srgb<u8>,
    percentage: f32,
}

impl DominantColor {
    pub const fn rgb(self) -> (u8, u8, u8) {
        let rgb = self.rgb;
        (rgb.red, rgb.green, rgb.blue)
    }

    pub fn is_close_to_color(
        self,
        target: Srgb<u8>,
        max_hue_difference: f32,
        max_saturation_difference: f32,
        max_value_difference: f32,
    ) -> bool {
        assert!(
            (0.0..=180.0).contains(&max_hue_difference),
            "max_hue_difference must be between 0 and 180 degrees"
        );

        assert!(
            (0.0..=1.0).contains(&max_saturation_difference),
            "max_saturation_difference must be between 0 and 1"
        );

        assert!(
            (0.0..=1.0).contains(&max_value_difference),
            "max_value_difference must be between 0 and 1"
        );

        let color_hsv: Hsv = self
            .rgb
            .into_format::<f32>()
            .into_color();
        let target_hsv: Hsv = target
            .into_format::<f32>()
            .into_color();

        let hue_difference = circular_hue_difference(
            color_hsv
                .hue
                .into_degrees(),
            target_hsv
                .hue
                .into_degrees(),
        );

        let saturation_difference = (color_hsv.saturation - target_hsv.saturation).abs();

        let value_difference = (color_hsv.value - target_hsv.value).abs();

        hue_difference <= max_hue_difference
            && saturation_difference <= max_saturation_difference
            && value_difference <= max_value_difference
    }

    pub fn is_close_to_forest_green(self) -> bool {
        const FOREST_GREEN: Srgb<u8> = Srgb::new(32, 150, 32);

        self.is_close_to_color(FOREST_GREEN, 60.0, 0.80, 0.80)
    }
}

impl Display for DominantColor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let color_hsv: Hsv = self
            .rgb
            .into_format()
            .into_color();

        let hue = color_hsv
            .hue
            .into_degrees();
        let saturation = color_hsv.saturation;
        let value = color_hsv.value;
        write!(f, "[{hue}, {saturation}, {value}]")
    }
}

pub fn dominant_colors(image: &DynamicImage, amount: usize) -> Vec<DominantColor> {
    let rgb_image = image.to_rgb8();
    let pixels = rgb_image.into_raw();

    let lab_pixels: Vec<Lab> = from_component_slice::<Srgb<u8>>(&pixels)
        .iter()
        .map(|rgb| {
            rgb.into_linear()
                .into_color()
        })
        .collect();

    let max_iter = 30;
    let convergence = 5.;
    let runs = 5;

    let mut best = Kmeans::new();

    for seed in 0..runs {
        let result = get_kmeans(amount, max_iter, convergence, false, &lab_pixels, seed);

        if result.score < best.score {
            best = result;
        }
    }

    let mut clusters = Lab::sort_indexed_colors(&best.centroids, &best.indices);
    clusters.sort_unstable_by(|a, b| {
        b.percentage
            .total_cmp(&a.percentage)
    });

    clusters
        .into_iter()
        .map(|cluster| {
            let rgb: Srgb<u8> = Srgb::from_linear(
                cluster
                    .centroid
                    .into_color(),
            );

            DominantColor {
                rgb,
                percentage: cluster.percentage,
            }
        })
        .collect()
}

fn circular_hue_difference(a: f32, b: f32) -> f32 {
    let difference = (a - b)
        .abs()
        .rem_euclid(360.0);
    difference.min(360.0 - difference)
}
