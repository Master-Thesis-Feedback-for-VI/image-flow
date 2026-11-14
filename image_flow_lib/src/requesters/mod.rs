//! Map-service configuration and raw Nominatim reverse-geocoding requests.
//!
//! High-level tag classification logs map failures and continues with remaining
//! evidence. [`nominatim::fetch_raw_nominatim_data`] returns request errors
//! directly. Custom Overpass configuration still permits public fallback.

pub(crate) mod client;
pub mod nominatim;
pub(crate) mod overpass;

/// Custom map endpoints and HTTP Basic authentication credentials.
///
/// Pass this at runtime to evaluation functions. Nominatim uses the supplied
/// base URL; Overpass tries the custom endpoint first, then public fallbacks
/// (with repeated custom candidates in the current retry list). Authentication
/// is attached only to the first Overpass attempt.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AuthToHost {
    /// Full Overpass interpreter URL, including the endpoint path.
    pub overpass_url: String,
    /// Nominatim base URL without a trailing slash; `/reverse?...` is appended.
    pub nominatim_url: String,
    /// Username for HTTP Basic authentication.
    pub user: String,
    /// Password for HTTP Basic authentication.
    pub passwd: String,
}
