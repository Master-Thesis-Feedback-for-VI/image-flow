use std::sync::Arc;

use serde::Deserialize;

use crate::{
    error,
    location::coordinate_system::Coordinate,
    requesters::{AuthToHost, client::throttled_get},
};

// TODO: Implement the above!
/// We could use here overpass agin to fetch what is arounfd the user instead of the nearest
/// availble object
const NOMINATIM_URL: &str = match option_env!("NOMINATIM_URL") {
    Some(url) => url,
    None => "https://nominatim.openstreetmap.org",
};

/// Fetching the raw data from nominatim.
///
/// # Errors
/// TODO
pub async fn fetch_raw_nominatim_data<T: Into<Coordinate>>(
    coordinates: T,
    auth: Option<Arc<AuthToHost>>,
) -> crate::Result<serde_json::Value> {
    let coords: Coordinate = coordinates.into();
    let arguments = format!(
        "/reverse?format=json&lat={}&lon={}&zoom=18&addressdetails=1&extratags=1&namedetails=1",
        coords.lat, coords.lon
    );

    let construction = if let Some(auth) = auth {
        let auth = auth.as_ref();
        let prefix_url = &auth.nominatim_url;
        let url = format!("{prefix_url}{arguments}");
        throttled_get(&url)
            .await
            .basic_auth(
                auth.user
                    .clone(),
                Some(
                    auth.passwd
                        .clone(),
                ),
            )
    } else {
        let url = format!("{NOMINATIM_URL}{arguments}");
        throttled_get(&url).await
    };

    let resp = construction
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
    s.parse::<f64>()
        .map_err(serde::de::Error::custom)
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct NominatimLocationData {
    pub class: String,

    #[serde(rename = "type")]
    pub location_type: String,
    #[serde(deserialize_with = "de_str_to_f64")]
    pub lat: f64,
    #[serde(deserialize_with = "de_str_to_f64")]
    pub lon: f64,

    #[serde(rename = "name_map")]
    pub _name_map: Option<String>,
    #[serde(rename = "display_name")]
    pub _display_name: String,
    #[serde(rename = "osm_type")]
    pub _osm_type: String,
}

impl NominatimLocationData {
    pub(crate) async fn from_cords<T: Into<Coordinate>>(
        cords: T,
        auth: Option<Arc<AuthToHost>>,
    ) -> crate::Result<Self> {
        let data = fetch_raw_nominatim_data(cords, auth).await?;
        let location: Self = serde_json::from_value(data)?;
        Ok(location)
    }

    pub(crate) const fn as_coordinate(&self) -> Coordinate {
        Coordinate {
            lat: self.lat,
            lon: self.lon,
        }
    }
}

impl From<NominatimLocationData> for Coordinate {
    fn from(value: NominatimLocationData) -> Self {
        value.as_coordinate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{error::ImageFlowError, tests::http::TestServer};

    fn fixture() -> serde_json::Value {
        serde_json::json!({"class": "building", "type": "house", "lat": "-12.5",
            "lon": "9.25", "display_name": "Test house", "osm_type": "way"})
    }

    #[test]
    #[allow(clippy::used_underscore_binding)]
    fn response_parses_string_coordinates_and_optional_name() {
        let data: NominatimLocationData =
            serde_json::from_value(fixture()).expect("parse response");
        assert_eq!(data.class, "building");
        assert_eq!(data.location_type, "house");
        assert!(
            data._name_map
                .is_none()
        );
        let coordinate: Coordinate = data.into();
        assert_eq!(coordinate.lat, -12.5);
        assert_eq!(coordinate.lon, 9.25);
    }

    #[test]
    fn response_rejects_invalid_coordinates_and_missing_required_fields() {
        for value in [
            serde_json::json!("bad"),
            serde_json::json!(12.5),
            serde_json::Value::Null,
        ] {
            let mut response = fixture();
            response["lat"] = value;
            assert!(serde_json::from_value::<NominatimLocationData>(response).is_err());
        }
        let mut response = fixture();
        response
            .as_object_mut()
            .expect("fixture object")
            .remove("class");
        assert!(serde_json::from_value::<NominatimLocationData>(response).is_err());
    }

    #[tokio::test]
    async fn reverse_request_includes_coordinates_options_authentication_and_headers() {
        let mut server = TestServer::new("200 OK", &fixture().to_string()).await;
        let data = NominatimLocationData::from_cords(
            Coordinate {
                lat: -12.5,
                lon: 9.25,
            },
            Some(server.auth()),
        )
        .await
        .expect("local reverse lookup");
        assert_eq!(data.lat, -12.5);
        let request = server
            .request()
            .await
            .to_lowercase();
        assert!(request.starts_with("get /reverse?format=json&lat=-12.5&lon=9.25&zoom=18&addressdetails=1&extratags=1&namedetails=1 http/1.1"));
        assert!(request.contains("authorization: basic dgvzddpzzwnyzxq="));
        assert!(request.contains("user-agent:"));
        assert!(request.contains("referer:"));
    }

    #[tokio::test]
    async fn reverse_request_rejects_error_status_and_invalid_json() {
        for (status, body) in [
            ("503 Service Unavailable", "{}"),
            ("200 OK", "invalid JSON"),
        ] {
            let mut server = TestServer::new(status, body).await;
            let error =
                fetch_raw_nominatim_data(Coordinate { lat: 0.0, lon: 0.0 }, Some(server.auth()))
                    .await
                    .expect_err("invalid response");
            assert!(matches!(
                ImageFlowError::from(error),
                ImageFlowError::Request { .. }
            ));
            server
                .request()
                .await;
        }
    }
}
