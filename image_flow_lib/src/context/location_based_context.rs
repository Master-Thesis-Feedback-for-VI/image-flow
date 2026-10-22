use std::sync::Arc;

use tracing::{Span, instrument};

use crate::{
    image_processor::DominantColors,
    location::{
        coordinate_system::Coordinate,
        tag::{LocationTag, UniqueLocationTags},
    },
    requesters::{
        AuthToHost,
        nominatim::NominatimLocationData,
        overpass::{find_nearby_intersections, inside_building},
    },
};

const MIN_INTERSECTION_NODES: usize = 2;

pub async fn evaluate_image_and_location_as_location_tags(
    pos: impl Into<Coordinate>,
    auth: Option<Arc<AuthToHost>>,
    dominant_colors: &DominantColors,
) -> UniqueLocationTags {
    let location_cat = evaluate_location_as_loaction_tags(pos, auth).await;
    let image_cat = evaluate_image_as_location_tags(dominant_colors);

    let mut cats = UniqueLocationTags::new();

    cats.extend(location_cat);
    cats.extend_context_tags(image_cat);

    cats
}

#[must_use]
pub fn evaluate_image_as_location_tags(dominant_colors: &DominantColors) -> UniqueLocationTags {
    if dominant_colors.any_close_to_forest_green() {
        UniqueLocationTags::new_singular(LocationTag::Forest)
    } else {
        UniqueLocationTags::new()
    }
}

/// TODO
///
/// # Errors
/// TODO
pub async fn evaluate_location_as_loaction_tags(
    pos: impl Into<Coordinate>,
    auth: Option<Arc<AuthToHost>>,
) -> UniqueLocationTags {
    let coordinates: Coordinate = pos.into();
    // dbg!(coordinates.as_url_osm());

    let overpass_auth = auth
        .as_ref()
        .map(Arc::clone);

    let overpass_fut = LocationTag::extract_categories_from_overpass(coordinates, overpass_auth);
    let nominatim_fut = async {
        LocationTag::extract_categories_from_nominatim(coordinates, auth)
            .await
            .unwrap_or_else(|e| {
                tracing::error!("failed to fetch from nominatim: {e:#?}");
                UniqueLocationTags::new()
            })
    };

    let mut categories = UniqueLocationTags::new();
    let (overpass_category, nominatim_category) = tokio::join!(overpass_fut, nominatim_fut);
    categories.extend(overpass_category);
    categories.extend_context_tags(nominatim_category);
    categories
}

