//! Location tags from Nominatim, Overpass, and dominant image colors.
//!
//! Request failures are logged and treated as missing evidence. Tagging can
//! therefore return partial or empty results. The top-level
//! [`crate::recognize_location_tags`] also handles image loading and color extraction.

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
        overpass::{distance_from_carriageway_m, find_nearby_intersections, inside_building},
    },
};

const MIN_INTERSECTION_NODES: usize = 2;
const STREET_GPS_TOLERANCE_M: f64 = 2.0;
const STREET_SEARCH_RADIUS_M: f64 = 50.0;

/// Combine geographic evidence with weaker dominant-color hints.
///
/// Outdoor image hints do not override a known building-containment result.
/// Most callers should use [`crate::recognize_location_tags`], since the
/// dominant-color type is defined in an internal module.
///
/// # Panics
/// Unknown Nominatim classifications can panic in debug builds.
pub async fn evaluate_image_and_location_as_location_tags(
    pos: impl Into<Coordinate>,
    auth: Option<Arc<AuthToHost>>,
    dominant_colors: &DominantColors,
) -> UniqueLocationTags {
    let location_cat = evaluate_location_as_loaction_tags(pos, auth).await;
    let image_cat = evaluate_image_as_location_tags(dominant_colors);

    let mut cats = location_cat;
    cats.extend_context_tags(image_cat);

    cats
}

/// Return `Forest` (and its implied `Outside`) if a dominant color resembles foliage.
///
/// Returns an empty set otherwise. Most callers should use
/// [`crate::recognize_location_tags`] to load an image and extract its colors.
#[must_use]
pub fn evaluate_image_as_location_tags(dominant_colors: &DominantColors) -> UniqueLocationTags {
    if dominant_colors.any_close_to_forest_green() {
        UniqueLocationTags::new_singular(LocationTag::Forest)
    } else {
        UniqueLocationTags::new()
    }
}

/// Classify coordinates using map data without loading an image or models.
///
/// Queries building containment, intersections, reverse geocoding, and street
/// distance. Request/parse failures are logged and omitted rather than returned
/// as errors. Known building containment takes precedence over weaker outdoor
/// hints, except when road-surface distance is within two meters of GPS tolerance.
/// `auth` selects custom endpoints; `None` selects the defaults.
///
/// The misspelling `loaction` is retained in the current public API.
///
/// # Panics
/// Panics in debug builds for an unknown Nominatim class/type. In release builds
/// that error is logged and its tags are omitted.
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
    let street_fut = distance_from_carriageway_m(coordinates, STREET_SEARCH_RADIUS_M, auth.clone());
    let nominatim_fut = async {
        LocationTag::extract_categories_from_nominatim(coordinates, auth)
            .await
            .unwrap_or_else(|e| {
                tracing::error!("failed to fetch from nominatim: {e:#?}");
                UniqueLocationTags::new()
            })
    };

    let mut categories = UniqueLocationTags::new();
    let (overpass_category, nominatim_category, street_distance) =
        tokio::join!(overpass_fut, nominatim_fut, street_fut);
    categories.extend(overpass_category);
    categories.extend_context_tags(nominatim_category);
    let street_distance = street_distance.unwrap_or_else(|error| {
        tracing::error!(%error, "street distance lookup failed");
        None
    });
    apply_street_distance(&mut categories, street_distance);
    categories
}

