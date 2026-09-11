use chrono::Duration;
use clap::Parser;
use image_flow::{location::tabular_gps::DataPointsGPS, video_slicer::ImageGPSZip};
use std::{fs::File, path::PathBuf};
use tempfile::tempdir;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long)]
    vid: PathBuf,
    #[arg(short, long)]
    csv: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("error"))
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

    let tmp_directory = tempdir()?;

    let file = File::open(args.csv)?;
    let datapoints = DataPointsGPS::from_csv(file)?;
    let zipped = ImageGPSZip::from_video_and_gps(
        args.vid,
        &datapoints,
        tmp_directory.path(),
        Duration::seconds(1),
    )
    .await?;

    for (img, frame) in zipped.iter() {
        let evaluation = image_flow::evaluate_context_impl(*img, frame).await;

        match evaluation {
            Ok(eva) => println!("{eva:#?}"),
            Err(e) => tracing::error!("{e}"),
        }
    }

    Ok(())
}
