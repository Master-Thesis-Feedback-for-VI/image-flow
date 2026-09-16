use chrono::Duration;
use clap::Parser;
use image_flow::{
    context::{
        UniqueGeographicCategories,
        image_based_context::{ImageRecognizer, ModelVariant::Medium},
    },
    location::{coordinate_system::Coordinate, tabular_gps::DataPointsGPS},
    video_slicer::ImageGPSZip,
};
use serde::Serialize;
use std::{fs::File, io::BufWriter, path::PathBuf};
use tempfile::tempdir;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long)]
    vid: PathBuf,
    #[arg(short, long)]
    csv: PathBuf,
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(short, long)]
    serialized_output: PathBuf,
}

#[derive(Serialize, Clone, Debug)]
struct CategoryTime {
    category: UniqueGeographicCategories,
    time_diff: Duration,
    location: Coordinate,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("info"))
        .init();

    let args: Args = Args::parse();
    if !args
        .vid
        .exists()
        || !args
            .csv
            .exists()
    {
        panic!("Path does not exist")
    }

    if args
        .serialized_output
        .is_file()
    {
        panic!("File already exists")
    }

    let tmp_dir = tempdir()?;
    let tmp_dir_path = tmp_dir
        .path()
        .to_path_buf();
    let dir_path = args
        .output
        .unwrap_or(tmp_dir_path);
    let model = ImageRecognizer::new_impl(Medium, &dir_path).await?;

    let file = File::open(args.csv)?;
    let datapoints = DataPointsGPS::from_csv(file)?;
    let zipped =
        ImageGPSZip::from_video_and_gps(args.vid, &datapoints, &dir_path, Duration::seconds(1))
            .await?;

    let mut unique_geo_cat_collection = Vec::new();
    for (location, frame) in zipped.iter() {
        let evaluation = image_flow::evaluate_context_impl(*location, frame, &model).await;

        match evaluation {
            Ok(eva) => {
                info!("{eva:#?}");
                let combined = CategoryTime {
                    category: eva,
                    time_diff: frame.time_offset_from_first_frame,
                    location: *location,
                };
                unique_geo_cat_collection.push(combined);
            }
            Err(e) => {
                tracing::error!("{e}");
            }
        }
    }

    let file = File::create(&args.serialized_output)?;
    let write = BufWriter::new(file);
    serde_json::to_writer_pretty(write, &unique_geo_cat_collection)?;

    Ok(())
}
