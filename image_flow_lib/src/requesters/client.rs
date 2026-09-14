use std::{sync::LazyLock, time::Duration};

use reqwest::{
    Client,
    header::{self, HeaderMap},
};
use tokio::{
    sync::Mutex,
    time::{Instant, sleep},
};

pub const HTTP_REFERER: &str = match option_env!("IMAGE_FLOW_REFERER") {
    Some(referer) => referer,
    None => "https://github.com/Master-Thesis-Feedback-for-VI/image-flow",
};

pub const USER_AGENT: &str = match option_env!("IMAGE_FLOW_AGENT") {
    Some(referer) => referer,
    None => "image_flow/0.2",
};

static CLIENT: LazyLock<Client> = LazyLock::new(|| {
    let headers = default_headers();
    Client::builder()
        .default_headers(headers)
        .build()
        .expect("client must be build")
});
static LAST_REQUEST: Mutex<Option<Instant>> = Mutex::const_new(None);
const MIN_INTERVAL: Duration = Duration::from_millis(1_000);

// fn init_client() -> Client {
//     let headers = default_headers();
//     Client::builder()
//         .default_headers(headers)
//         .build()
//         .expect("client must be build")
// }

pub fn default_headers() -> HeaderMap {
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

pub fn get_client() -> &'static Client {
    &CLIENT
}

#[must_use]
pub async fn throttle() {
    let mut last = LAST_REQUEST
        .lock()
        .await;
    if let Some(prev) = *last {
        let elapsed = prev.elapsed();
        if elapsed < MIN_INTERVAL {
            sleep(
                MIN_INTERVAL
                    .checked_sub(elapsed)
                    .expect(
                        "the time can not be less if this EVER happens we should talk about it",
                    ),
            )
            .await;
        }
    }
    *last = Some(Instant::now());
}

pub async fn throttled_get(url: &str) -> reqwest::RequestBuilder {
    let () = throttle().await;

    get_client().get(url)
}

pub async fn throttled_post(url: &str) -> reqwest::RequestBuilder {
    let () = throttle().await;

    get_client().post(url)
}
