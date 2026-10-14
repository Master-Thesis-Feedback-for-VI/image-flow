use reqwest::header::{self};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::HashSet,
    fmt::Write,
    iter,
    sync::{Arc, LazyLock},
};
use tokio::time::Duration;

use crate::{
    error,
    location::{coordinate_system::Coordinate, tag::LocationTag},
    requesters::{AuthToHost, client::throttled_post},
};

// ===== Configuration =====

static OVERPASS_URLS: LazyLock<&[&str]> = LazyLock::new(|| {
    &[
        "https://overpass-api.de/api/interpreter",
        "https://overpass.private.coffee/api/interpreter",
        "https://maps.mail.ru/osm/tools/overpass/api/interpreter",
        "https://overpass.osm.ch/api/interpreter",
        "https://lz4.overpass-api.de/api/interpreter",
        "https://z.overpass-api.de/api/interpreter",
    ]
});
const OVERPASS_TIMEOUT: Duration = Duration::from_secs(30);
const ROAD_TYPES: &str =
    "motorway|trunk|primary|secondary|tertiary|unclassified|residential|living_street";

// =========================

/// Fetching the raw data from Overpass.
///
/// # Errors
/// TODO
pub async fn fetch_raw_overpass_data(
    query: &str,
    auth: Option<Arc<AuthToHost>>,
) -> crate::Result<Value> {
    let body = format!("data={}", urlencoding_encode(query));

    let auth_url = auth
        .as_ref()
        .map(|auth| {
            auth.overpass_url
                .clone()
        });

    let urls: Vec<String> = OVERPASS_URLS
        .iter()
        .flat_map(|&static_url| {
            auth_url
                .iter()
                .cloned()
                .chain(iter::once(static_url.to_owned()))
        })
        .collect();

    fetch_from_urls(&body, auth, &urls).await
}

async fn fetch_from_urls(
    body: &str,
    auth: Option<Arc<AuthToHost>>,
    urls: &[String],
) -> crate::Result<Value> {
    let mut last_error = None;

    let mut first = true;
    for url in urls {
        let mut construction = throttled_post(url)
            .await
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
        if first {
            if let Some(auth) = auth.clone() {
                construction = construction.basic_auth(
                    auth.user
                        .clone(),
                    Some(
                        auth.passwd
                            .clone(),
                    ),
                );
            }

            first = false;
        }

        let response = construction
            .body(body.to_owned())
            .timeout(OVERPASS_TIMEOUT)
            .send()
            .await;

        let response = match response {
            Ok(resp) => resp,
            Err(err) => {
                last_error = Some(error::request(err));
                continue;
            }
        };

        let response = match response.error_for_status() {
            Ok(resp) => resp,
            Err(err) => {
                last_error = Some(error::request(err));
                continue;
            }
        };

        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| {
                v.to_str()
                    .ok()
            })
            .unwrap_or("")
            .to_owned();

        let raw_text = match response
            .text()
            .await
        {
            Ok(text) => text,
            Err(err) => {
                last_error = Some(error::request(err));
                continue;
            }
        };

        if !content_type.contains("json") {
            tracing::error!(
                url = %url,
                content_type = %content_type,
                body_preview = %raw_text.chars().take(500).collect::<String>(),
                "overpass returned non-JSON body"
            );
        }

        match serde_json::from_str::<Value>(&raw_text) {
            Ok(json) => return Ok(json),
            Err(err) => {
                tracing::error!(
                    url = %url,
                    content_type = %content_type,
                    body_preview = %raw_text.chars().take(500).collect::<String>(),
                    error = %err,
                    "failed to decode overpass response as JSON"
                );
                last_error = Some(err.into());
            }
        }
    }

    Err(last_error.expect("retry loop must produce an error when all attempts fail"))
}

#[derive(Debug, Deserialize)]
struct OverpassResponse {
    elements: Vec<OverpassElement>,
}

#[derive(Debug, Deserialize)]
struct OverpassElement {
    #[serde(rename = "type")]
    elem_type: String,
    id: i64,
    lat: Option<f64>,
    lon: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Intersection {
    pub node_id: i64,
    pub lat: f64,
    pub lon: f64,
    pub distance_m: f64,
}

impl Intersection {
    pub(crate) const fn as_coordinates(&self) -> Coordinate {
        Coordinate {
            lat: self.lat,
            lon: self.lon,
        }
    }
}

impl From<Intersection> for Coordinate {
    fn from(value: Intersection) -> Self {
        value.as_coordinates()
    }
}

fn build_intersections_query<T: Into<Coordinate>>(coordinates: T, radius_m: f64) -> String {
    let coordinates: Coordinate = coordinates.into();
    let lat = coordinates.lat;
    let lon = coordinates.lon;
    format!(
        r#"[out:json][timeout:25];
way(around:{radius_m},{lat},{lon})["highway"~"^({ROAD_TYPES})$"]->.streets;
node(way_link.streets:2-)->.junctions;
node.junctions(around:{radius_m},{lat},{lon});
out geom;"#,
    )
}

/// Haversine distance in meters between two lat/lon points.
fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6_371_000.0; // Earth radius in meters
    let (lat1r, lat2r) = (lat1.to_radians(), lat2.to_radians());
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0)
        .sin()
        .mul_add(
            (dlat / 2.0).sin(),
            lat1r.cos()
                * lat2r.cos()
                * (dlon / 2.0)
                    .sin()
                    .powi(2),
        );
    let c = 2.0
        * a.sqrt()
            .asin();
    R * c
}

