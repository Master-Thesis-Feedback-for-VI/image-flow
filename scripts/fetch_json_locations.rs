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
        let raw = fetch_raw_nominatim_data(v, auth)
            .await
            .unwrap();
        data.push(annotate_location(raw, k, v).expect("Nominatim must return an object"));
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

fn annotate_location(
    mut raw: serde_json::Value,
    name: &str,
    coordinate: Coordinate,
) -> Result<serde_json::Value, &'static str> {
    let object = raw
        .as_object_mut()
        .ok_or("Nominatim response is not an object")?;
    object.insert("name_map".into(), name.into());
    object.insert(
        "url_osm".into(),
        coordinate
            .print_location_url()
            .into(),
    );
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotation_preserves_response_fields_and_adds_name_and_location_link() {
        let coordinate = Coordinate {
            lat: -12.5,
            lon: 9.25,
        };
        let response = json!({"class": "highway", "type": "primary", "name_map": "old"});
        let annotated =
            annotate_location(response, "Test road", coordinate).expect("annotate response");
        assert_eq!(annotated["class"], "highway");
        assert_eq!(annotated["type"], "primary");
        assert_eq!(annotated["name_map"], "Test road");
        assert_eq!(annotated["url_osm"], coordinate.print_location_url());
    }

    #[test]
    fn annotation_rejects_non_object_responses() {
        for response in [json!(null), json!([]), json!("error")] {
            assert!(
                annotate_location(response, "Test", Coordinate { lat: 0.0, lon: 0.0 }).is_err()
            );
        }
    }
}
