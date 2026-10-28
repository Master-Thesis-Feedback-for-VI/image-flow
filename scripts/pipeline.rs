use clap::Parser;
use image_flow::{
    ImageFlowEvaluationForRecording,
    context::image_based_context::{ImageRecognizer, ModelVariant},
    enable_tracing, image_flow_evaluation_impl,
    location::tabular_gps::DataPointsGPS,
    requesters::AuthToHost,
    video_slicer::ImageGPSZip,
};
use indicatif::{ProgressBar, ProgressStyle};
use std::io::Read;
use std::io::Write;
use std::{
    fs::{self, File, OpenOptions},
    io::BufWriter,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, LazyLock},
    time::Duration,
};
use tempfile::tempdir;
use tracing::info;
use walkdir::WalkDir;
use zip::CompressionMethod;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

const MODEL_SIZE: ModelVariant = ModelVariant::Large;

const FRAME_DIFF_DURATION_IN_MILIS: i64 = 400;
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
    input: PathBuf,
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(short, long)]
    serialized_output: PathBuf,
    #[arg(long, default_value = "pipeline-errors.log")]
    error_log: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = enable_tracing(Some(image_flow::FilterLevel::Error));
    ultralytics_inference::logging::set_verbose(false);

    let args: Args = Args::parse();
    let inputs = recording_inputs(&args.input)?;
    if let Some(parent) = args
        .error_log
        .parent()
        .filter(|path| {
            !path
                .as_os_str()
                .is_empty()
        })
    {
        fs::create_dir_all(parent)?;
    }
    let mut error_log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&args.error_log)?;
    let batch = args
        .input
        .is_dir();
    let model_dir = tempdir()?;
    let model_path = args
        .output
        .as_deref()
        .unwrap_or_else(|| model_dir.path());
    fs::create_dir_all(model_path)?;
    let model = ImageRecognizer::new_impl(MODEL_SIZE, model_path).await?;
    let mut failures = 0;
    for (index, input) in inputs
        .iter()
        .enumerate()
    {
        let job = recording_args(&args, input, batch);
        println!(
            "Processing archive {}/{}: {}",
            index + 1,
            inputs.len(),
            input.display()
        );
        if let Err(error) = process_recording(&job, &model, &mut error_log).await {
            eprintln!("Failed to process {}: {error}", input.display());
            log_processing_error(&mut error_log, input, None, error.as_ref())?;
            failures += 1;
        }
    }
    if failures > 0 {
        return Err(std::io::Error::other(format!("{failures} recording(s) failed")).into());
    }
    Ok(())
}

fn recording_inputs(input: &Path) -> std::io::Result<Vec<PathBuf>> {
    if !input.is_dir() {
        fs::metadata(input)?;
        return Ok(vec![input.to_path_buf()]);
    }
    let mut inputs = Vec::new();
    for entry in fs::read_dir(input)? {
        let entry = entry?;
        let path = entry.path();
        if entry
            .file_type()?
            .is_file()
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
        {
            inputs.push(path);
        }
    }
    inputs.sort();
    if inputs.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No ZIP files found in input folder",
        ));
    }
    Ok(inputs)
}

fn recording_args(args: &Args, input: &Path, batch: bool) -> Args {
    let name = input
        .file_stem()
        .expect("input file has a name");
    Args {
        input: input.to_path_buf(),
        error_log: args
            .error_log
            .clone(),
        output: args
            .output
            .as_ref()
            .map(|path| if batch { path.join(name) } else { path.clone() }),
        serialized_output: if batch {
            args.serialized_output
                .join(name)
        } else {
            args.serialized_output
                .clone()
        },
    }
}