impl LocationTag {
    #[instrument(skip_all, fields(
        location = tracing::field::Empty,
    ))]
    async fn extract_categories_from_nominatim<T: Into<Coordinate>>(
        coordinates: T,
        auth: Option<Arc<AuthToHost>>,
    ) -> crate::Result<UniqueLocationTags> {
        let coordinates = coordinates.into();

        let span = Span::current();
        span.record("location", tracing::field::debug(&coordinates));

        let loc_data = NominatimLocationData::from_cords(coordinates, auth).await?;
        Self::categories_from_nominatim_data(&loc_data)
    }

    // Unknown values panic in debug builds, but return errors in release builds.
    #[cfg_attr(debug_assertions, allow(clippy::unnecessary_wraps))]
    fn categories_from_nominatim_data(
        loc_data: &NominatimLocationData,
    ) -> crate::Result<UniqueLocationTags> {
        let mut categories = UniqueLocationTags::new();
        // Reverse geocoding returns the nearest suitable OSM object, not an
        // object guaranteed to contain the requested point. It cannot establish
        // whether the camera is inside that object (or outside a nearby road).
        let location_type = loc_data
            .location_type
            .as_str();
        match location_type {
            "elevator" => {
                categories.insert(Self::Elevator);
            }
            "apartments" | "library" | "cafe" | "pub" | "house" | "university" | "hospital"
            | "bicycle_parking" | "platform" | "bench" => {}
            "track" => {
                categories.insert(Self::Forest);
            }
            "pedestrian" | "service" => {
                categories.insert(Self::Pedestrian);
            }
            "bicycle_parking" | "platform" | "memorial" => {
                categories.insert(Self::Outside);
            }
            "parking" => {
                categories.insert(Self::Parking);
            }
            "motorway" | "trunk" | "primary" | "secondary" | "tertiary" | "unclassified"
            | "residential" | "living_street" => {
                categories.insert(Self::Street);
            }
            "bench" | "yes" => {}
            _ => {
                let msg = format!(
                    "New location type: [[{location_type}]],\ncheck it out here: {}",
                    loc_data
                        .as_coordinate()
                        .as_url_osm()
                );
                #[cfg(debug_assertions)]
                unreachable!("{msg}");
                #[cfg(not(debug_assertions))]
                {
                    tracing::error!("{msg}");
                    return Err(crate::error::request(msg));
                }
            }
        }

        let location_class = loc_data
            .class
            .as_str();
        match location_class {
            "highway" => {
                categories.insert(Self::Street);
            }
            "railway" => {
                categories.insert(Self::Railway);
            }
            "building" | "place" | "amenity" | "yes" => {}
            _ => {
                let msg = format!(
                    "New Class: {location_class}, check it out here: {}",
                    loc_data
                        .as_coordinate()
                        .as_url_osm()
                );
                #[cfg(debug_assertions)]
                unreachable!("{msg}");
                #[cfg(not(debug_assertions))]
                {
                    tracing::error!("{msg}");
                    return Err(crate::error::request(msg));
                }
            }
        }

        Ok(categories)
    }

    #[instrument(skip_all, fields(
        location = tracing::field::Empty,
    ))]
    async fn extract_categories_from_overpass(
        coordinates: impl Into<Coordinate>,
        auth: Option<Arc<AuthToHost>>,
    ) -> UniqueLocationTags {
        let coordinates = coordinates.into();

        let span = Span::current();
        span.record("location", tracing::field::debug(coordinates));

        let intersections = find_nearby_intersections(coordinates, 10., auth.clone())
            .await
            .inspect_err(|e| {
                tracing::error!(
                    error = %e,
                    "intersection fetch failure"
                );
            });

        let inside_building = inside_building(coordinates, auth)
            .await
            .inspect_err(|e| {
                tracing::error!(
                    location = %coordinates,
                    error = %e,
                    "couldnt check if location is inside building"
                );
            });
        let near_intersection =
            intersections.is_ok_and(|nodes| nodes.len() >= MIN_INTERSECTION_NODES);
        Self::categories_from_overpass_data(inside_building.ok(), near_intersection)
    }

    fn categories_from_overpass_data(
        building: Option<Self>,
        near_intersection: bool,
    ) -> UniqueLocationTags {
        let mut categories = UniqueLocationTags::new();
        if let Some(inside_building) = building {
            categories.insert(inside_building);
        }

        // A nearby road junction does not put a camera inside a building outdoors.
        if !categories.contains(Self::Inside) && near_intersection {
            categories.insert(Self::Intersection);
        }

        categories
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use crate::{
        context::location_based_context::evaluate_location_as_loaction_tags,
        location::{
            coordinate_system::Coordinate,
            tag::{LocationTag, UniqueLocationTags},
        },
        requesters::nominatim::NominatimLocationData,
        tests::{
            AUTH_AGENT, fetch_img,
            location_data::{self, INTERSECTION_LOCATIONS},
        },
    };

    #[test]
    fn classification_nearest_building_does_not_imply_inside() {
        for (class, location_type) in [
            ("building", "apartments"),
            ("amenity", "hospital"),
            ("amenity", "cafe"),
        ] {
            let data = serde_json::from_value(serde_json::json!({
                "class": class, "type": location_type,
                "lat": "52.0", "lon": "9.0",
                "display_name": "Nearby feature", "osm_type": "way"
            }))
            .unwrap();
            let tags = LocationTag::categories_from_nominatim_data(&data).unwrap();
            assert!(!tags.contains(LocationTag::Inside));
            assert!(!tags.contains(LocationTag::Outside));
        }
    }

    #[test]
    fn classification_nearest_bench_does_not_imply_inside_or_outside() {
        let data = serde_json::from_value(serde_json::json!({
            "class": "amenity", "type": "bench",
            "lat": "52.3848136", "lon": "9.8039927",
            "display_name": "Nearby bench", "osm_type": "node"
        }))
        .unwrap();

        let tags = LocationTag::categories_from_nominatim_data(&data).unwrap();
        assert!(tags.is_empty());
    }

    #[test]
    fn classification_unknown_nominatim_values_fail_explicitly() {
        for (class, location_type) in [("amenity", "unmapped_type"), ("unmapped_class", "house")] {
            let data = serde_json::from_value(serde_json::json!({
                "class": class, "type": location_type,
                "lat": "52.0", "lon": "9.0",
                "display_name": "Unknown feature", "osm_type": "way"
            }))
            .unwrap();

            #[cfg(debug_assertions)]
            assert!(
                std::panic::catch_unwind(|| { LocationTag::categories_from_nominatim_data(&data) })
                    .is_err()
            );

            #[cfg(not(debug_assertions))]
            assert!(LocationTag::categories_from_nominatim_data(&data).is_err());
        }
    }

    #[test]
    fn classification_building_containment_beats_nearby_intersection() {
        let tags = LocationTag::categories_from_overpass_data(Some(LocationTag::Inside), true);
        assert!(tags.contains(LocationTag::Inside));
        assert!(!tags.contains(LocationTag::Outside));
        assert!(!tags.contains(LocationTag::Intersection));
    }

    #[test]
    fn classification_outdoor_intersection_is_preserved() {
        let tags = LocationTag::categories_from_overpass_data(Some(LocationTag::Outside), true);
        assert!(tags.contains(LocationTag::Outside));
        assert!(tags.contains(LocationTag::Intersection));
        assert!(!tags.contains(LocationTag::Inside));
    }

    #[test]
    fn classification_failed_building_lookup_is_not_a_location_verdict() {
        let tags = LocationTag::categories_from_overpass_data(None, false);
        assert!(tags.is_empty());
    }

    #[tokio::test]
    #[ignore = "requires live map services"]
    async fn check_all_available_types() {
        let auth_agent = AUTH_AGENT
            .clone()
            .map(Arc::new);

        let all_locs = [
            location_data::HARMLESS_LOCATIONS,
            location_data::MODERATE_LOCATIONS,
            location_data::DANGEROUSE_LOCATIONS,
        ]
        .concat();

        for (_name, loc) in all_locs {
            let auth = auth_agent
                .as_ref()
                .map(Arc::clone);
            _ = LocationTag::extract_categories_from_nominatim(loc, auth)
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    #[ignore = "requires live map services"]
    async fn apartment() {
        let auth_agent = AUTH_AGENT
            .clone()
            .map(Arc::new);

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

        let auth = auth_agent
            .as_ref()
            .map(Arc::clone);
        let nom_data = NominatimLocationData::from_cords(element.1, auth)
            .await
            .unwrap();
        insta::assert_debug_snapshot!(nom_data, @r#"
        NominatimLocationData {
            class: "highway",
            location_type: "pedestrian",
            lat: 52.3729985,
            lon: 9.708789774613281,
            _name_map: None,
            _display_name: "Linden-Nord, Linden-Limmer, Hannover, Region Hannover, Niedersachsen, 30451, Deutschland",
            _osm_type: "way",
        }
        "#);
        insta::assert_snapshot!(&nom_data.as_coordinate().as_url_osm(), @"https://www.openstreetmap.org/?mlat=52.3729985&mlon=9.708789774613281#map=17/52.3729985/9.708789774613281");

        let mut categories: Vec<_> = evaluate_location_as_loaction_tags(element.1, auth_agent)
            .await
            .categories()
            .into_iter()
            .collect();
        categories.sort();
        insta::assert_debug_snapshot!(categories, @"
        [
            Street,
            Pedestrian,
            Outside,
        ]
        ");
    }

    #[tokio::test]
    #[ignore = "requires live map services"]
    async fn karl_wichert_allee() {
        let auth_agent = AUTH_AGENT
            .clone()
            .map(Arc::new);

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

        let auth = auth_agent
            .as_ref()
            .map(Arc::clone);
        let nom_data = NominatimLocationData::from_cords(element.1, auth)
            .await
            .unwrap();
        insta::assert_debug_snapshot!(nom_data, @r#"
        NominatimLocationData {
            class: "highway",
            location_type: "secondary",
            lat: 52.386021433663096,
            lon: 9.808812145300207,
            _name_map: None,
            _display_name: "Karl-Wiechert-Allee, Groß-Buchholz, Buchholz-Kleefeld, Hannover, Region Hannover, Niedersachsen, 30625, Deutschland",
            _osm_type: "way",
        }
        "#);
        insta::assert_snapshot!(&nom_data.as_coordinate().as_url_osm(), @"https://www.openstreetmap.org/?mlat=52.386021433663096&mlon=9.808812145300207#map=17/52.386021433663096/9.808812145300207");

        let mut should_category: UniqueLocationTags = [
            LocationTag::Outside,
            LocationTag::Street,
            LocationTag::Intersection,
        ]
        .into_iter()
        .collect();
        let mut categories = evaluate_location_as_loaction_tags(element.1, auth_agent).await;
        assert_eq!(should_category.categories(), categories.categories());
    }

    #[tokio::test]
    #[ignore = "requires live map services"]
    async fn intersections_test() {
        let auth_agent = AUTH_AGENT
            .clone()
            .map(Arc::new);

        let intersection_locations = INTERSECTION_LOCATIONS;
        for (name, location) in intersection_locations {
            let categories =
                LocationTag::extract_categories_from_overpass(*location, auth_agent.clone()).await;

            let url = location.as_url_osm();
            assert!(
                categories.contains(LocationTag::Intersection),
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
        let categories =
            LocationTag::extract_categories_from_overpass(no_intersection.1, auth_agent).await;

        let name = no_intersection.0;
        let url = no_intersection
            .1
            .as_url_osm();
        assert!(
            !categories.contains(LocationTag::Intersection),
            "Location: {name} does not have a intersection although it should. Check out this url to inspect the location: {url}"
        );
    }

    #[tokio::test]
    #[ignore = "requires live map services and sample images"]
    async fn check_inside_building() {
        let auth = AUTH_AGENT
            .clone()
            .map(Arc::new);
        let luetzeken = fetch_img("luetzekenstrasse.png").await;

        let tags = LocationTag::extract_categories_from_overpass(luetzeken, auth).await;

        assert!(tags.contains(LocationTag::Inside));
    }

    #[tokio::test]
    async fn nominatim_types_and_classes_map_to_expected_tags_without_live_services() {
        use crate::tests::http::TestServer;
        for (kind, class, expected) in [
            ("elevator", "amenity", vec![LocationTag::Elevator]),
            ("apartments", "building", vec![LocationTag::Inside]),
            ("library", "amenity", vec![LocationTag::Inside]),
            ("cafe", "amenity", vec![LocationTag::Inside]),
            ("pub", "amenity", vec![LocationTag::Inside]),
            ("house", "building", vec![LocationTag::Inside]),
            ("university", "amenity", vec![LocationTag::Inside]),
            ("hospital", "amenity", vec![LocationTag::Inside]),
            (
                "track",
                "highway",
                vec![
                    LocationTag::Forest,
                    LocationTag::Outside,
                    LocationTag::Street,
                ],
            ),
            (
                "pedestrian",
                "highway",
                vec![LocationTag::Pedestrian, LocationTag::Street],
            ),
            (
                "service",
                "highway",
                vec![LocationTag::Pedestrian, LocationTag::Street],
            ),
            ("bicycle_parking", "amenity", vec![LocationTag::Outside]),
            (
                "platform",
                "railway",
                vec![LocationTag::Outside, LocationTag::Railway],
            ),
            ("memorial", "place", vec![LocationTag::Outside]),
            (
                "parking",
                "amenity",
                vec![LocationTag::Parking, LocationTag::Street],
            ),
            ("motorway", "highway", vec![LocationTag::Street]),
            ("trunk", "highway", vec![LocationTag::Street]),
            ("primary", "highway", vec![LocationTag::Street]),
            ("secondary", "highway", vec![LocationTag::Street]),
            ("tertiary", "highway", vec![LocationTag::Street]),
            ("unclassified", "highway", vec![LocationTag::Street]),
            ("residential", "highway", vec![LocationTag::Street]),
            ("living_street", "highway", vec![LocationTag::Street]),
            ("bench", "amenity", vec![]),
            ("yes", "yes", vec![]),
        ] {
            let body = serde_json::json!({
                "type": kind, "class": class, "lat": "52.5", "lon": "9.25",
                "display_name": "Fixture", "osm_type": "way"
            })
            .to_string();
            let mut server = TestServer::new("200 OK", &body).await;
            let mut actual = LocationTag::extract_categories_from_nominatim(
                Coordinate {
                    lat: 52.5,
                    lon: 9.25,
                },
                Some(server.auth()),
            )
            .await
            .expect("classify local response");
            assert_eq!(
                actual.categories(),
                expected
                    .into_iter()
                    .collect(),
                "{kind}/{class}"
            );
            server
                .request()
                .await;
        }
    }
}
