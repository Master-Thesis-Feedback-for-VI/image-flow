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
// static INSIDE_CATEGORIES: LazyLock<HashSet<LocationTag>> =
//     LazyLock::new(|| HashSet::from([LocationTag::Inside, LocationTag::Elevator]));

pub async fn evaluate_image_and_location_as_location_tags(
    pos: impl Into<Coordinate>,
    auth: Option<Arc<AuthToHost>>,
    dominant_colors: &DominantColors,
) -> UniqueLocationTags {
    let location_cat = evaluate_location_as_loaction_tags(pos, auth).await;
    let image_cat = evaluate_image_as_location_tags(dominant_colors);

    let mut cats = UniqueLocationTags::new();

    cats.extend(location_cat);
    cats.extend(image_cat);

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
    let (mut overpass_category, mut nominatim_category) = tokio::join!(overpass_fut, nominatim_fut);

    // dbg!(&overpass_category, &nominatim_category);

    if overpass_category.contains(LocationTag::Inside) {
        nominatim_category.remove(&LocationTag::Outside);
        // .into_iter()
        // .filter(|pred| INSIDE_CATEGORIES.contains(pred))
        // .collect();
    } else if nominatim_category.contains(LocationTag::Inside) {
        overpass_category.remove(&LocationTag::Outside);
        // .into_iter()
        // .filter(|pred| INSIDE_CATEGORIES.contains(pred))
        // .collect();
    }

    // dbg!(
    //     &overpass_category,
    //     &nominatim_category,
    //     coordinates.as_url_osm()
    // );
    categories.extend(overpass_category);
    categories.extend(nominatim_category);
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
        let mut categories = UniqueLocationTags::new();

        let location_type = loc_data
            .location_type
            .as_str();
        match location_type {
            "elevator" => {
                categories.insert(Self::Elevator);
            }
            "apartments" | "library" | "cafe" | "pub" | "house" | "university" | "hospital" => {
                categories.insert(Self::Inside);
            }
            "track" => {
                categories.insert(Self::Outside);
                categories.insert(Self::Forest);
            }
            "pedestrian" | "service" => {
                categories.insert(Self::Pedestrian);
            }
            "bicycle_parking" | "platform" => {
                categories.insert(Self::Outside);
            }
            "parking" => {
                categories.insert(Self::Parking);
            }
            "motorway" | "trunk" | "primary" | "secondary" | "tertiary" | "unclassified"
            | "residential" | "living_street" => {
                categories.insert(Self::Street);
            }
            "bench" => {}
            _ => {
                #[cfg(debug_assertions)]
                {
                    unreachable!(
                        "New location type: [[{location_type}]],\ncheck it out here: {}",
                        loc_data
                            .as_coordinate()
                            .as_url_osm()
                    )
                }
                #[cfg(not(debug_assertions))]
                {
                    let msg = format!(
                        "New location type: [[{location_type}]],\ncheck it out here: {}",
                        loc_data
                            .as_coordinate()
                            .as_url_osm()
                    );
                    tracing::error!("{}", msg);
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
            "building" => {
                categories.insert(Self::Inside);
            }
            "railway" => {
                categories.insert(Self::Railway);
            }
            "place" | "amenity" => {}
            _ => {
                unreachable!(
                    "New Class: {location_class}, check it out here: {}",
                    loc_data
                        .as_coordinate()
                        .as_url_osm()
                )
            }
        }

        // dbg!(&location_type, &location_class);
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

        let mut categories = UniqueLocationTags::new();

        let intersections = find_nearby_intersections(coordinates, 10., auth.clone())
            .await
            .inspect_err(|e| {
                tracing::error!(
                    error = %e,
                    "intersection fetch failure"
                );
            });

        if let Ok(intersections) = intersections
            && intersections.len() >= MIN_INTERSECTION_NODES
        {
            categories.insert(Self::Intersection);
            categories.insert(Self::Outside);
        }

        let inside_building = inside_building(coordinates, auth)
            .await
            .inspect_err(|e| {
                tracing::error!(
                    location = %coordinates,
                    error = %e,
                    "couldnt check if location is inside building"
                );
            });
        if let Ok(inside_building) = inside_building {
            categories.insert(inside_building);
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

    #[tokio::test]
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
    async fn check_inside_building() {
        let auth = AUTH_AGENT
            .clone()
            .map(Arc::new);
        let luetzeken = fetch_img("luetzekenstrasse.png").await;

        let tags = LocationTag::extract_categories_from_overpass(luetzeken, auth).await;

        assert!(tags.contains(LocationTag::Inside));
    }
}