async fn process_recording(
    args: &Args,
    model: &ImageRecognizer,
    error_log: &mut File,
) -> Result<(), Box<dyn std::error::Error>> {
    if args
        .serialized_output
        .is_file()
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "Result path is already a file",
        )
        .into());
    }

    let tmp_dir = tempdir()?;
    let (video, csv) = extract_recording(&args.input, tmp_dir.path())?;
    let tmp_dir_path = tmp_dir
        .path()
        .to_path_buf();
    let dir_path = args
        .output
        .clone()
        .unwrap_or(tmp_dir_path);
    fs::create_dir_all(&dir_path)?;

    let file = File::open(csv)?;
    let datapoints = DataPointsGPS::from_csv(file)?;

    println!("Processing Video...");
    let zipped = ImageGPSZip::from_video_and_gps(
        &video,
        &datapoints,
        &dir_path,
        chrono::Duration::milliseconds(FRAME_DIFF_DURATION_IN_MILIS),
    )
    .await?;
    println!("... finished now proceeding to data tagging");

    let progress = ProgressBar::new(zipped.len() as u64);
    progress.set_style(ProgressStyle::with_template(
        "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} images | ETA {eta} {msg}",
    )?);
    progress.enable_steady_tick(Duration::from_millis(100));

    let mut unique_geo_cat_collection: Vec<ImageFlowEvaluationForRecording> = Vec::new();
    for (index, (location, frame)) in zipped
        .iter()
        .enumerate()
    {
        let auth = AUTH_AGENT.clone();
        let evaluation = image_flow_evaluation_impl(*location, auth, frame, model, 2).await;

        match evaluation {
            Ok(eva) => {
                let time_diff: Duration = frame
                    .time_offset_from_first_frame
                    .to_std()
                    .expect("It failed?");
                info!("it was evaluated to: {eva:#?}");
                let evaluated =
                    ImageFlowEvaluationForRecording::new(Arc::new(eva), time_diff, *location);

                unique_geo_cat_collection.push(evaluated);
            }
            Err(e) => {
                tracing::error!("{e}");
                log_processing_error(
                    error_log,
                    &args.input,
                    Some((
                        index + 1,
                        frame
                            .time_offset_from_first_frame
                            .num_milliseconds(),
                    )),
                    &e,
                )?;
            }
        }
        progress.inc(1);
    }
    progress.finish_with_message("Tagging complete");

    fs::create_dir_all(&args.serialized_output)?;
    let content_file = args
        .serialized_output
        .join("content.json");
    let file = File::create(content_file)?;
    let write = BufWriter::new(file);
    serde_json::to_writer_pretty(write, &unique_geo_cat_collection)?;

    let movie_name = args
        .serialized_output
        .join("recording.mov");
    export_quieter_recording(&video, &movie_name)?;

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
    )?;

    Ok(())
}

