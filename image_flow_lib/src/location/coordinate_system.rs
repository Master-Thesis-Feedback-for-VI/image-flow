//! Latitude/longitude coordinates shared by map requests and GPS recordings.

use std::fmt::Display;

use serde::{Deserialize, Serialize};

/// Geographic latitude and longitude in decimal degrees.
///
/// Values are stored as supplied; construction and deserialization do not
/// validate coordinate ranges or reject non-finite values.
#[derive(Debug, Clone, Copy, uniffi::Record, Serialize, Deserialize)]
pub struct Coordinate {
    /// Latitude in degrees, normally in `-90.0..=90.0`.
    pub lat: f64,
    /// Longitude in degrees, normally in `-180.0..=180.0`.
    pub lon: f64,
}

impl Coordinate {
    /// Return an OpenStreetMap link centered on this coordinate for debugging.
    ///
    /// Despite the method name, this returns a string and does not print it.
    #[allow(dead_code)]
    #[must_use]
    pub fn print_location_url(self) -> String {
        format!(
            "https://www.openstreetmap.org/?mlat={}&mlon={}#map=16/{}/{}",
            self.lat, self.lon, self.lat, self.lon
        )
    }

    #[allow(dead_code)]
    pub(crate) fn as_url_osm(&self) -> String {
        let lat = self.lat;
        let lon = self.lon;
        format!("https://www.openstreetmap.org/?mlat={lat}&mlon={lon}#map=17/{lat}/{lon}")
    }
}

impl Display for Coordinate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "(lat = {}, lon = {})", self.lat, self.lon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_and_urls_preserve_signed_coordinates() {
        let coordinate = Coordinate {
            lat: -12.5,
            lon: 9.25,
        };
        assert_eq!(coordinate.to_string(), "(lat = -12.5, lon = 9.25)");
        assert_eq!(
            coordinate.print_location_url(),
            "https://www.openstreetmap.org/?mlat=-12.5&mlon=9.25#map=16/-12.5/9.25"
        );
        assert_eq!(
            coordinate.as_url_osm(),
            "https://www.openstreetmap.org/?mlat=-12.5&mlon=9.25#map=17/-12.5/9.25"
        );
    }

    #[test]
    fn json_round_trip_and_missing_field_rejection() {
        let coordinate = Coordinate {
            lat: 90.0,
            lon: -180.0,
        };
        let restored: Coordinate =
            serde_json::from_value(serde_json::to_value(coordinate).expect("serialize coordinate"))
                .expect("deserialize coordinate");
        assert_eq!(restored.lat, coordinate.lat);
        assert_eq!(restored.lon, coordinate.lon);
        assert!(serde_json::from_str::<Coordinate>(r#"{"lat":1}"#).is_err());
    }
}
