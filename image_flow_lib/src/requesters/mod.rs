use reqwest::{
    Client, ClientBuilder,
    header::{self, HeaderMap},
};

use crate::error;

pub mod nominatim;
pub(crate) mod overpass;

pub(crate) const HTTP_REFERER: &str = match option_env!("IMAGE_FLOW_REFERER") {
    Some(referer) => referer,
    None => "https://github.com/Master-Thesis-Feedback-for-VI/image-flow",
};

pub(crate) const USER_AGENT: &str = match option_env!("IMAGE_FLOW_AGENT") {
    Some(referer) => referer,
    None => "image_flow/0.1",
};

pub(crate) fn default_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::REFERER,
        header::HeaderValue::from_static(HTTP_REFERER),
    );
    headers.insert(
        header::USER_AGENT,
        header::HeaderValue::from_static(USER_AGENT),
    );

    headers
}

pub(crate) fn build_reqwest_client_with_default_headers() -> ClientBuilder {
    let headers = default_headers();
    Client::builder().default_headers(headers)
}

pub(crate) async fn fetch_raw_data(url: &str) -> crate::Result<serde_json::Value> {
    let client = build_reqwest_client_with_default_headers();
    let resp = client
        .build()
        .map_err(error::request)?
        .get(url)
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