/// `distance_from_carriageway_m` is negative on the road surface. A camera within
/// GPS tolerance of that surface is on the street, which is outdoors, so this
/// also removes any building containment caused by GPS drift.
fn apply_street_distance(
    categories: &mut UniqueLocationTags,
    distance_from_carriageway_m: Option<f64>,
) {
    if categories.contains(LocationTag::Street) {
        return;
    }
    if distance_from_carriageway_m.is_some_and(|distance| distance <= STREET_GPS_TOLERANCE_M) {
        categories.insert(LocationTag::Street);
        categories.insert(LocationTag::Outside);
    }
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
            // Nearby POIs have no equivalent tag and do not prove containment.
            "apartments" | "library" | "cafe" | "pub" | "house" | "university" | "hospital"
            | "bicycle_parking" | "platform" | "bench" | "waste_basket" | "building"
            | "memorial" | "yes" | "map" | "research" | "tomb" | "restaurant" | "ice_cream"
            | "atm" | "gift" | "mall" | "shoes" | "jewelry" | "fire_hose" | "fast_food"
            | "vending_machine" | "bakery" | "defibrillator" | "florist" | "pharmacy"
            | "luggage_locker" | "ticket_validator" | "artwork" | "clock" | "museum"
            | "fountain" | "chemist" | "sports" | "clothes" | "perfumery" | "watches"
            | "mobile_phone" | "optician" | "bank" | "office" | "internet_cafe" | "books"
            | "social_facility" | "musical_instrument" | "citywalls" | "construction"
            | "attraction" | "kindergarten" => {}
            "track" => {
                categories.insert(Self::Forest);
            }
            "pedestrian" | "service" | "footway" | "path" => {
                categories.insert(Self::Pedestrian);
            }
            "parking" | "motorcycle_parking" | "taxi" => {
                categories.insert(Self::Parking);
            }
            "motorway" | "trunk" | "primary" | "secondary" | "tertiary" | "unclassified"
            | "residential" | "living_street" | "cycleway" => {
                categories.insert(Self::Street);
            }
            "railway" => {
                categories.insert(Self::Railway);
            }
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
            "railway" | "ticket_validator" | "platform" => {
                categories.insert(Self::Railway);
            }
            "building" | "place" | "amenity" | "yes" | "historic" | "tourism" | "shop"
            | "emergency" | "office" | "landuse" | "leisure" | "information" => {}
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

    #[test]
    fn street_tag_requires_carriageway_within_gps_tolerance() {
        for (distance, expected) in [
            (None, false),
            (Some(-4.0), true),
            (Some(0.0), true),
            (Some(2.0), true),
            (Some(2.001), false),
            (Some(f64::NAN), false),
        ] {
            let mut tags = UniqueLocationTags::new_singular(LocationTag::Parking);
            super::apply_street_distance(&mut tags, distance);
            assert_eq!(tags.contains(LocationTag::Street), expected);
            assert!(tags.contains(LocationTag::Parking));
        }
    }

    #[test]
    fn standing_on_the_street_overrides_building_containment() {
        let mut tags = UniqueLocationTags::new_singular(LocationTag::Inside);
        super::apply_street_distance(&mut tags, Some(-1.0));
        assert!(tags.contains(LocationTag::Street));
        assert!(tags.contains(LocationTag::Outside));
        assert!(!tags.contains(LocationTag::Inside));

        let mut tags = UniqueLocationTags::new_singular(LocationTag::Inside);
        super::apply_street_distance(&mut tags, Some(5.0));
        assert!(tags.contains(LocationTag::Inside));
        assert!(!tags.contains(LocationTag::Street));
    }

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
    fn classification_motorcycle_parking_maps_to_parking() {
        let data = serde_json::from_value(serde_json::json!({
            "class": "amenity", "type": "motorcycle_parking",
            "lat": "52.3848509", "lon": "9.8037274",
            "display_name": "Motorcycle parking", "osm_type": "node"
        }))
        .unwrap();

        let tags = LocationTag::categories_from_nominatim_data(&data).unwrap();
        assert!(tags.contains(LocationTag::Parking));
        assert!(tags.contains(LocationTag::Street));
        assert_eq!(
            tags.iter()
                .count(),
            2
        );
    }

    #[test]
    fn classification_nearest_waste_basket_adds_no_location_tags() {
        let data = serde_json::from_value(serde_json::json!({
            "class": "amenity", "type": "waste_basket",
            "lat": "52.3849102", "lon": "9.8039816",
            "display_name": "Nearby waste basket", "osm_type": "node"
        }))
        .unwrap();

        let tags = LocationTag::categories_from_nominatim_data(&data).unwrap();
        assert!(tags.is_empty());
    }

    #[test]
    fn classification_building_service_and_memorial_are_supported() {
        for (class, location_type, expected_tags) in [
            ("building", "building", vec![]),
            (
                "highway",
                "service",
                vec![LocationTag::Street, LocationTag::Pedestrian],
            ),
            ("historic", "memorial", vec![]),
        ] {
            let data = serde_json::from_value(serde_json::json!({
                "class": class, "type": location_type,
                "lat": "52.380141", "lon": "9.7242918",
                "display_name": "Nearby feature", "osm_type": "way"
            }))
            .unwrap();

            let tags = LocationTag::categories_from_nominatim_data(&data).unwrap();
            assert_eq!(
                tags.iter()
                    .count(),
                expected_tags.len()
            );
            for tag in expected_tags {
                assert!(tags.contains(tag));
            }
        }
    }

    #[test]
    fn classification_location_types_from_pipeline_log_are_supported() {
        for (class, location_type, expected) in [
            ("tourism", "map", vec![]),
            ("information", "map", vec![]),
            ("amenity", "research", vec![]),
            ("historic", "tomb", vec![]),
            ("highway", "cycleway", vec![LocationTag::Street]),
            ("amenity", "restaurant", vec![]),
            ("amenity", "ice_cream", vec![]),
            ("amenity", "atm", vec![]),
            ("shop", "gift", vec![]),
            ("shop", "mall", vec![]),
            ("shop", "shoes", vec![]),
            ("shop", "jewelry", vec![]),
            ("emergency", "fire_hose", vec![]),
            ("amenity", "fast_food", vec![]),
            ("amenity", "vending_machine", vec![]),
            ("shop", "bakery", vec![]),
            ("emergency", "defibrillator", vec![]),
            ("shop", "florist", vec![]),
            ("amenity", "pharmacy", vec![]),
            ("amenity", "luggage_locker", vec![]),
            ("railway", "ticket_validator", vec![LocationTag::Railway]),
            ("tourism", "artwork", vec![]),
            ("amenity", "clock", vec![]),
            (
                "amenity",
                "taxi",
                vec![LocationTag::Parking, LocationTag::Street],
            ),
            ("tourism", "museum", vec![]),
            ("amenity", "fountain", vec![]),
            ("shop", "chemist", vec![]),
            ("shop", "sports", vec![]),
            ("shop", "clothes", vec![]),
            ("shop", "perfumery", vec![]),
            ("shop", "watches", vec![]),
            ("shop", "mobile_phone", vec![]),
            ("shop", "optician", vec![]),
            ("amenity", "bank", vec![]),
            ("building", "office", vec![]),
            ("amenity", "internet_cafe", vec![]),
            ("shop", "books", vec![]),
            ("amenity", "social_facility", vec![]),
            ("shop", "musical_instrument", vec![]),
            ("historic", "citywalls", vec![]),
            ("landuse", "construction", vec![]),
            (
                "highway",
                "path",
                vec![LocationTag::Pedestrian, LocationTag::Street],
            ),
            ("tourism", "attraction", vec![]),
            (
                "highway",
                "footway",
                vec![LocationTag::Pedestrian, LocationTag::Street],
            ),
            ("amenity", "kindergarten", vec![]),
            ("office", "research", vec![]),
            ("leisure", "sports", vec![]),
        ] {
            let data = serde_json::from_value(serde_json::json!({
                "class": class, "type": location_type,
                "lat": "52.0", "lon": "9.0",
                "display_name": "Pipeline feature", "osm_type": "node"
            }))
            .unwrap();
            let mut tags = LocationTag::categories_from_nominatim_data(&data).unwrap();
            assert_eq!(
                tags.categories(),
                expected
                    .into_iter()
                    .collect(),
                "{class}/{location_type}"
            );
        }
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
            ("apartments", "building", vec![]),
            ("library", "amenity", vec![]),
            ("cafe", "amenity", vec![]),
            ("pub", "amenity", vec![]),
            ("house", "building", vec![]),
            ("university", "amenity", vec![]),
            ("hospital", "amenity", vec![]),
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
            ("bicycle_parking", "amenity", vec![]),
            ("platform", "railway", vec![LocationTag::Railway]),
            ("memorial", "place", vec![]),
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
            ("map", "information", vec![]),
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
