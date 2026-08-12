#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct Coordinate {
    pub lat: f64,
    pub lon: f64,
}

impl Coordinate {
    /// This is pure for Debugging purposes
    #[allow(dead_code)]
    fn print_location_url(self) -> String {
        return format!(
            "https://www.openstreetmap.org/?mlat={}&mlon={}#map=16/{}/{}",
            self.lat, self.lon, self.lat, self.lon
        );
    }
}

/// [`DangerLevel`] describes how dangerouse a given context is. This is a general description with
/// Levels similar to logging libraries. Concrete examples of Dangers are for example Danger
/// depending on the location. Danger is in genral very subjective and really dependant on an
/// situation. This is in no way a perfect solution but rather a heurestic which that can help
/// assesing the real danger of the situation by the user.
///
/// Currently we have the following dangers context defined:
/// - [`LocationDangerLevel`] TODO
/// - [`ImageDetectionDangerLevel`] TODO
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Ord, Eq, uniffi::Enum)]
pub enum DangerLevel {
    /// Debug only server for testing and development purposes
    Debug,
    /// TODO explain this
    Harmless,
    /// TODO explain this
    Moderate,
    /// TODO explain this
    Dangerouse,
}

pub type LocationDangerLevel = DangerLevel;
impl LocationDangerLevel {
    pub async fn from_coords(coordinates: Coordinate) -> DangerLevel {
        todo!("IMPLEMENT")
        // DangerLevel::Dangerouse
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    // TODO: refer to about the Veloroute Classification https://github.com/Master-Thesis-Feedback-for-VI/image-flow/issues/1
    #[tokio::test]
    async fn infer_from_coords_test() {
        // Dangerouse Locations
        {
            let dangerouse_locations = HashMap::from([
                (
                    "Karl-Wiechert-Allee",
                    Coordinate {
                        lat: 52.38602,
                        lon: 9.80881,
                    },
                ),
                (
                    "Hannoversche Strasse",
                    Coordinate {
                        lat: 52.38704,
                        lon: 9.83569,
                    },
                ),
                (
                    "Limmerstrasse (Am Kuechengarten)",
                    Coordinate {
                        lat: 52.37166,
                        lon: 9.71264,
                    },
                ),
                (
                    "Im Maschsee",
                    Coordinate {
                        lat: 52.36241,
                        lon: 9.73834,
                    },
                ),
            ]);

            for (name, cord) in dangerouse_locations {
                let danger_level = LocationDangerLevel::from_coords(cord).await;
                assert_eq!(
                    danger_level,
                    DangerLevel::Dangerouse,
                    "failed for location: {name} ({cord:?})\nurl: {}",
                    cord.print_location_url()
                )
            }
        }

        // Moderate Locations
        {
            let moderate_locations = HashMap::from([
                (
                    "Berckhusenstrasse",
                    Coordinate {
                        lat: 52.37423,
                        lon: 9.79119,
                    },
                ),
                (
                    "Marienstrasse Rolltreppe",
                    Coordinate {
                        lat: 52.37032,
                        lon: 9.75328,
                    },
                ),
                (
                    "Steintor",
                    Coordinate {
                        lat: 52.37592,
                        lon: 9.73218,
                    },
                ),
            ]);

            for (name, cord) in moderate_locations {
                let danger_level = LocationDangerLevel::from_coords(cord).await;
                assert_eq!(
                    danger_level,
                    DangerLevel::Moderate,
                    "failed for location: {name} ({cord:?})\nurl: {}",
                    cord.print_location_url()
                )
            }
        }

        // Harmless Locations
        {
            let harmless_locations = HashMap::from([(
                "Eilenriede",
                Coordinate {
                    lat: 52.38305,
                    lon: 9.78945,
                },
            )]);

            for (name, cord) in harmless_locations {
                let danger_level = LocationDangerLevel::from_coords(cord).await;
                assert_eq!(
                    danger_level,
                    DangerLevel::Harmless,
                    "failed for location: {name} ({cord:?})\nurl: {}",
                    cord.print_location_url()
                )
            }
        }
    }
}
