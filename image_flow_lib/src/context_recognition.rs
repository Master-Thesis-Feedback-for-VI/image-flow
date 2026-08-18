use std::{collections::HashSet, fmt::Display};


use crate::requesters::nominatim::NominatimLocationData;

#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct Coordinate {
    pub lat: f64,
    pub lon: f64,
}

impl Coordinate {
    /// This is pure for Debugging purposes
    #[allow(dead_code)]
    #[must_use]
    pub fn print_location_url(self) -> String {
        format!(
            "https://www.openstreetmap.org/?mlat={}&mlon={}#map=16/{}/{}",
            self.lat, self.lon, self.lat, self.lon
        )
    }
}

impl Display for Coordinate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "(lat = {}, lon = {})", self.lat, self.lon)
    }
}

/// [`DangerLevel`] describes how dangerouse a given context is. This is a general description with
/// Levels similar to logging libraries. Concrete examples of Dangers are for example Danger
/// depending on the location. Danger is in general very subjective and really dependant on an
/// situation. This is in no way a perfect solution but rather a heurestic which that can help
/// assessing the real danger of the situation by the user.
///
/// Currently we have the following dangers context defined:
/// - [`LocationDangerLevel`] TODO
/// - [`ImageDetectionDangerLevel`] TODO
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Ord, Eq, uniffi::Enum)]
pub enum DangerLevel {
    /// Debug only server for testing and development purposes
    Debug,

    /// Harmless is the less urgent level of danger. Harmless is used when no danger whats so ever
    /// can be inferred.
    ///
    /// ## Context [`LocationDangerLevel`]
    /// These are locations where other traffic participants simply do not exists, are strongly
    /// reduced or simply is constrained to other pedestriants. In these situations we consider the
    /// location as harmless.
    Harmless,

    /// We define Moderate as situations which can potentially lead to a dangerouse situation.
    ///
    /// ## Context [`LocationDangerLevel`]
    /// The Moderate locations are those that are usually safe but can get well travelled by
    /// pedestrians or other traffic participants such as in intersections where all type of
    /// traffic participants can meet, making it potentially dangerouse.
    ///
    /// Other Situations can include when the user approaches an escalator or in genral stairs
    /// which can lead to falling.
    Moderate,

    /// As Dangerouse we define situations where it is per default dangerouse and requires the most
    /// amount of attention.
    ///
    /// ## Context [`LocationDangerLevel`]
    /// These are locations which should always be treated with care. If the usere is positioned on
    /// the street it should be marked dangerouse.
    Dangerouse,
}

/// All potential geographic categories supported.
///
/// <https://github.com/Master-Thesis-Feedback-for-VI/image-flow/issues/2>
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
enum GeographicCategory {
    Forest,

    Outside,
    Building,

    Street,
    Pedestrian,
    Intersection,

    Elevator,
}

impl GeographicCategory {
    fn into_category(loc_data: &NominatimLocationData) -> HashSet<Self> {
        let mut categories = HashSet::new();
        let location_type = loc_data.location_type.as_str();
        match location_type {
            "elevator" => {
                categories.insert(Self::Elevator);
            }
            "secondary" => {}
            "apartments" => {
                categories.insert(Self::Building);
            }
            "track" => {
                categories.insert(Self::Outside);
                categories.insert(Self::Forest);
            }
            "pedestrian" => {
                categories.insert(Self::Outside);
                categories.insert(Self::Pedestrian);
            }
            "bicycle_parking" => {
                categories.insert(Self::Outside);
            }
            _ => {
                unreachable!(
                    "New Type: {location_type}, check it out here: {}",
                    loc_data.as_url_osm()
                )
            }
        }

        let location_class = loc_data.class.as_str();
        match location_class {
            "highway" => {
                categories.insert(Self::Street);
            }
            "building" => {
                categories.insert(Self::Building);
            }
            "amenity" => {
                categories.insert(Self::Outside);
            }
            _ => {
                unreachable!(
                    "New Class: {location_class}, check it out here: {}",
                    loc_data.as_url_osm()
                )
            }
        }

        categories
    }

    pub async fn from_cords(coordinates: Coordinate) -> crate::Result<HashSet<Self>> {
        let location = NominatimLocationData::from_cords(coordinates).await?;
        let categories = Self::into_category(&location);

        Ok(categories)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_recognition::{GeographicCategory, NominatimLocationData},
        tests::data,
    };

    #[tokio::test]
    async fn check_all_available_types() {
        let all_locs = [
            data::HARMLESS_LOCATIONS,
            data::MODERATE_LOCATIONS,
            data::DANGEROUSE_LOCATIONS,
        ]
        .concat();

        for (_name, loc) in all_locs {
            _ = GeographicCategory::from_cords(loc).await.unwrap();
        }
    }

    #[tokio::test]
    async fn appartment() {
        let loc = data::DANGEROUSE_LOCATIONS;
        let element = loc.get(2).unwrap();
        insta::assert_debug_snapshot!(element, @r#"
        (
            "Limmerstrasse (Am Kuechengarten)",
            Coordinate {
                lat: 52.37166,
                lon: 9.71264,
            },
        )
        "#);

        let nom_data = NominatimLocationData::from_cords(element.1).await.unwrap();
        insta::assert_debug_snapshot!(nom_data, @r#"
        NominatimLocationData {
            name_map: None,
            display_name: "3a, Limmerstraße, Linden-Nord, Linden-Limmer, Hannover, Region Hannover, Niedersachsen, 30451, Deutschland",
            osm_type: "way",
            class: "building",
            location_type: "apartments",
            lat: 52.3715399,
            lon: 9.7125486,
        }
        "#);
        insta::assert_snapshot!(nom_data.as_url_osm(), @"https://www.openstreetmap.org/?mlat=52.3715399&mlon=9.7125486#map=17/52.3715399/9.7125486");

        let nom_data = GeographicCategory::into_category(&nom_data);
        insta::assert_debug_snapshot!(nom_data, @"
        {
            Building,
        }
        ");
    }

    #[tokio::test]
    async fn karl_wichert_allee() {
        let loc = data::DANGEROUSE_LOCATIONS;
        let element = loc.first().unwrap();
        insta::assert_debug_snapshot!(element, @r#"
        (
            "Karl-Wiechert-Allee",
            Coordinate {
                lat: 52.38602,
                lon: 9.80881,
            },
        )
        "#);

        let nom_data = NominatimLocationData::from_cords(element.1).await.unwrap();
        insta::assert_debug_snapshot!(nom_data, @r#"
        NominatimLocationData {
            name_map: None,
            display_name: "Karl-Wiechert-Allee, Groß-Buchholz, Buchholz-Kleefeld, Hannover, Region Hannover, Niedersachsen, 30625, Deutschland",
            osm_type: "way",
            class: "highway",
            location_type: "secondary",
            lat: 52.3860214,
            lon: 9.8088121,
        }
        "#);
        insta::assert_snapshot!(nom_data.as_url_osm(), @"https://www.openstreetmap.org/?mlat=52.3860214&mlon=9.8088121#map=17/52.3860214/9.8088121");

        let nom_data = GeographicCategory::into_category(&nom_data);
        insta::assert_debug_snapshot!(nom_data, @"
        {
            Street,
        }
        ");
    }
}
