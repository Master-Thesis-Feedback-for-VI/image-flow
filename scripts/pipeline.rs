use clap::Parser;
use image_flow::{
    ImageFlowEvaluationForRecording,
    context::image_based_context::{ImageRecognizer, ModelVariant},
    enable_tracing, image_flow_evaluation_impl,
    location::tabular_gps::DataPointsGPS,
    requesters::AuthToHost,
    video_slicer::ImageGPSZip,
};
use std::io::Read;
use std::io::Write;
use std::{
    fs::{self, File},
    io::BufWriter,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
    time::Duration,
};
use tempfile::tempdir;
use tracing::info;
use walkdir::WalkDir;
use zip::CompressionMethod;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

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
    let model = ImageRecognizer::new_impl(ModelVariant::Large, &dir_path).await?;

    let file = File::open(args.csv)?;
    let datapoints = DataPointsGPS::from_csv(file)?;

    println!("Processing Video...");
    let zipped = ImageGPSZip::from_video_and_gps(
        &args.vid,
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

    let content_file = args
        .serialized_output
        .join("content.json");
    let file = File::create(content_file)?;
    let write = BufWriter::new(file);
    serde_json::to_writer_pretty(write, &unique_geo_cat_collection)?;

    let movie_name = args
        .serialized_output
        .join("recording.mov");
    fs::copy(&args.vid, movie_name)?;

    let dst_file = args
        .serialized_output
        .file_name()
        .and_then(|s| s.to_str())
        .expect("this must work");
    zip_folder(
        &args.serialized_output,
        args.serialized_output
            .join("..")
            .join(format!("{dst_file}.zip")),
    )
    .expect("Could not zip folder");

    Ok(())
}

fn zip_folder(
    src_path: impl AsRef<Path>,
    dst_file: impl AsRef<Path>,
) -> zip::result::ZipResult<()> {
    let output = File::create(dst_file)?;
    let mut zip = ZipWriter::new(output);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(9))
        .unix_permissions(0o755);

    let mut buffer = Vec::new();
    for entry in WalkDir::new(&src_path) {
        let entry = entry.map_err(std::io::Error::from)?;
        let path = entry.path();
        let name = path
            .strip_prefix(&src_path)
            .expect("this should not fail");

        if path.is_file() {
            zip.start_file(name.to_string_lossy(), options)?;
            let mut f = File::open(path)?;
            f.read_to_end(&mut buffer)?;
            zip.write_all(&buffer)?;
            buffer.clear();
        } else if !name
            .as_os_str()
            .is_empty()
        {
            // Preserve empty directories.
            zip.add_directory(name.to_string_lossy(), options)?;
        }
    }

    zip.finish()?;
    Ok(())
}
