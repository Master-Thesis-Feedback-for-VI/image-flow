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

    let evaluation = image_flow::evaluate_context_impl(gps_data, args.img).await?;

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
