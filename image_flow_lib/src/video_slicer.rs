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
use tracing::{debug, error, info, instrument, trace, warn};

static ONCE: OnceCell<bool> = OnceCell::const_new();

#[derive(Debug)]
pub struct FramePath {
    path: PathBuf,

    should_drop: bool,
}

impl Drop for FramePath {
    fn drop(&mut self) {
        if self.should_drop {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[instrument(skip(video_source_path, temporary_storage), fields(video_path = %video_source_path.as_ref().display()))]
pub(crate) fn export_images_from_video(
    video_source_path: impl AsRef<Path>,
    temporary_storage: impl AsRef<Path>,
) -> impl Stream<Item = crate::Result<FramePath>> {
    let (tx, rx) = mpsc::channel::<crate::Result<FramePath>>(16);

    let temporary_storage = temporary_storage.as_ref().to_path_buf();
    let video_source_path = video_source_path.as_ref().to_path_buf();
    tokio::spawn(async move {
        ONCE.get_or_init(|| async {
            info!("init ffmpeg");
            ffmpeg::init().expect("Ffmpeg could not init");
            true
        })
        .await;

        let temp_storage_disp = temporary_storage.display();
        debug!(dir = %temp_storage_disp, "ensuring frames directory exists");
        if let Err(e) = fs::create_dir_all(&temporary_storage).await {
            error!(error = %e, dir = %temp_storage_disp, "failed to create frames directory");
            let _ = tx.send(Err(e.into())).await;
            return;
        }

        let video_path_display = video_source_path.display().to_string();
        tokio::task::spawn_blocking(move || {
            info!(video_path = %video_path_display, "starting blocking decode task");
            if let Err(e) = decode_and_send(&video_source_path, &temporary_storage, &tx) {
                error!(error = ?e, video_path = %video_path_display, "decode_and_send failed");
                let _ = tx.blocking_send(Err(e));
            } else {
                info!(video_path = %video_path_display, "decode_and_send completed successfully");
            }
        });
    });

    ReceiverStream::new(rx)
}

#[instrument(skip(video_source_path, relative_dir_storage_path, tx))]
fn decode_and_send(
    video_source_path: impl AsRef<Path>,
    relative_dir_storage_path: impl AsRef<Path>,
    tx: &mpsc::Sender<crate::Result<FramePath>>,
) -> crate::Result<()> {
    let mut ictx = format::input(&video_source_path)?;
    let input_stream = ictx
        .streams()
        .best(Type::Video)
        .ok_or(ffmpeg::Error::StreamNotFound)?;
    let video_stream_index = input_stream.index();
    let time_base = input_stream.time_base();

    debug!(
        video_stream_index,
        time_base_num = time_base.numerator(),
        time_base_den = time_base.denominator(),
        "resolved best video stream"
    );

    let context_decoder = codec::Context::from_parameters(input_stream.parameters())?;
    let mut decoder = context_decoder.decoder().video()?;

    info!(
        width = decoder.width(),
        height = decoder.height(),
        format = ?decoder.format(),
        "video decoder created"
    );

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
                    trace!(pts, next_second_to_capture, "capturing frame at pts");

                    let lhs = pts * i64::from(time_base.numerator());
                    let rhs = next_second_to_capture * i64::from(time_base.denominator());
                    if lhs >= rhs {
                        let mut rgb_frame = Video::empty();
                        scaler.run(&video_decoded, &mut rgb_frame)?;

                        let frame_path = relative_dir_storage_path
                            .as_ref()
                            .join(format!("{next_second_to_capture:05}s.png"));
                        let frame_path_disp = frame_path.clone();
                        let frame_path_disp = frame_path_disp.display();

                        if let Err(e) = save_png(&rgb_frame, &frame_path) {
                            warn!(error = ?e, path = %frame_path.display(), "failed to save frame png");
                        } else {
                            debug!(path = %frame_path_disp, "frame saved");
                        }

                        // Stop decoding early if the receiver was dropped.
                        let tmp_frame_path = FramePath {
                            path: frame_path,
                            should_drop: true,
                        };
                        if tx.blocking_send(Ok(tmp_frame_path)).is_err() {
                            warn!(path = %frame_path_disp, "receiver dropped, stopping decode early");
                            return Ok(());
                        }

                        next_second_to_capture += 1;
                    }
                }
            }
            Ok(())
        };

    let mut packets_for_stream = 0u64;
    for (stream, packet) in ictx.packets() {
        if stream.index() == video_stream_index {
            packets_for_stream += 1;
            decoder.send_packet(&packet)?;
            receive_and_process_decoded_frames(&mut decoder)?;
        }
    }
    debug!(packets_for_stream, "finished feeding packets, sending EOF");

    decoder.send_eof()?;
    receive_and_process_decoded_frames(&mut decoder)?;

    info!(
        frames_captured = next_second_to_capture,
        "decoding finished"
    );

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
    .map_err(crate::error::ffmpeg)
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
                .ok_or_else(|| crate::error::io(String::from("no file name found")))?
                .to_str()
                .ok_or_else(|| {
                    crate::error::io(String::from("file name has unsupported encoding"))
                })?
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
        .map_err(crate::error::ffmpeg)?
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, time::Duration};

    use futures::StreamExt;
    use tokio::fs;

    use crate::{
        tests::video::SAMPLE_VIDEO_SOURCES,
        video_slicer::{VideoMetadata, export_images_from_video},
    };

    const ACCEPTED_DUR: Duration = Duration::from_secs(1);

    async fn check_video(src: &str) -> Result<(), String> {
        let video_metadata = VideoMetadata::open(src)
            .await
            .map_err(|e| format!("failed to open {src}: {e:#?}"))?;

        let file_stem = Path::new(&video_metadata.name)
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("invalid file stem for {}", video_metadata.name))?;

        let dir = Path::new("..")
            .join("out")
            .join(format!("video-{file_stem}"));

        if dir.exists() {
            fs::remove_dir_all(&dir).await.ok(); // best-effort cleanup
        }
        fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;

        let seconds = export_images_from_video(src, &dir)
            .fold(0u64, |acc, frame| async move {
                if let Ok(mut frame) = frame {
                    frame.should_drop = false;
                }
                acc + 1
            })
            .await as u64;
        let diff = Duration::from_secs(seconds).abs_diff(video_metadata.duration);

        if diff > ACCEPTED_DUR {
            return Err(format!(
                "{}: expected ~{:?}, got diff {:?}",
                video_metadata.name, video_metadata.duration, seconds
            ));
        }
        Ok(())
    }

    #[tokio::test]
    async fn fetch_images() {
        let errors: Vec<String> =
            futures::future::join_all(SAMPLE_VIDEO_SOURCES.iter().map(|&src| check_video(src)))
                .await
                .into_iter()
                .filter_map(Result::err)
                .collect();

        assert!(errors.is_empty(), "failures:\n{}", errors.join("\n"));
    }
}
