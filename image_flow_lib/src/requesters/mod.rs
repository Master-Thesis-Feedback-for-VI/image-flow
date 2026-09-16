pub(crate) mod client;
pub mod nominatim;
pub(crate) mod overpass;

#[derive(Debug, Clone, uniffi::Record)]
pub struct AuthToHost {
    url: String,
    user: String,
    passwd: String,
}