fn export_quieter_recording(input: &Path, output: &Path) -> std::io::Result<()> {
    let result = Command::new("ffmpeg")
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(input)
        .args([
            "-map",
            "0",
            "-c",
            "copy",
            "-c:a",
            "aac",
            "-filter:a",
            "volume=0.8",
        ])
        .arg(output)
        .output()
        .map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!("Failed to run ffmpeg for audio export: {error}"),
            )
        })?;
    if !result
        .status
        .success()
    {
        return Err(std::io::Error::other(format!(
            "Failed to export quieter recording ({}): {}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        )));
    }
    Ok(())
}

fn log_processing_error(
    writer: &mut impl Write,
    input: &Path,
    frame: Option<(usize, i64)>,
    error: &dyn std::error::Error,
) -> std::io::Result<()> {
    let entry = serde_json::json!({
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "archive": input,
        "frame": frame.map(|(index, _)| index),
        "offset_ms": frame.map(|(_, offset)| offset),
        "error": format!("{error:?}"),
    });
    writeln!(writer, "{entry}")?;
    writer.flush()
}

fn extract_recording(
    input: &Path,
    destination: &Path,
) -> Result<(PathBuf, PathBuf), Box<dyn std::error::Error>> {
    let mut archive = zip::ZipArchive::new(File::open(input)?)?;
    let mut videos = Vec::new();
    let mut csvs = Vec::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let path = entry
            .enclosed_name()
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "Unsafe path in input ZIP")
            })?;
        if entry.is_dir() || path.starts_with("__MACOSX") {
            continue;
        }
        if path
            .file_name()
            .is_some_and(|name| name == "gps_timestamps.csv")
        {
            csvs.push((index, path));
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("mov"))
        {
            videos.push((index, path));
        }
    }

    let ([(video_index, video_path)], [(csv_index, csv_path)]) =
        (videos.as_slice(), csvs.as_slice())
    else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Input ZIP must contain exactly one .mov file and one gps_timestamps.csv",
        )
        .into());
    };
    if video_path.parent() != csv_path.parent() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "The .mov file and gps_timestamps.csv must be in the same recording folder",
        )
        .into());
    }

    let video = destination.join("recording.mov");
    let csv = destination.join("gps_timestamps.csv");
    for (index, path) in [(*video_index, &video), (*csv_index, &csv)] {
        let mut entry = archive.by_index(index)?;
        std::io::copy(&mut entry, &mut File::create(path)?)?;
    }
    Ok((video, csv))
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
            zip.add_directory(name.to_string_lossy(), options)?;
        }
    }

    zip.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exported_recording_is_quieter_and_supports_silent_video() {
        let dir = tempdir().expect("temporary directory");
        for with_audio in [true, false] {
            let input = dir
                .path()
                .join("input.mov");
            let output = dir
                .path()
                .join("output.mov");
            let mut generate = Command::new("ffmpeg");
            generate.args([
                "-nostdin",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=size=16x16:rate=10:duration=1",
            ]);
            if with_audio {
                generate.args([
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:duration=1",
                    "-c:a",
                    "pcm_s16le",
                ]);
            }
            let result = generate
                .args(["-c:v", "mpeg4"])
                .arg(&input)
                .output()
                .expect("ffmpeg must be installed");
            assert!(
                result
                    .status
                    .success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            let original = fs::read(&input).expect("original video");
            export_quieter_recording(&input, &output).expect("export recording");
            assert_eq!(fs::read(&input).expect("unchanged input"), original);

            if with_audio {
                let rms = |path: &Path| {
                    let decoded = Command::new("ffmpeg")
                        .args(["-nostdin", "-loglevel", "error", "-i"])
                        .arg(path)
                        .args(["-vn", "-f", "f32le", "-c:a", "pcm_f32le", "-"])
                        .output()
                        .expect("decode audio");
                    assert!(
                        decoded
                            .status
                            .success()
                    );
                    let samples: Vec<f64> = decoded
                        .stdout
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|bytes| f64::from(f32::from_le_bytes(*bytes)))
                        .collect();
                    (samples
                        .iter()
                        .map(|sample| sample * sample)
                        .sum::<f64>()
                        / f64::from(u32::try_from(samples.len()).expect("short test audio")))
                    .sqrt()
                };
                let ratio = rms(&output) / rms(&input);
                assert!((ratio - 0.8).abs() < 0.03, "audio amplitude ratio: {ratio}");
            }
            assert!(
                fs::metadata(&output)
                    .expect("exported video")
                    .len()
                    > 0
            );
        }
    }

    #[test]
    fn error_log_option_has_default_and_accepts_custom_path() {
        let args = Args::try_parse_from(["pipeline", "-i", "input.zip", "-s", "results"])
            .expect("default log argument");
        assert_eq!(args.error_log, PathBuf::from("pipeline-errors.log"));
        let args = Args::try_parse_from([
            "pipeline",
            "-i",
            "inputs",
            "-s",
            "results",
            "--error-log",
            "logs/errors.log",
        ])
        .expect("custom log argument");
        assert_eq!(args.error_log, PathBuf::from("logs/errors.log"));
        assert_eq!(
            recording_args(&args, Path::new("inputs/one.zip"), true).error_log,
            args.error_log
        );
    }

    #[test]
    fn error_log_collects_frame_and_recording_errors_as_json_lines() {
        let mut log = Vec::new();
        let error = std::io::Error::other("evaluation failed\nextra detail");
        log_processing_error(&mut log, Path::new("one.zip"), Some((3, 800)), &error)
            .expect("log frame error");
        log_processing_error(&mut log, Path::new("two.zip"), None, &error)
            .expect("log recording error");
        let content = String::from_utf8(log).expect("UTF-8 log");
        let entries: Vec<serde_json::Value> = content
            .lines()
            .map(|line| serde_json::from_str(line).expect("JSON entry"))
            .collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["archive"], "one.zip");
        assert_eq!(entries[0]["frame"], 3);
        assert_eq!(entries[0]["offset_ms"], 800);
        assert!(
            entries[0]["error"]
                .as_str()
                .expect("error details")
                .contains("evaluation failed")
        );
        assert!(entries[0]["timestamp"].is_string());
        assert_eq!(entries[1]["archive"], "two.zip");
        assert!(entries[1]["frame"].is_null());
        assert!(entries[1]["offset_ms"].is_null());
        assert!(
            log_processing_error(
                &mut std::io::sink(),
                Path::new("one.zip"),
                Some((1, 0)),
                &error
            )
            .is_ok()
        );
    }

    #[test]
    fn folder_inputs_are_sorted_zips_only_and_not_recursive() {
        let dir = tempdir().expect("temporary directory");
        for name in ["b.ZIP", "a.zip", "notes.txt"] {
            fs::write(
                dir.path()
                    .join(name),
                "",
            )
            .expect("write file");
        }
        fs::create_dir(
            dir.path()
                .join("nested.zip"),
        )
        .expect("create nested directory");
        fs::write(
            dir.path()
                .join("nested.zip/hidden.zip"),
            "",
        )
        .expect("write nested ZIP");
        assert_eq!(
            recording_inputs(dir.path()).expect("find ZIPs"),
            vec![
                dir.path()
                    .join("a.zip"),
                dir.path()
                    .join("b.ZIP")
            ]
        );
        let single = dir
            .path()
            .join("a.zip");
        assert_eq!(recording_inputs(&single).expect("single ZIP"), vec![single]);
        assert!(
            recording_inputs(
                &dir.path()
                    .join("missing")
            )
            .is_err()
        );
        let empty = tempdir().expect("empty directory");
        assert!(recording_inputs(empty.path()).is_err());
    }

    #[test]
    fn batch_outputs_are_isolated_and_single_outputs_unchanged() {
        let args = Args::try_parse_from([
            "pipeline", "-i", "inputs", "-s", "results", "--output", "frames",
        ])
        .expect("parse args");
        let input = Path::new("inputs/recording.zip");
        let batch = recording_args(&args, input, true);
        assert_eq!(batch.input, input);
        assert_eq!(batch.output, Some(PathBuf::from("frames/recording")));
        assert_eq!(batch.serialized_output, PathBuf::from("results/recording"));
        let single = recording_args(&args, input, false);
        assert_eq!(single.output, args.output);
        assert_eq!(single.serialized_output, args.serialized_output);
        let args = Args {
            output: None,
            ..args
        };
        assert!(
            recording_args(&args, input, true)
                .output
                .is_none()
        );
    }

    #[test]
    fn cli_requires_input_zip_and_serialized_output() {
        assert!(Args::try_parse_from(["pipeline"]).is_err());
        assert!(Args::try_parse_from(["pipeline", "-i", "recording.zip"]).is_err());
        let args = Args::try_parse_from([
            "pipeline",
            "-i",
            "recording.zip",
            "-s",
            "result",
            "--output",
            "frames",
        ])
        .expect("parse complete arguments");
        assert_eq!(args.input, PathBuf::from("recording.zip"));
        assert_eq!(args.serialized_output, PathBuf::from("result"));
        assert_eq!(args.output, Some(PathBuf::from("frames")));
        let args = Args::try_parse_from([
            "pipeline",
            "--input",
            "recording.zip",
            "--serialized-output",
            "result",
        ])
        .expect("parse long arguments");
        assert!(
            args.output
                .is_none()
        );
    }

    #[test]
    fn extracts_recording_folder_from_zip() {
        let dir = tempdir().expect("temporary directory");
        let input = dir
            .path()
            .join("input.zip");
        let mut archive = ZipWriter::new(File::create(&input).expect("create ZIP"));
        for (name, content) in [
            ("recording-2026-08-12-13-24-46/6FC95631.mov", "movie bytes"),
            (
                "recording-2026-08-12-13-24-46/gps_timestamps.csv",
                "gps data",
            ),
            ("__MACOSX/recording/._6FC95631.mov", "metadata"),
        ] {
            archive
                .start_file(name, SimpleFileOptions::default())
                .expect("start member");
            archive
                .write_all(content.as_bytes())
                .expect("write member");
        }
        archive
            .finish()
            .expect("finish ZIP");
        let (video, csv) = extract_recording(&input, dir.path()).expect("extract recording");
        assert_eq!(fs::read(video).expect("read video"), b"movie bytes");
        assert_eq!(fs::read(csv).expect("read CSV"), b"gps data");
    }

    #[test]
    fn rejects_invalid_recording_archives() {
        let dir = tempdir().expect("temporary directory");
        for members in [
            vec!["recording/video.mov"],
            vec!["recording/gps_timestamps.csv"],
            vec!["recording/video.mov", "other/gps_timestamps.csv"],
            vec![
                "recording/video.mov",
                "recording/other.mov",
                "recording/gps_timestamps.csv",
            ],
            vec!["../video.mov", "../gps_timestamps.csv"],
        ] {
            let input = dir
                .path()
                .join("input.zip");
            let mut archive = ZipWriter::new(File::create(&input).expect("create ZIP"));
            for name in members {
                archive
                    .start_file(name, SimpleFileOptions::default())
                    .expect("start member");
            }
            archive
                .finish()
                .expect("finish ZIP");
            assert!(extract_recording(&input, dir.path()).is_err());
        }
        let input = dir
            .path()
            .join("invalid.zip");
        fs::write(&input, "not a ZIP").expect("write invalid ZIP");
        assert!(extract_recording(&input, dir.path()).is_err());
        assert!(
            extract_recording(
                &dir.path()
                    .join("missing.zip"),
                dir.path()
            )
            .is_err()
        );
    }

    #[test]
    fn archive_preserves_nested_files_empty_directories_and_exact_contents() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let source = dir
            .path()
            .join("source");
        fs::create_dir_all(source.join("nested/empty")).expect("create directories");
        fs::write(source.join("content.json"), b"[]").expect("write content");
        fs::write(source.join("nested/recording.mov"), b"movie bytes").expect("write movie");
        fs::write(source.join("zero"), b"").expect("write empty file");
        let destination = dir
            .path()
            .join("result.zip");
        zip_folder(&source, &destination).expect("archive folder");
        let mut archive =
            zip::ZipArchive::new(File::open(destination).expect("open ZIP")).expect("read ZIP");
        assert_eq!(archive.len(), 5);
        for (name, expected) in [
            ("content.json", b"[]".as_slice()),
            ("nested/recording.mov", b"movie bytes".as_slice()),
            ("zero", b"".as_slice()),
        ] {
            let mut file = archive
                .by_name(name)
                .expect("archive member");
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)
                .expect("read member");
            assert_eq!(bytes, expected);
        }
        assert!(
            archive
                .by_name("nested/empty/")
                .expect("empty directory")
                .is_dir()
        );
    }

    #[test]
    fn archive_rejects_missing_source_and_unwritable_destination() {
        let dir = tempfile::tempdir().expect("temporary directory");
        assert!(
            zip_folder(
                dir.path()
                    .join("missing"),
                dir.path()
                    .join("result.zip")
            )
            .is_err()
        );
        assert!(
            zip_folder(
                dir.path(),
                dir.path()
                    .join("missing/result.zip")
            )
            .is_err()
        );
    }
}