fn urlencoding_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push('%');
                let _ = write!(out, "{byte:02X}");
            }
        }
    }
    out
}

pub async fn inside_building(
    coordinates: impl Into<Coordinate>,
    auth: Option<Arc<AuthToHost>>,
) -> crate::Result<LocationTag> {
    let Coordinate { lat, lon } = coordinates.into();

    let query = format!("[out:json];is_in({lat},{lon})->.a;wr(pivot.a)[building];out tags;");
    let resp = fetch_raw_overpass_data(&query, auth).await?;
    let empty_building = resp["elements"]
        .as_array()
        .cloned()
        .is_some_and(|val| val.is_empty());

    let ctx = if empty_building {
        LocationTag::Outside
    } else {
        LocationTag::Inside
    };
    Ok(ctx)
}

pub async fn find_nearby_intersections(
    coordinates: impl Into<Coordinate>,
    radius_m: f64,
    auth: Option<Arc<AuthToHost>>,
) -> crate::Result<Vec<Intersection>> {
    let coordinates: Coordinate = coordinates.into();
    let lat = coordinates.lat;
    let lon = coordinates.lon;
    let query = build_intersections_query(coordinates, radius_m);

    let parsed: OverpassResponse =
        serde_json::from_value(fetch_raw_overpass_data(&query, auth).await?)
            .map_err(error::serialization)?;

    let mut seen_node_ids = HashSet::new();
    let mut intersections: Vec<Intersection> = parsed
        .elements
        .into_iter()
        .filter(|e| e.elem_type == "node")
        .filter_map(|e| {
            if !seen_node_ids.insert(e.id) {
                return None;
            }

            let (elat, elon) = (e.lat?, e.lon?);
            Some(Intersection {
                node_id: e.id,
                lat: elat,
                lon: elon,
                distance_m: haversine_m(lat, lon, elat, elon),
            })
        })
        .collect();

    intersections.sort_by(|a, b| {
        a.distance_m
            .partial_cmp(&b.distance_m)
            .unwrap_or(std::cmp::Ordering::Less)
    });
    Ok(intersections)
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use crate::{
        location::tag::LocationTag,
        requesters::overpass::inside_building,
        tests::{AUTH_AGENT, fetch_img},
    };

    #[tokio::test]
    #[ignore = "requires live map services and sample images"]
    async fn is_inside_building_test() {
        let auth = AUTH_AGENT
            .clone()
            .map(Arc::new);

        let mhh = fetch_img("MHH.png").await;
        let luetzeken = fetch_img("luetzekenstrasse.png").await;

        let res = inside_building(mhh, auth.clone())
            .await
            .unwrap();
        assert_eq!(res, LocationTag::Inside);

        let res = inside_building(luetzeken, auth)
            .await
            .unwrap();
        assert_eq!(res, LocationTag::Inside);
    }

    #[test]
    fn percent_encoding_preserves_unreserved_bytes_and_encodes_utf8() {
        assert_eq!(super::urlencoding_encode("AZaz09-_.~"), "AZaz09-_.~");
        assert_eq!(
            super::urlencoding_encode("a b+c&=\nü"),
            "a%20b%2Bc%26%3D%0A%C3%BC"
        );
        assert_eq!(super::urlencoding_encode(""), "");
    }

    #[test]
    fn haversine_known_distances_symmetry_and_antimeridian() {
        use super::haversine_m;
        assert_eq!(haversine_m(52.0, 9.0, 52.0, 9.0), 0.0);
        assert!((haversine_m(0.0, 0.0, 0.0, 1.0) - 111_194.926_645).abs() < 0.001);
        assert_eq!(
            haversine_m(52.0, 9.0, 53.0, 10.0),
            haversine_m(53.0, 10.0, 52.0, 9.0)
        );
        assert!((haversine_m(0.0, 179.0, 0.0, -179.0) - 222_389.853_29).abs() < 0.001);
    }

    #[test]
    fn intersections_query_contains_radius_coordinates_and_road_filter() {
        let query = super::build_intersections_query(
            crate::location::coordinate_system::Coordinate {
                lat: -12.5,
                lon: 9.25,
            },
            42.0,
        );
        assert!(query.contains("around:42,-12.5,9.25"));
        assert!(query.contains(super::ROAD_TYPES));
        assert!(query.contains("node(way_link.streets:2-)"));
        assert!(query.ends_with("out geom;"));
    }

    #[tokio::test]
    async fn post_request_encodes_query_and_supplies_authentication() {
        use crate::tests::http::TestServer;
        let mut server = TestServer::new("200 OK", r#"{"elements":[]}"#).await;
        let response = super::fetch_raw_overpass_data("[out:json]; a+b", Some(server.auth()))
            .await
            .expect("local Overpass query");
        assert_eq!(response["elements"], serde_json::json!([]));
        let request = server
            .request()
            .await;
        let lowercase = request.to_lowercase();
        assert!(request.starts_with("POST / HTTP/1.1"));
        assert!(lowercase.contains("content-type: application/x-www-form-urlencoded"));
        assert!(lowercase.contains("authorization: basic dgvzddpzzwnyzxq="));
        assert!(request.ends_with("data=%5Bout%3Ajson%5D%3B%20a%2Bb"));
    }

    #[tokio::test]
    async fn building_lookup_distinguishes_empty_and_nonempty_elements() {
        use crate::{location::coordinate_system::Coordinate, tests::http::TestServer};
        for (body, expected) in [
            (r#"{"elements":[]}"#, LocationTag::Outside),
            (
                r#"{"elements":[{"type":"way","id":1}]}"#,
                LocationTag::Inside,
            ),
        ] {
            let mut server = TestServer::new("200 OK", body).await;
            let actual = inside_building(Coordinate { lat: 1.0, lon: 2.0 }, Some(server.auth()))
                .await
                .expect("local building lookup");
            assert_eq!(actual, expected);
            let request = server
                .request()
                .await;
            assert!(request.contains("is_in%281%2C2%29"));
        }
    }

    #[tokio::test]
    async fn intersections_filter_deduplicate_and_sort_by_distance() {
        use crate::{location::coordinate_system::Coordinate, tests::http::TestServer};
        let body = serde_json::json!({"elements": [
            {"type":"node", "id":3, "lat":0.0, "lon":0.003},
            {"type":"way", "id":4, "lat":0.0, "lon":0.0},
            {"type":"node", "id":2, "lat":0.0, "lon":0.001},
            {"type":"node", "id":2, "lat":0.0, "lon":0.002},
            {"type":"node", "id":5},
            {"type":"node", "id":6, "lat":0.0},
            {"type":"node", "id":1, "lat":0.0, "lon":0.0}
        ]})
        .to_string();
        let mut server = TestServer::new("200 OK", &body).await;
        let intersections = super::find_nearby_intersections(
            Coordinate { lat: 0.0, lon: 0.0 },
            1000.0,
            Some(server.auth()),
        )
        .await
        .expect("local intersections lookup");
        assert_eq!(
            intersections
                .iter()
                .map(|i| i.node_id)
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert_eq!(intersections[0].distance_m, 0.0);
        assert!(
            intersections
                .windows(2)
                .all(|pair| pair[0].distance_m <= pair[1].distance_m)
        );
        let coordinate: Coordinate = intersections[1]
            .clone()
            .into();
        assert_eq!(coordinate.lon, 0.001);
        server
            .request()
            .await;
    }

    #[tokio::test]
    async fn intersections_reject_malformed_payload() {
        use crate::{location::coordinate_system::Coordinate, tests::http::TestServer};
        let mut server = TestServer::new("200 OK", r#"{"elements":"wrong type"}"#).await;
        let error = super::find_nearby_intersections(
            Coordinate { lat: 0.0, lon: 0.0 },
            10.0,
            Some(server.auth()),
        )
        .await
        .expect_err("malformed payload");
        assert!(matches!(
            crate::error::ImageFlowError::from(error),
            crate::error::ImageFlowError::Serialization { .. }
        ));
        server
            .request()
            .await;
    }

    #[tokio::test]
    async fn fallback_retries_http_and_json_errors_without_forwarding_credentials() {
        use crate::tests::http::TestServer;
        let mut unavailable = TestServer::new("503 Service Unavailable", "{}").await;
        let mut malformed = TestServer::new("200 OK", "invalid JSON").await;
        let mut success = TestServer::new("200 OK", r#"{"elements":[]}"#).await;
        let urls = [
            unavailable
                .url
                .clone(),
            malformed
                .url
                .clone(),
            success
                .url
                .clone(),
        ];
        let response = super::fetch_from_urls("data=query", Some(unavailable.auth()), &urls)
            .await
            .expect("fallback success");
        assert_eq!(response["elements"], serde_json::json!([]));
        assert!(
            unavailable
                .request()
                .await
                .to_lowercase()
                .contains("authorization:")
        );
        for server in [&mut malformed, &mut success] {
            let request = server
                .request()
                .await
                .to_lowercase();
            assert!(!request.contains("authorization:"));
            assert!(request.ends_with("data=query"));
        }
    }

    #[tokio::test]
    async fn exhausted_local_fallback_returns_last_error() {
        use crate::tests::http::TestServer;
        let mut server = TestServer::new("500 Internal Server Error", "{}").await;
        let error = super::fetch_from_urls(
            "data=query",
            None,
            &[server
                .url
                .clone()],
        )
        .await
        .expect_err("all endpoints failed");
        assert!(matches!(
            crate::error::ImageFlowError::from(error),
            crate::error::ImageFlowError::Request { .. }
        ));
        server
            .request()
            .await;
    }
}
