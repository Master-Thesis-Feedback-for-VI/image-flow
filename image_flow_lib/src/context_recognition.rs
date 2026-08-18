use std::{collections::HashSet, fmt::Display};

use crate::requesters::{nominatim::NominatimLocationData, overpass::find_nearby_intersections};

const MIN_INTERSECTION_NODES: usize = 2;

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

impl Coordinate {
    #[allow(dead_code)]
    pub(crate) fn as_url_osm(&self) -> String {
        let lat = self.lat;
        let lon = self.lon;
        format!("https://www.openstreetmap.org/?mlat={lat}&mlon={lon}#map=17/{lat}/{lon}")
    }
}

impl Display for Coordinate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "(lat = {}, lon = {})", self.lat, self.lon)
    }
}

/// TODO
///
/// # Errors
/// TODO
pub async fn evaulate_location_into_categories<T: Into<Coordinate>>(
    pos: T,
) -> crate::Result<UniqueGeographicCategories> {
    GeographicCategory::from_cords(pos).await
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
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum GeographicCategory {
    Forest,

    Outside,
    Building,

    Street,
    Pedestrian,
    Intersection,

    Elevator,
}

pub type UniqueGeographicCategories = HashSet<GeographicCategory>;

impl GeographicCategory {
    async fn extract_categories_from_nominatim<T: Into<Coordinate>>(
        coordinates: T,
    ) -> crate::Result<UniqueGeographicCategories> {
        let loc_data = NominatimLocationData::from_cords(coordinates).await?;
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
                    loc_data.as_coordinate().as_url_osm()
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
                    loc_data.as_coordinate().as_url_osm()
                )
            }
        }

        Ok(categories)
    }

    async fn extract_categories_from_overpass<T: Into<Coordinate>>(
        coordinates: T,
    ) -> crate::Result<UniqueGeographicCategories> {
        let mut categories = HashSet::new();

        let intersections = find_nearby_intersections(coordinates, 50.).await?;
        if intersections.len() >= MIN_INTERSECTION_NODES {
            categories.insert(Self::Intersection);
        }

        Ok(categories)
    }

    /// TODO
    ///
    /// # Errors
    /// TODO
    pub async fn from_cords<T: Into<Coordinate>>(
        coordinates: T,
    ) -> crate::Result<UniqueGeographicCategories> {
        let coordinates: Coordinate = coordinates.into();

        let mut categories = Self::extract_categories_from_nominatim(coordinates).await?;
        let overpass_categories = Self::extract_categories_from_overpass(coordinates).await?;

        categories.extend(overpass_categories);
        Ok(categories)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_recognition::{Coordinate, GeographicCategory, NominatimLocationData},
        tests::data::{self, INTERSECTION_LOCATIONS},
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
            _ = GeographicCategory::extract_categories_from_nominatim(loc)
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn apartment() {
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
        insta::assert_snapshot!(&nom_data.as_coordinate().as_url_osm(), @"https://www.openstreetmap.org/?mlat=52.3715399&mlon=9.7125486#map=17/52.3715399/9.7125486");

        let categories = GeographicCategory::from_cords(element.1).await.unwrap();
        let mut vec_categories: Vec<GeographicCategory> = categories.into_iter().collect();
        vec_categories.sort();
        insta::assert_debug_snapshot!(vec_categories, @"
        [
            Building,
            Intersection,
        ]
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
        insta::assert_snapshot!(&nom_data.as_coordinate().as_url_osm(), @"https://www.openstreetmap.org/?mlat=52.3860214&mlon=9.8088121#map=17/52.3860214/9.8088121");

        let categories = GeographicCategory::from_cords(element.1).await.unwrap();
        let mut vec_categories: Vec<GeographicCategory> = categories.into_iter().collect();
        vec_categories.sort();
        insta::assert_debug_snapshot!(vec_categories, @"
        [
            Street,
            Intersection,
        ]
        ");
    }

    #[tokio::test]
    async fn intersections() {
        let intersection_locations = INTERSECTION_LOCATIONS;
        for (name, location) in intersection_locations {
            let categories = GeographicCategory::extract_categories_from_overpass(*location)
                .await
                .unwrap();

            let url = location.as_url_osm();
            assert!(
                categories.contains(&GeographicCategory::Intersection),
                "Location: {name} does not have a intersection although it should. Check out this url to inspect the location: {url}"
            );
        }

        let no_intersection = (
            "Ada-Lessing-Strasse",
            Coordinate {
                lat: 52.409_246,
                lon: 9.781_501,
            },
        );
        let categories = GeographicCategory::extract_categories_from_overpass(no_intersection.1)
            .await
            .unwrap();

        let name = no_intersection.0;
        let url = no_intersection.1.as_url_osm();
        assert!(
            !categories.contains(&GeographicCategory::Intersection),
            "Location: {name} does not have a intersection although it should. Check out this url to inspect the location: {url}"
        );
    }
}
