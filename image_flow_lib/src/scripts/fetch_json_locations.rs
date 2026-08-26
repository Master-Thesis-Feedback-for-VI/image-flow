use std::{collections::HashMap, io::ErrorKind};

use image_flow::{
    location::coordinate_system::Coordinate, requesters::nominatim::fetch_raw_nominatim_data,
};
use serde_json::json;
use tokio::fs;

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

    let mut data = Vec::new();
    for (k, v) in places {
        let mut raw = fetch_raw_nominatim_data(v)
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

    fs::create_dir_all("./out")
        .await
        .unwrap();

    let out_path = "./out/nominatim_raw_fetch.json";
    match fs::remove_file(out_path).await {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => panic!("failed to remove {out_path}: {e}"),
    }

    let pretty = serde_json::to_string_pretty(&data_write).unwrap();
    fs::write(out_path, pretty)
        .await
        .unwrap();
}
