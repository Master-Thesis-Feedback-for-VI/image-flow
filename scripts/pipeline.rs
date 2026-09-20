use clap::Parser;
use image_flow::{
    context::{
        UniqueLocationTags,
        image_based_context::{ImageRecognizer, ModelVariant::Medium},
    },
    location::{coordinate_system::Coordinate, tabular_gps::DataPointsGPS},
    requesters::AuthToHost,
    video_slicer::ImageGPSZip,
};
use serde::Serialize;
use std::{
    fs::File,
    io::BufWriter,
    path::PathBuf,
    sync::{Arc, LazyLock},
    time::Duration,
};
use tempfile::tempdir;
use tracing::info;
use tracing_subscriber::EnvFilter;

pub static AUTH_AGENT: LazyLock<Option<AuthToHost>> = LazyLock::new(|| {
    let user_name = option_env!("USER_AGENT_NAME");
    let passwd = option_env!("PASSWORD");
    let url = option_env!("OVERPASS_URL");

    if let Some(passwd) = passwd
        && let Some(user) = user_name
        && let Some(url) = url
    {
        Some(AuthToHost {
            url: url.to_string(),
            user: user.to_string(),
            passwd: passwd.to_string(),
        })
    } else {
        None
    }
});

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
    categories: UniqueLocationTags,
    time_diff: Duration,
    location: Coordinate,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("error"))
        .init();

    let auth_agent = AUTH_AGENT
        .clone()
        .map(Arc::new);

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

    println!("Processing Video...");
    let zipped = ImageGPSZip::from_video_and_gps(
        args.vid,
        &datapoints,
        &dir_path,
        chrono::Duration::seconds(1),
    )
    .await?;
    println!("... finished now proceeding to data tagging");

    let mut unique_geo_cat_collection = Vec::new();
    for (location, frame) in zipped.iter() {
        let evaluation =
            image_flow::evaluate_context_impl(*location, frame, &model, auth_agent.clone()).await;

        match evaluation {
            Ok(eva) => {
                let time_diff: Duration = frame
                    .time_offset_from_first_frame
                    .to_std()
                    .expect("It failed?");
                info!("it was evaluated to: {eva:#?}");
                let combined = CategoryTime {
                    categories: eva,
                    time_diff,
                    location: *location,
                };
                println!("This data was captured:\n{combined:#?}");
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
