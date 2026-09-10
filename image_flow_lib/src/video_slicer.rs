use std::{ops::Deref, path::Path};

use chrono::Duration;
use ffmpeg_next::{
    self as ffmpeg, codec, decoder,
    format::{self, Pixel},
    frame::Video,
    media::Type::{self},
    software::scaling::{self, Flags},
};
use futures::StreamExt;
use image::ColorType;
use tokio::{
    fs::{self},
    sync::{OnceCell, mpsc},
};
use tokio_stream::{Stream, wrappers::ReceiverStream};
use tracing::{debug, error, info, instrument, trace, warn};

use crate::{
    FramePath,
    location::{
        coordinate_system::Coordinate,
        tabular_gps::{BestMatchByDistance, DataPointsGPS, TabularGPS},
    },
};

pub struct ImageGPSZip {
    zipped_content: Vec<(Coordinate, FramePath)>,
}

impl Deref for ImageGPSZip {
    type Target = Vec<(Coordinate, FramePath)>;

    fn deref(&self) -> &Self::Target {
        &self.zipped_content
    }
}

impl ImageGPSZip {
    /// # Errors
    /// TODO
    pub async fn from_video_and_gps(
        video_path: impl AsRef<Path>,
        tabular_data: &DataPointsGPS,
        storage_path: impl AsRef<Path>,
        capture_interval: Duration,
    ) -> crate::Result<Self> {
        let mut images = export_images_from_video(video_path, storage_path, capture_interval);

        let mut frame_paths = Vec::new();
        while let Some(frame) = images
            .next()
            .await
        {
            let frame = frame?;
            let offset = frame.time_offset_from_first_frame;
            let cord = tabular_data
                .fetch(offset, Duration::milliseconds(400))
                .best_match_by_distance(offset)
                .map(TabularGPS::coordinates);

            if let Some(cord) = cord {
                frame_paths.push((cord, frame));
            }
        }

        Ok(Self {
            zipped_content: frame_paths,
        })
    }

    pub fn do_not_remove_frames(&mut self) {
        for content in &mut self.zipped_content {
            content
                .1
                .should_drop = false;
        }
    }
}

static ONCE: OnceCell<bool> = OnceCell::const_new();

