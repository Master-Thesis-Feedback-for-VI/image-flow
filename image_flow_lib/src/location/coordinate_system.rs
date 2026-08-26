use std::fmt::Display;

#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct Coordinate {
    pub lat: f64,
    pub lon: f64,
}

impl Coordinate {
    /// This is pure for Debugging purposes
    #[allow(dead_code)]
    #[must_use]
    pub fn print_location_url(self) -> String {
        format!(
            "https://www.openstreetmap.org/?mlat={}&mlon={}#map=16/{}/{}",
            self.lat, self.lon, self.lat, self.lon
        )
    }
}

impl Coordinate {
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
