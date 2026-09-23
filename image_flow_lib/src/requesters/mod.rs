pub(crate) mod client;
pub mod nominatim;
pub(crate) mod overpass;

#[derive(Debug, Clone, uniffi::Record)]
pub struct AuthToHost {
    pub overpass_url: String,
    pub nominatim_url: String,
    pub user: String,
    pub passwd: String,
}
