pub(crate) mod client;
pub mod nominatim;
pub(crate) mod overpass;

#[derive(Debug, Clone, uniffi::Record)]
pub struct AuthToHost {
    pub url: String,
    pub user: String,
    pub passwd: String,
}
