use std::collections::HashSet;

use crate::{
    context::ContextCategory,
    location::coordinate_system::Coordinate,
    requesters::{
        nominatim::NominatimLocationData,
        overpass::{find_nearby_intersections, inside_building},
    },
};

const MIN_INTERSECTION_NODES: usize = 2;

/// TODO
///
/// # Errors
/// TODO
pub async fn evaluate_location_into_categories<T: Into<Coordinate>>(
    pos: T,
) -> UniqueGeographicCategories {
    ContextCategory::from_cords(pos).await
}

pub type UniqueGeographicCategories = HashSet<ContextCategory>;

impl ContextCategory {
    async fn extract_categories_from_nominatim<T: Into<Coordinate>>(
        coordinates: T,
    ) -> crate::Result<UniqueGeographicCategories> {
        let loc_data = NominatimLocationData::from_cords(coordinates).await?;
        let mut categories = HashSet::new();

        let location_type = loc_data
            .location_type
            .as_str();
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
                    loc_data
                        .as_coordinate()
                        .as_url_osm()
                )
            }
        }

        let location_class = loc_data
            .class
            .as_str();
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
                    loc_data
                        .as_coordinate()
                        .as_url_osm()
                )
            }
        }

        Ok(categories)
    }

    async fn extract_categories_from_overpass<T>(coordinates: T) -> UniqueGeographicCategories
    where
        T: Into<Coordinate> + Copy,
    {
        let mut categories = HashSet::new();

        let intersections = find_nearby_intersections(coordinates, 50.).await;
        if let Ok(intersections) = intersections
            && intersections.len() >= MIN_INTERSECTION_NODES
        {
            categories.insert(Self::Intersection);
            categories.insert(Self::Outside);
        }

        let inside_building = inside_building(coordinates).await;
        if let Ok(inside_building) = inside_building {
            categories.insert(inside_building);
        }

        categories
    }

    /// TODO
    ///
    /// # Errors
    /// TODO
    pub async fn from_cords<T>(coordinates: T) -> UniqueGeographicCategories
    where
        T: Into<Coordinate>,
    {
        let coordinates: Coordinate = coordinates.into();
        let mut categories = HashSet::new();

        let nominatim_categories = Self::extract_categories_from_nominatim(coordinates)
            .await
            .unwrap_or_else(|_| HashSet::new());
        let overpass_categories = Self::extract_categories_from_overpass(coordinates).await;

        categories.extend(nominatim_categories);
        categories.extend(overpass_categories);
        if categories.contains(&Self::Building) {
            categories = Self::remove_outside_categories(categories);
        }
        categories
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::{
        context::ContextCategory,
        location::coordinate_system::Coordinate,
        requesters::nominatim::NominatimLocationData,
        tests::location_data::{self, INTERSECTION_LOCATIONS},
    };

    #[tokio::test]
    async fn check_all_available_types() {
        let all_locs = [
            location_data::HARMLESS_LOCATIONS,
            location_data::MODERATE_LOCATIONS,
            location_data::DANGEROUSE_LOCATIONS,
        ]
        .concat();

        for (_name, loc) in all_locs {
            _ = ContextCategory::extract_categories_from_nominatim(loc)
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn apartment() {
        let loc = location_data::DANGEROUSE_LOCATIONS;
        let element = loc
            .get(2)
            .unwrap();
        insta::assert_debug_snapshot!(element, @r#"
        (
            "Limmerstrasse (Am Kuechengarten)",
            Coordinate {
                lat: 52.37166,
                lon: 9.71264,
            },
        )
        "#);

        let nom_data = NominatimLocationData::from_cords(element.1)
            .await
            .unwrap();
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

        let should_categories = HashSet::from([ContextCategory::Building]);
        let categories = ContextCategory::from_cords(element.1).await;

        assert_eq!(should_categories, categories);
    }

    #[tokio::test]
    async fn karl_wichert_allee() {
        let loc = location_data::DANGEROUSE_LOCATIONS;
        let element = loc
            .first()
            .unwrap();
        insta::assert_debug_snapshot!(element, @r#"
        (
            "Karl-Wiechert-Allee",
            Coordinate {
                lat: 52.38602,
                lon: 9.80881,
            },
        )
        "#);

        let nom_data = NominatimLocationData::from_cords(element.1)
            .await
            .unwrap();
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

        let should_category = HashSet::from([
            ContextCategory::Outside,
            ContextCategory::Street,
            ContextCategory::Intersection,
        ]);
        let categories = ContextCategory::from_cords(element.1).await;
        assert_eq!(should_category, categories);
    }

    #[tokio::test]
    async fn intersections() {
        let intersection_locations = INTERSECTION_LOCATIONS;
        for (name, location) in intersection_locations {
            let categories = ContextCategory::extract_categories_from_overpass(*location).await;

            let url = location.as_url_osm();
            assert!(
                categories.contains(&ContextCategory::Intersection),
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
        let categories = ContextCategory::extract_categories_from_overpass(no_intersection.1).await;

        let name = no_intersection.0;
        let url = no_intersection
            .1
            .as_url_osm();
        assert!(
            !categories.contains(&ContextCategory::Intersection),
            "Location: {name} does not have a intersection although it should. Check out this url to inspect the location: {url}"
        );
    }
}
