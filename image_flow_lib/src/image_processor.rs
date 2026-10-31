use std::fmt::Display;
use std::ops::Deref;
use std::path::Path;

use image::imageops::FilterType;
use kmeans_colors::{Kmeans, Sort, get_kmeans};
use palette::cast::from_component_slice;
use palette::{Hsv, IntoColor, Lab, Srgb};
use tracing::instrument;

#[cfg(test)]
const FOREST_GREEN: Srgb<u8> = Srgb::new(32, 150, 32);

#[derive(Debug)]
pub struct DominantColors {
    colors: Vec<DominantColor>,
}

impl DominantColors {
    pub fn any_close_to_forest_green(&self) -> bool {
        self.iter()
            .any(|color| color.is_close_to_forest_green())
    }
}

impl Deref for DominantColors {
    type Target = Vec<DominantColor>;

    fn deref(&self) -> &Self::Target {
        &self.colors
    }
}

impl FromIterator<DominantColor> for DominantColors {
    fn from_iter<I: IntoIterator<Item = DominantColor>>(iter: I) -> Self {
        let colors = iter
            .into_iter()
            .collect();

        Self { colors }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DominantColor {
    rgb: Srgb<u8>,

    _percentage: f32,
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
        let hsv: Hsv = self
            .rgb
            .into_format::<f32>()
            .into_color();

        // Foliage ranges from yellow-green/olive to cool green, and shadows or
        // haze can substantially reduce its saturation. Avoid a distance from a
        // single saturated reference, which misses these common vegetation colors.
        // Exclude near-neutral and nearly black colors whose hue is unreliable.
        (65.0..=165.0).contains(
            &hsv.hue
                .into_positive_degrees(),
        ) && hsv.saturation >= 0.15
            && hsv.value >= 0.08
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

#[instrument(skip(frame_path))]
pub async fn dominant_colors(
    frame_path: impl AsRef<Path>,
    amount: usize,
) -> crate::Result<DominantColors> {
    let frame_path = frame_path
        .as_ref()
        .to_path_buf();

    tokio::task::spawn_blocking(move || -> crate::Result<DominantColors> {
        let img = image::open(frame_path)?;
        let small = img.resize(200, 200, FilterType::Triangle);
        let rgb_image = small.to_rgb8();
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

        let dominant_colors = clusters
            .into_iter()
            .map(|cluster| {
                let rgb: Srgb<u8> = Srgb::from_linear(
                    cluster
                        .centroid
                        .into_color(),
                );

                DominantColor {
                    rgb,
                    _percentage: cluster.percentage,
                }
            })
            .collect::<DominantColors>();

        Ok(dominant_colors)
    })
    .await?
}

fn circular_hue_difference(a: f32, b: f32) -> f32 {
    let difference = (a - b)
        .abs()
        .rem_euclid(360.0);
    difference.min(360.0 - difference)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        context::location_based_context::evaluate_image_as_location_tags,
        location::tag::LocationTag,
    };

    fn color(rgb: Srgb<u8>) -> DominantColor {
        DominantColor {
            rgb,
            _percentage: 1.0,
        }
    }

    #[test]
    fn hue_distance_wraps_and_is_symmetric() {
        for (a, b, expected) in [
            (0.0, 0.0, 0.0),
            (0.0, 180.0, 180.0),
            (350.0, 10.0, 20.0),
            (-10.0, 10.0, 20.0),
            (720.0, 0.0, 0.0),
        ] {
            assert_eq!(circular_hue_difference(a, b), expected);
            assert_eq!(circular_hue_difference(b, a), expected);
        }
    }

    #[test]
    fn color_matching_respects_each_tolerance() {
        let red = color(Srgb::new(255, 0, 0));
        assert_eq!(red.rgb(), (255, 0, 0));
        assert!(red.is_close_to_color(red.rgb, 0.0, 0.0, 0.0));
        assert!(!red.is_close_to_color(Srgb::new(0, 255, 0), 0.0, 1.0, 1.0));
        assert!(!red.is_close_to_color(Srgb::new(255, 128, 128), 180.0, 0.0, 1.0));
        assert!(!red.is_close_to_color(Srgb::new(128, 0, 0), 180.0, 1.0, 0.0));
        assert!(red.is_close_to_color(Srgb::new(0, 255, 0), 180.0, 1.0, 1.0));
    }

    #[test]
    fn invalid_color_tolerances_panic() {
        for (hue, saturation, value) in [
            (-1.0, 0.0, 0.0),
            (181.0, 0.0, 0.0),
            (0.0, -0.1, 0.0),
            (0.0, 1.1, 0.0),
            (0.0, 0.0, -0.1),
            (0.0, 0.0, 1.1),
            (f32::NAN, 0.0, 0.0),
        ] {
            assert!(
                std::panic::catch_unwind(|| {
                    color(FOREST_GREEN).is_close_to_color(FOREST_GREEN, hue, saturation, value)
                })
                .is_err()
            );
        }
    }

    #[test]
    fn foliage_colors_include_muted_olive_and_shadowed_greens() {
        for rgb in [
            Srgb::new(110, 135, 105), // muted foliage
            Srgb::new(110, 120, 50),  // olive foliage
            Srgb::new(20, 35, 18),    // shadowed foliage
            Srgb::new(80, 200, 60),   // sunlit foliage
            FOREST_GREEN,
        ] {
            let colors: DominantColors = std::iter::once(color(rgb)).collect();
            assert!(colors.any_close_to_forest_green(), "missed {rgb:?}");
            assert!(evaluate_image_as_location_tags(&colors).contains(LocationTag::Forest));
        }
    }

    #[test]
    fn foliage_colors_exclude_neutrals_and_other_hues() {
        for rgb in [
            Srgb::new(0, 0, 0),
            Srgb::new(120, 120, 120),
            Srgb::new(240, 245, 240),
            Srgb::new(2, 5, 2),
            Srgb::new(150, 100, 50),
            Srgb::new(255, 255, 0),
            Srgb::new(0, 255, 255),
            Srgb::new(0, 0, 255),
            Srgb::new(255, 0, 0),
        ] {
            assert!(!color(rgb).is_close_to_forest_green(), "matched {rgb:?}");
        }
    }

    #[tokio::test]
    async fn muted_green_image_is_classified_as_forest() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir
            .path()
            .join("muted-foliage.png");
        image::RgbImage::from_fn(32, 32, |x, _| {
            if x < 24 {
                image::Rgb([110, 135, 105])
            } else {
                image::Rgb([110, 120, 50])
            }
        })
        .save(&path)
        .expect("save foliage fixture");
        let colors = dominant_colors(path, 2)
            .await
            .expect("extract colors");
        let tags = evaluate_image_as_location_tags(&colors);
        assert!(tags.contains(LocationTag::Forest));
        assert!(tags.contains(LocationTag::Outside));
    }

    #[test]
    fn forest_classification_requires_a_matching_color() {
        let empty: DominantColors = std::iter::empty().collect();
        assert!(!empty.any_close_to_forest_green());
        assert!(evaluate_image_as_location_tags(&empty).is_empty());
        let red: DominantColors = std::iter::once(color(Srgb::new(255, 0, 0))).collect();
        assert!(!red.any_close_to_forest_green());
        let green: DominantColors = std::iter::once(color(FOREST_GREEN)).collect();
        let tags = evaluate_image_as_location_tags(&green);
        assert!(tags.contains(LocationTag::Forest));
        assert!(tags.contains(LocationTag::Outside));
        let image_tags = crate::context::image_based_context::evaluate_image_into_location_tags(
            &green,
            &crate::image::detections::ImageDetections::new(),
        );
        assert!(image_tags.contains(LocationTag::Forest));
        assert!(image_tags.contains(LocationTag::Outside));
    }

    #[tokio::test]
    async fn solid_image_extracts_its_color() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir
            .path()
            .join("solid.png");
        image::RgbImage::from_pixel(16, 12, image::Rgb([32, 150, 32]))
            .save(&path)
            .expect("save fixture image");
        let colors = dominant_colors(path, 1)
            .await
            .expect("extract colors");
        assert_eq!(colors.len(), 1);
        let (r, g, b) = colors[0].rgb();
        assert!(r.abs_diff(32) <= 1 && g.abs_diff(150) <= 1 && b.abs_diff(32) <= 1);
        assert!(colors.any_close_to_forest_green());
    }

    #[tokio::test]
    async fn missing_and_corrupt_images_return_errors() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir
            .path()
            .join("bad.png");
        assert!(
            dominant_colors(&path, 1)
                .await
                .is_err()
        );
        std::fs::write(&path, b"not an image").expect("write corrupt image");
        assert!(
            dominant_colors(&path, 1)
                .await
                .is_err()
        );
    }
}
