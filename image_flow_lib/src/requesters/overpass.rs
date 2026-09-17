use reqwest::header::{self};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::HashSet,
    fmt::Write,
    string::ToString,
    sync::{Arc, LazyLock},
};
use tokio::time::Duration;

use crate::{
    context::ContextCategory,
    error,
    location::coordinate_system::Coordinate,
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

    let urls: Vec<String> = auth
        .clone()
        .map(|val| {
            val.url
                .clone()
        })
        .into_iter()
        .chain(
            OVERPASS_URLS
                .iter()
                .map(ToString::to_string),
        )
        .collect();

    let mut last_error = None;

    let mut first = true;
    for url in &urls {
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
            .body(body.clone())
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
) -> crate::Result<ContextCategory> {
    let Coordinate { lat, lon } = coordinates.into();

    let query = format!("[out:json];is_in({lat},{lon})->.a;wr(pivot.a)[building];out tags;");
    let resp = fetch_raw_overpass_data(&query, auth).await?;
    let empty_building = resp["elements"]
        .as_array()
        .cloned()
        .is_some_and(|val| val.is_empty());

    let ctx = if empty_building {
        ContextCategory::Outside
    } else {
        ContextCategory::Inside
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
