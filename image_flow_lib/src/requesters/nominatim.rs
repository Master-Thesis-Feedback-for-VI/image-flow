use serde::Deserialize;

use crate::{
    context_recognition::Coordinate,
    error,
    requesters::client::{get_client, throttled_get},
};

/// Fetching the raw data from nominatim.
///
/// # Errors
/// TODO
pub async fn fetch_raw_nominatim_data(coords: &Coordinate) -> crate::Result<serde_json::Value> {
    let url = format!(
        "https://nominatim.openstreetmap.org/reverse?format=json&lat={}&lon={}&zoom=18&addressdetails=1&extratags=1&namedetails=1",
        coords.lat, coords.lon
    );

    let resp = throttled_get(&url)
        .await
        .send()
        .await
        .map_err(error::request)?
        .error_for_status()
        .map_err(error::request)?
        .json::<serde_json::Value>()
        .await
        .map_err(error::request)?;

    Ok(resp)
}

#[allow(dead_code)]
fn de_str_to_f64<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    s.parse::<f64>().map_err(serde::de::Error::custom)
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct NominatimLocationData {
    pub name_map: Option<String>,
    pub display_name: String,
    pub osm_type: String,
    pub class: String,
    #[serde(rename = "type")]
    pub location_type: String,

    #[serde(deserialize_with = "de_str_to_f64")]
    pub lat: f64,
    #[serde(deserialize_with = "de_str_to_f64")]
    pub lon: f64,
}

impl NominatimLocationData {
    #[allow(dead_code)]
    pub(crate) fn as_url_osm(&self) -> String {
        let lat = self.lat;
        let lon = self.lon;
        format!("https://www.openstreetmap.org/?mlat={lat}&mlon={lon}#map=17/{lat}/{lon}")
    }

    pub(crate) async fn from_cords(cords: &Coordinate) -> crate::Result<Self> {
        let data = fetch_raw_nominatim_data(cords).await?;
        let location: Self = serde_json::from_value(data)?;
        Ok(location)
    }
}