/// # Panics
/// If FFMPEG cant init this will crash surely.
#[instrument(skip(video_source_path, temporary_storage), fields(video_path = %video_source_path.as_ref().display()))]
pub fn export_images_from_video(
    video_source_path: impl AsRef<Path>,
    temporary_storage: impl AsRef<Path>,
    capture_interval: Duration,
) -> impl Stream<Item = crate::Result<FramePath>> {
    let (tx, rx) = mpsc::channel::<crate::Result<FramePath>>(16);

    let temporary_storage = temporary_storage
        .as_ref()
        .to_path_buf();
    let video_source_path = video_source_path
        .as_ref()
        .to_path_buf();
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
            let _ = tx
                .send(Err(e.into()))
                .await;
            return;
        }

        let video_path_display = video_source_path
            .display()
            .to_string();
        tokio::task::spawn_blocking(move || {
            info!(video_path = %video_path_display, "starting blocking decode task");
            if let Err(e) = decode_and_send(
                &video_source_path,
                &temporary_storage,
                &tx,
                capture_interval,
            ) {
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
    capture_interval: Duration,
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
    let mut decoder = context_decoder
        .decoder()
        .video()?;

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

    let mut next_capture_time = Duration::zero();
    let mut frames_captured = 0u64;
    let mut first_pts = None;

    let mut receive_and_process_decoded_frames =
        |decoder: &mut decoder::Video| -> crate::Result<()> {
            let mut video_decoded = Video::empty();
            while decoder
                .receive_frame(&mut video_decoded)
                .is_ok()
            {
                if let Some(pts) = video_decoded.timestamp() {
                    let base_pts = *first_pts.get_or_insert(pts);
                    let current_pts_time = pts_to_duration(pts - base_pts, time_base);

                    trace!(pts, ?next_capture_time, "capturing frame at pts");

                    if current_pts_time >= next_capture_time {
                        let mut rgb_frame = Video::empty();
                        scaler.run(&video_decoded, &mut rgb_frame)?;

                        let frame_path = relative_dir_storage_path
                            .as_ref()
                            .join(format!("{:09}s.png", current_pts_time.num_milliseconds()));
                        let frame_path_disp = frame_path.clone();
                        let frame_path_disp = frame_path_disp.display();

                        if let Err(e) = save_png(&rgb_frame, &frame_path) {
                            warn!(error = ?e, path = %frame_path.display(), "failed to save frame png");
                        } else {
                            debug!(path = %frame_path_disp, "frame saved");
                        }

                        let tmp_frame_path = FramePath::new_impl(current_pts_time, frame_path);
                        if tx
                            .blocking_send(Ok(tmp_frame_path))
                            .is_err()
                        {
                            warn!(path = %frame_path_disp, "receiver dropped, stopping decode early");
                            return Ok(());
                        }

                        frames_captured += 1;
                        next_capture_time += capture_interval;
                    }
                }
            }
            Ok(())
        };

    let mut packets_for_stream = 0u64;
    for (stream, packet) in ictx.packets() {
        if stream.index() == video_stream_index {
            packets_for_stream += 1;
            if let Err(e) = decoder.send_packet(&packet) {
                error!(error = ?e, packets_for_stream, "send_packet failed");
                return Err(e.into());
            }
            receive_and_process_decoded_frames(&mut decoder)?;
        }
    }
    debug!(packets_for_stream, "finished feeding packets, sending EOF");

    decoder.send_eof()?;
    receive_and_process_decoded_frames(&mut decoder)?;

    info!(frames_captured, "decoding finished");

    Ok(())
}

fn pts_to_duration(pts: i64, time_base: ffmpeg::Rational) -> Duration {
    let num = i128::from(time_base.numerator());
    let den = i128::from(time_base.denominator());

    let nanos = i128::from(pts)
        .saturating_mul(num)
        .saturating_mul(1_000_000_000)
        / den;

    Duration::nanoseconds(nanos.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64)
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

pub struct VideoMetadata {
    pub name: String,
    pub duration: Duration,
}

impl VideoMetadata {
    /// # Errors
    /// TODO
    pub async fn open<P>(path: P) -> crate::Result<Self>
    where
        P: AsRef<Path>,
    {
        let path = path
            .as_ref()
            .to_path_buf();

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
            let duration = Duration::seconds(duration_seconds_fractional.round() as _);

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
    use chrono::Duration;
    use futures::StreamExt;
    use std::path::Path;

    use crate::{
        location::tabular_gps::DataPointsGPS,
        tests::{TESTING_FILE_MANAGER, video_data::SAMPLE_VIDEO_SOURCES},
        video_slicer::{ImageGPSZip, VideoMetadata, export_images_from_video},
    };

    const ACCEPTED_DUR: Duration = Duration::seconds(1);
    const CAPTURE_INTERVAL: Duration = Duration::seconds(1);

    async fn check_video(src: &str) -> Result<(), String> {
        let video_metadata = VideoMetadata::open(src)
            .await
            .map_err(|e| format!("failed to open {src}: {e:#?}"))?;

        let file_stem = Path::new(&video_metadata.name)
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("invalid file stem for {}", video_metadata.name))?;

        let out_dir = {
            TESTING_FILE_MANAGER
                .lock()
                .await
                .test_out(format!("video-{file_stem}"))
        };

        let mut stream = export_images_from_video(src, &out_dir, CAPTURE_INTERVAL);
        let mut seconds = 0i64;
        while let Some(frame) = stream
            .next()
            .await
        {
            let mut frame = frame.map_err(|e| format!("frame error for {src}: {e:#?}"))?;
            frame.should_drop = false;
            seconds += 1;
        }
        let diff = (Duration::seconds(seconds) - video_metadata.duration).abs();

        if diff > ACCEPTED_DUR {
            return Err(format!(
                "{}: expected ~{:?}, got {} frames (diff {:?})",
                video_metadata.name, video_metadata.duration, seconds, diff
            ));
        }
        Ok(())
    }

    #[tokio::test]
    async fn fetch_images() {
        let errors: Vec<String> = futures::future::join_all(
            SAMPLE_VIDEO_SOURCES
                .iter()
                .map(|src| check_video(src.video_path)),
        )
        .await
        .into_iter()
        .filter_map(Result::err)
        .collect();

        assert!(errors.is_empty(), "failures:\n{}", errors.join("\n"));
    }

    #[tokio::test]
    async fn process_pipeline() {
        for (index, gps_video_combo) in SAMPLE_VIDEO_SOURCES
            .iter()
            .enumerate()
        {
            let out_dir = {
                TESTING_FILE_MANAGER
                    .lock()
                    .await
                    .test_out(Path::new("video_pipeline").join(format!("{index}")))
            };

            let raw_b = gps_video_combo
                .csv_embed
                .as_bytes();
            let csv_gps_data = DataPointsGPS::from_csv(raw_b).unwrap();
            let mut zipped = ImageGPSZip::from_video_and_gps(
                gps_video_combo.video_path,
                &csv_gps_data,
                out_dir,
                Duration::seconds(1),
            )
            .await
            .unwrap();
            zipped.do_not_remove_frames();

            // TODO
            // We should have the ability to call on each iteration: evaluate_context and pass in
            // the image and the coordinate.
            //
            // The image should then be passed through computer vision
            //
            // The location can already be evaluated!
        }
    }
}
