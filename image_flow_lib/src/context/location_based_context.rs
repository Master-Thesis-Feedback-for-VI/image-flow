use std::{path::Path, sync::Arc};

use tracing::{Span, instrument};

use crate::{
    context::{ContextCategory, UniqueGeographicCategories},
    image_processor::dominant_colors,
    location::coordinate_system::Coordinate,
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
    img_path: impl AsRef<Path>,
    amount: usize,
) -> UniqueGeographicCategories {
    let location_cat_fut = evaluate_location_as_loaction_tags(pos, auth);
    let image_cat_fut = evaluate_image_as_location_tags(img_path, amount);

    let mut cats = UniqueGeographicCategories::new();
    let (location_cat, image_cat) = tokio::join!(location_cat_fut, image_cat_fut);

    cats.extend(location_cat);
    cats.extend(image_cat);
    cats.extend_by_implied()
}

pub async fn evaluate_image_as_location_tags(
    img_path: impl AsRef<Path>,
    amount: usize,
) -> UniqueGeographicCategories {
    let dominant_colors = dominant_colors(img_path, amount).await;

    let cats = match dominant_colors.map(|value| value.any_close_to_forest_green()) {
        Ok(true) => UniqueGeographicCategories::new_singular(ContextCategory::Forest),
        Ok(false) | Err(_) => UniqueGeographicCategories::new(),
    };

    cats.extend_by_implied()
}

/// TODO
///
/// # Errors
/// TODO
pub async fn evaluate_location_as_loaction_tags(
    pos: impl Into<Coordinate>,
    auth: Option<Arc<AuthToHost>>,
) -> UniqueGeographicCategories {
    let coordinates: Coordinate = pos.into();

    let overpass_fut = ContextCategory::extract_categories_from_overpass(coordinates, auth);
    let nominatim_fut = async {
        ContextCategory::extract_categories_from_nominatim(coordinates)
            .await
            .unwrap_or_else(|e| {
                tracing::error!("failed to fetch from nominatim: {e:#?}");
                UniqueGeographicCategories::new()
            })
    };

    let mut categories = UniqueGeographicCategories::new();
    let (overpass_category, nominatim_category) = tokio::join!(overpass_fut, nominatim_fut);

    categories.extend(overpass_category);
    categories.extend(nominatim_category);
    categories
}

impl ContextCategory {
    #[instrument(skip_all, fields(
        location = tracing::field::Empty,
    ))]
    async fn extract_categories_from_nominatim<T: Into<Coordinate>>(
        coordinates: T,
    ) -> crate::Result<UniqueGeographicCategories> {
        let coordinates = coordinates.into();

        let span = Span::current();
        span.record("location", tracing::field::debug(&coordinates));

        let loc_data = NominatimLocationData::from_cords(coordinates).await?;
        let mut categories = UniqueGeographicCategories::new();

        let location_type = loc_data
            .location_type
            .as_str();
        match location_type {
            "elevator" => {
                categories.insert(Self::Elevator);
            }
            "apartments" | "library" | "cafe" => {
                categories.insert(Self::Inside);
            }
            "track" => {
                categories.insert(Self::Outside);
                categories.insert(Self::Forest);
            }
            "pedestrian" => {
                categories.insert(Self::Outside);
                categories.insert(Self::Pedestrian);
            }
            "bicycle_parking" | "house" => {
                categories.insert(Self::Outside);
            }
            "motorway" | "trunk" | "primary" | "secondary" | "tertiary" | "unclassified"
            | "residential" | "living_street" => {
                categories.insert(Self::Street);
            }
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
            "amenity" => {
                categories.insert(Self::Outside);
            }
            "place" => {}
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

    #[instrument(skip_all, fields(
        location = tracing::field::Empty,
    ))]
    async fn extract_categories_from_overpass(
        coordinates: impl Into<Coordinate> + Copy,
        auth: Option<Arc<AuthToHost>>,
    ) -> UniqueGeographicCategories {
        let coordinates = coordinates.into();

        let span = Span::current();
        span.record("location", tracing::field::debug(coordinates));

        let mut categories = UniqueGeographicCategories::new();

        let intersections = find_nearby_intersections(coordinates, 50., auth.clone())
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
mod tests {
    use std::{iter, sync::Arc};

    use crate::{
        context::{
            ContextCategory, UniqueGeographicCategories,
            location_based_context::evaluate_location_as_loaction_tags,
        },
        location::coordinate_system::Coordinate,
        requesters::nominatim::NominatimLocationData,
        tests::{
            AUTH_AGENT,
            location_data::{self, INTERSECTION_LOCATIONS},
        },
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

        let nom_data = NominatimLocationData::from_cords(element.1)
            .await
            .unwrap();
        insta::assert_debug_snapshot!(nom_data, @r#"
        NominatimLocationData {
            class: "building",
            location_type: "apartments",
            lat: 52.3715399,
            lon: 9.7125486,
            _name_map: None,
            _display_name: "3a, Limmerstraße, Linden-Nord, Linden-Limmer, Hannover, Region Hannover, Niedersachsen, 30451, Deutschland",
            _osm_type: "way",
        }
        "#);
        insta::assert_snapshot!(&nom_data.as_coordinate().as_url_osm(), @"https://www.openstreetmap.org/?mlat=52.3715399&mlon=9.7125486#map=17/52.3715399/9.7125486");

        let mut should_categories: UniqueGeographicCategories =
            iter::once(ContextCategory::Inside).collect();
        let mut categories = evaluate_location_as_loaction_tags(element.1, auth_agent).await;

        assert_eq!(should_categories.categories(), categories.categories());
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

        let nom_data = NominatimLocationData::from_cords(element.1)
            .await
            .unwrap();
        insta::assert_debug_snapshot!(nom_data, @r#"
        NominatimLocationData {
            class: "highway",
            location_type: "secondary",
            lat: 52.3860214,
            lon: 9.8088121,
            _name_map: None,
            _display_name: "Karl-Wiechert-Allee, Groß-Buchholz, Buchholz-Kleefeld, Hannover, Region Hannover, Niedersachsen, 30625, Deutschland",
            _osm_type: "way",
        }
        "#);
        insta::assert_snapshot!(&nom_data.as_coordinate().as_url_osm(), @"https://www.openstreetmap.org/?mlat=52.3860214&mlon=9.8088121#map=17/52.3860214/9.8088121");

        let mut should_category: UniqueGeographicCategories = [
            ContextCategory::Outside,
            ContextCategory::Street,
            ContextCategory::Intersection,
        ]
        .into_iter()
        .collect();
        let mut categories = evaluate_location_as_loaction_tags(element.1, auth_agent).await;
        assert_eq!(should_category.categories(), categories.categories());
    }

    #[tokio::test]
    async fn intersections() {
        let auth_agent = AUTH_AGENT
            .clone()
            .map(Arc::new);

        let intersection_locations = INTERSECTION_LOCATIONS;
        for (name, location) in intersection_locations {
            let categories =
                ContextCategory::extract_categories_from_overpass(*location, auth_agent.clone())
                    .await;

            let url = location.as_url_osm();
            assert!(
                categories.contains(ContextCategory::Intersection),
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
            ContextCategory::extract_categories_from_overpass(no_intersection.1, auth_agent).await;

        let name = no_intersection.0;
        let url = no_intersection
            .1
            .as_url_osm();
        assert!(
            !categories.contains(ContextCategory::Intersection),
            "Location: {name} does not have a intersection although it should. Check out this url to inspect the location: {url}"
        );
    }
}
