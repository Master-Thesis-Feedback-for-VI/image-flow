use clap::Parser;
use image_flow::{
    ImageFlowEvaluationForRecording,
    context::image_based_context::{ImageRecognizer, ModelVariant::Medium},
    enable_tracing, image_flow_evaluation_impl,
    location::tabular_gps::DataPointsGPS,
    requesters::AuthToHost,
    video_slicer::ImageGPSZip,
};
use std::{
    fs::File,
    io::BufWriter,
    path::PathBuf,
    sync::{Arc, LazyLock},
    time::Duration,
};
use tempfile::tempdir;
use tracing::info;

pub static AUTH_AGENT: LazyLock<Option<AuthToHost>> = LazyLock::new(|| {
    let user_name = option_env!("USER_AGENT_NAME");
    let passwd = option_env!("PASSWORD");
    let url = option_env!("OVERPASS_URL");

    if let Some(passwd) = passwd
        && let Some(user) = user_name
        && let Some(url) = url
    {
        Some(AuthToHost {
            overpass_url: url.to_string(),
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = enable_tracing(Some(image_flow::FilterLevel::Error));

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

    let mut unique_geo_cat_collection: Vec<ImageFlowEvaluationForRecording> = Vec::new();
    for (location, frame) in zipped.iter() {
        let auth = AUTH_AGENT.clone();
        let evaluation = image_flow_evaluation_impl(*location, auth, frame, &model, 4).await;

        match evaluation {
            Ok(eva) => {
                let time_diff: Duration = frame
                    .time_offset_from_first_frame
                    .to_std()
                    .expect("It failed?");
                info!("it was evaluated to: {eva:#?}");
                let evaluated =
                    ImageFlowEvaluationForRecording::new(Arc::new(eva), time_diff, *location);

                println!("This data was captured:\n{evaluated:#?}");
                unique_geo_cat_collection.push(evaluated);
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
