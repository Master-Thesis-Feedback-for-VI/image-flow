use std::{
    collections::HashMap,
    io::ErrorKind,
    path::Path,
    sync::{Arc, LazyLock},
};

use image_flow::{
    location::coordinate_system::Coordinate,
    requesters::{AuthToHost, nominatim::fetch_raw_nominatim_data},
};
use serde_json::json;
use tokio::fs;

pub static AUTH_AGENT: LazyLock<Option<AuthToHost>> = LazyLock::new(|| {
    let user_name = option_env!("USER_AGENT_NAME");
    let passwd = option_env!("PASSWORD");
    let overpass_url = option_env!("OVERPASS_URL");
    let nominatim_url = option_env!("NOMINATIM_URL");

    if let Some(passwd) = passwd
        && let Some(user) = user_name
        && let Some(overpass_url) = overpass_url
        && let Some(nominatim_url) = nominatim_url
    {
        Some(AuthToHost {
            overpass_url: overpass_url.to_string(),
            user: user.to_string(),
            passwd: passwd.to_string(),
            nominatim_url: nominatim_url.to_string(),
        })
    } else {
        None
    }
});

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[tokio::main]
pub(crate) async fn main() {
    let places = HashMap::from([
        (
            "Karl-Wiechert-Allee",
            Coordinate {
                lat: 52.38602,
                lon: 9.80881,
            },
        ),
        (
            "Im Maschsee",
            Coordinate {
                lat: 52.36241,
                lon: 9.73834,
            },
        ),
        (
            "Steintor",
            Coordinate {
                lat: 52.37592,
                lon: 9.73218,
            },
        ),
        (
            "Eilenriede",
            Coordinate {
                lat: 52.38305,
                lon: 9.78945,
            },
        ),
    ]);

    let auth = AUTH_AGENT
        .clone()
        .map(Arc::new);
    let mut data = Vec::new();
    for (k, v) in places {
        let auth = auth
            .as_ref()
            .map(Arc::clone);
        let mut raw = fetch_raw_nominatim_data(v, auth)
            .await
            .unwrap();
        raw.as_object_mut()
            .unwrap()
            .insert("name_map".to_string(), k.into());
        raw.as_object_mut()
            .unwrap()
            .insert(
                "url_osm".to_string(),
                v.print_location_url()
                    .into(),
            );
        data.push(raw);
    }
    let data_write = json!(data);

    let out_dir = Path::new(".").join("out");
    fs::create_dir_all(&out_dir)
        .await
        .unwrap();

    let out_path = out_dir.join("nominatim_raw_fetch.json");
    match fs::remove_file(&out_path).await {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => panic!("failed to remove {}: {e}", out_path.display()),
    }

    let pretty = serde_json::to_string_pretty(&data_write).unwrap();
    fs::write(out_path, pretty)
        .await
        .unwrap();
}
