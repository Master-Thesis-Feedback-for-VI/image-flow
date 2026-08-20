use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use ffmpeg_next::{
    self as ffmpeg, codec, decoder,
    format::{self, Pixel},
    frame::Video,
    media::Type::{self},
    software::scaling::{self, Flags},
};
use image::ColorType;
use tokio::{
    fs::{self},
    sync::{OnceCell, mpsc},
};
use tokio_stream::{Stream, wrappers::ReceiverStream};

use crate::error;

static ONCE: OnceCell<bool> = OnceCell::const_new();
const DIR_FRAMES: &str = "frames";

pub(crate) fn export_images_from_video<P>(
    video_source_path: P,
    temporary_storage: impl AsRef<Path>,
) -> impl Stream<Item = crate::Result<PathBuf>>
where
    P: AsRef<Path> + Send + 'static,
{
    let (tx, rx) = mpsc::channel::<crate::Result<PathBuf>>(8);

    let temporary_storage = temporary_storage.as_ref().to_path_buf();
    tokio::spawn(async move {
        ONCE.get_or_init(|| async {
            ffmpeg::init().expect("Ffmpeg could not init");
            true
        })
        .await;

        let relative_dir_storage_path = temporary_storage.join(DIR_FRAMES);
        if let Err(e) = fs::create_dir_all(&relative_dir_storage_path).await {
            let _ = tx.send(Err(e.into())).await;
            return;
        }

        tokio::task::spawn_blocking(move || {
            if let Err(e) = decode_and_send(&video_source_path, &relative_dir_storage_path, &tx) {
                let _ = tx.blocking_send(Err(e));
            }
        });
    });

    ReceiverStream::new(rx)
}

fn decode_and_send(
    video_source_path: impl AsRef<Path>,
    relative_dir_storage_path: impl AsRef<Path>,
    tx: &mpsc::Sender<crate::Result<PathBuf>>,
) -> crate::Result<()> {
    let mut ictx = format::input(&video_source_path)?;
    let input_stream = ictx
        .streams()
        .best(Type::Video)
        .ok_or(ffmpeg::Error::StreamNotFound)?;
    let video_stream_index = input_stream.index();
    let time_base = input_stream.time_base();

    let context_decoder = codec::Context::from_parameters(input_stream.parameters())?;
    let mut decoder = context_decoder.decoder().video()?;

    let mut scaler = scaling::context::Context::get(
        decoder.format(),
        decoder.width(),
        decoder.height(),
        Pixel::RGB24,
        decoder.width(),
        decoder.height(),
        Flags::BILINEAR,
    )?;

    let mut next_second_to_capture = 0i64;

    let mut receive_and_process_decoded_frames =
        |decoder: &mut decoder::Video| -> crate::Result<()> {
            let mut video_decoded = Video::empty();
            while decoder.receive_frame(&mut video_decoded).is_ok() {
                if let Some(pts) = video_decoded.pts() {
                    let lhs = pts * i64::from(time_base.numerator());
                    let rhs = next_second_to_capture * i64::from(time_base.denominator());
                    if lhs >= rhs {
                        let mut rgb_frame = Video::empty();
                        scaler.run(&video_decoded, &mut rgb_frame)?;

                        let frame_path = relative_dir_storage_path
                            .as_ref()
                            .join(format!("{next_second_to_capture:05}s.png"));
                        let _ = save_png(&rgb_frame, &frame_path);

                        // Stop decoding early if the receiver was dropped.
                        if tx.blocking_send(Ok(frame_path)).is_err() {
                            return Ok(());
                        }

                        next_second_to_capture += 1;
                    }
                }
            }
            Ok(())
        };

    for (stream, packet) in ictx.packets() {
        if stream.index() == video_stream_index {
            decoder.send_packet(&packet)?;
            receive_and_process_decoded_frames(&mut decoder)?;
        }
    }
    decoder.send_eof()?;
    receive_and_process_decoded_frames(&mut decoder)?;

    Ok(())
}

fn save_png(frame: &Video, path: impl AsRef<Path>) -> crate::Result<()> {
    image::save_buffer(
        path,
        frame.data(0),
        frame.width(),
        frame.height(),
        ColorType::Rgb8,
    )
    .map_err(error::ffmpeg)
}

struct VideoMetadata {
    name: String,
    duration: Duration,
}

impl VideoMetadata {
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation
    )]
    async fn open<P>(path: P) -> crate::Result<Self>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref().to_path_buf();

        tokio::task::spawn_blocking(move || -> crate::Result<Self> {
            let file_name = path
                .file_name()
                .ok_or_else(|| error::io(String::from("no file name found")))?
                .to_str()
                .ok_or_else(|| error::io(String::from("file name has unsupported encoding")))?
                .to_owned();

            let ictx = format::input(&path)?;
            let duration_seconds_fractional =
                ictx.duration() as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE);
            let duration = Duration::from_secs(duration_seconds_fractional.round() as u64);

            Ok(Self {
                name: file_name,
                duration,
            })
        })
        .await
        .map_err(error::ffmpeg)?
    }
}

#[cfg(test)]
mod tests {
    use futures::{
        StreamExt,
        executor::{block_on, block_on_stream},
    };

    use crate::tests::video::SAMPLE_VIDEO_SOURCES;

    use super::*;

    #[tokio::test]
    async fn fetch_images() {
        let futures = SAMPLE_VIDEO_SOURCES.iter().map(|&src| async move {
            let video_metadata = VideoMetadata::open(src).await.unwrap();
            let file_stem = Path::new(&video_metadata.name)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap();
            let dir = Path::new("..")
                .join("out")
                .join(format!("video-{file_stem}"));
            let frames_dir = dir.join(DIR_FRAMES);

            if !dir.exists() {
                fs::create_dir_all(&dir).await.unwrap();
            }
            if frames_dir.exists() {
                fs::remove_dir_all(&frames_dir).await.unwrap_or_else(|e| {
                    panic!(
                        "could not find dir '{}' with failure: {e:#?}",
                        frames_dir.canonicalize().unwrap().to_str().unwrap()
                    )
                });
            }

            let mut images = export_images_from_video(src, &dir);

            let mut seconds = 0u64;
            while let Some(_image) = images.next().await {
                seconds += 1;
            }
            let seconds_duration = Duration::from_secs(seconds);

            let diff = seconds_duration.abs_diff(video_metadata.duration);
            let accepted_dur = Duration::from_secs(7);
            assert!(
                diff <= accepted_dur,
                "file diff not working: {} with time diff: {:?}",
                video_metadata.name,
                video_metadata.duration
            );
        });

        let _res = futures::future::join_all(futures).await;
    }
}
