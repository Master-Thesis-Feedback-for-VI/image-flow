use std::{fs::File, path::Path};

use ffmpeg_next::{
    self as ffmpeg, codec,
    format::{self, Pixel},
    media::Type,
    software::scaling::{self, Flags},
};
use tempfile::{tempdir, tempfile};
use tokio::{fs::create_dir_all, sync::OnceCell};

use crate::error;

static ONCE: OnceCell<bool> = OnceCell::const_new();
const DIR_FRAMES: &str = "frames";

pub(crate) async fn set_fps_from_video<P: AsRef<Path> + Send + 'static>(
    path: P,
    _fps: usize,
) -> crate::Result<File> {
    let _ = ONCE
        .get_or_init(|| async {
            ffmpeg::init().unwrap();
            true
        })
        .await;

    let relative_dir = path.as_ref().join(DIR_FRAMES);
    let () = create_dir_all(relative_dir).await?;

    let handle = tokio::task::spawn_blocking(move || -> crate::Result<File> {
        let mut ictx = format::input(&path)?;
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

        let f = tempfile()?;
        Ok(f)
    });

    handle.await.map_err(error::ffmpeg)?
}
