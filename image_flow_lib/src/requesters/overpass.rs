use reqwest::{Client, header};
use serde::Deserialize;
use std::fmt::Write;

use crate::{
    context_recognition::Coordinate,
    error,
    requesters::{build_reqwest_client_with_default_headers, default_headers, fetch_raw_data},
};

const OVERPASS_URL: &str = "https://overpass-api.de/api/interpreter";
const ROAD_TYPES: &str =
    "motorway|trunk|primary|secondary|tertiary|unclassified|residential|living_street";

/// Fetching the raw data from nominatim.
///
/// # Errors
/// TODO
pub async fn fetch_raw_nominatim_data(coords: Coordinate) -> crate::Result<serde_json::Value> {
    let url = format!(
        "https://nominatim.openstreetmap.org/reverse?format=json&lat={}&lon={}&zoom=18&addressdetails=1&extratags=1&namedetails=1",
        coords.lat, coords.lon
    );

    let resp = fetch_raw_data(&url).await?;
    Ok(resp)
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

fn build_query(lat: f64, lon: f64, radius_m: f64) -> String {
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
    let a = (dlat / 2.0).sin().mul_add(
        (dlat / 2.0).sin(),
        lat1r.cos() * lat2r.cos() * (dlon / 2.0).sin().powi(2),
    );
    let c = 2.0 * a.sqrt().asin();
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

pub async fn find_nearby_intersections(
    coordinates: &Coordinate,
    radius_m: f64,
) -> crate::Result<Vec<Intersection>> {
    let lat = coordinates.lat;
    let lon = coordinates.lon;
    let query = build_query(lat, lon, radius_m);

    let mut headers = default_headers();
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/x-www-form-urlencoded"),
    );
    let cli = Client::builder()
        .default_headers(headers)
        .build()
        .map_err(error::request)?;
    let response = cli
        .post(OVERPASS_URL)
        .body(format!("data={}", urlencoding_encode(&query)))
        .send()
        .await
        .map_err(error::request)?;

    if !response.status().is_success() {
        return Err(error::request(String::from(
            "Failed to send request to overpass",
        )));
    }

    let parsed: OverpassResponse = response.json().await.map_err(error::serialization)?;

    let mut intersections: Vec<Intersection> = parsed
        .elements
        .into_iter()
        .filter(|e| e.elem_type == "node")
        .filter_map(|e| {
            let (elat, elon) = (e.lat?, e.lon?);
            Some(Intersection {
                node_id: e.id,
                lat: elat,
                lon: elon,
                distance_m: haversine_m(lat, lon, elat, elon),
            })
        })
        .collect();

    intersections.sort_by(|a, b| a.distance_m.partial_cmp(&b.distance_m).unwrap());
    Ok(intersections)
}
