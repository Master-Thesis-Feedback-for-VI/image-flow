use std::{fs::File, path::PathBuf};

use clap::Parser;
use image_flow::location::tabular_gps::DataPointsGPS;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long)]
    img: PathBuf,

    #[arg(short, long)]
    gps: PathBuf,
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("info"))
        .init();

    let args = Args::parse();

    assert!(check_validity(&args), "Path does not exist");
    let file = File::open(args.gps)?;
    let data_points = DataPointsGPS::from_csv(file)?;
    let gps_data = *(*data_points)
        .first()
        .expect("This must contain something!");

    let evaluation =
        image_flow::recognize_location_tags_impl(gps_data.coordinates(), None, args.img, 4).await?;

    println!("{evaluation:#?}");

    Ok(())
}

fn check_validity(args: &Args) -> bool {
    args.img
        .exists()
        && args
            .gps
            .exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_requires_image_and_gps_paths() {
        assert!(Args::try_parse_from(["asses_image"]).is_err());
        assert!(Args::try_parse_from(["asses_image", "-i", "image.png"]).is_err());
        for arguments in [
            ["asses_image", "-i", "image.png", "-g", "gps.csv"],
            ["asses_image", "--img", "image.png", "--gps", "gps.csv"],
        ] {
            let args = Args::try_parse_from(arguments).expect("parse arguments");
            assert_eq!(args.img, PathBuf::from("image.png"));
            assert_eq!(args.gps, PathBuf::from("gps.csv"));
        }
    }

    #[test]
    fn path_validation_requires_both_paths_to_exist() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let img = dir
            .path()
            .join("image.png");
        let gps = dir
            .path()
            .join("gps.csv");
        let args = Args {
            img: img.clone(),
            gps: gps.clone(),
        };
        assert!(!check_validity(&args));
        std::fs::write(&img, b"fixture").expect("write image fixture");
        assert!(!check_validity(&args));
        std::fs::write(&gps, b"fixture").expect("write GPS fixture");
        assert!(check_validity(&args));
    }
}
