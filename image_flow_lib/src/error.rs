use std::{
    error::Error as StdError,
    fmt::{self, Display},
    io,
};

use ultralytics_inference::InferenceError;

#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum ImageFlowError {
    #[error("Request failed: {message}")]
    Request { message: String },

    #[error("Serialization failed: {message}")]
    Serialization { message: String },

    #[error("I/O failed: {message}")]
    Io { message: String },

    #[error("Image failure: {message}")]
    Image { message: String },

    #[cfg(feature = "video")]
    #[error("FFmpeg failed: {message}")]
    Ffmpeg { message: String },
}

impl From<Error> for ImageFlowError {
    fn from(error: Error) -> Self {
        let message = error.to_string();

        match error
            .inner
            .kind
        {
            Kind::Request => Self::Request { message },
            Kind::Serialization => Self::Serialization { message },
            Kind::Io => Self::Io { message },
            Kind::Image => Self::Image { message },

            #[cfg(feature = "video")]
            Kind::Ffmpeg => Self::Ffmpeg { message },
            Kind::FileNotExists => Self::Io { message },
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Error {
    inner: Box<Inner>,
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self
            .inner
            .kind
        {
            Kind::Request => "request error",
            Kind::Serialization => "serialization error",
            Kind::Io => "I/O error",
            Kind::Image => "Image failure",

            #[cfg(feature = "video")]
            Kind::Ffmpeg => "FFmpeg error",
            Kind::FileNotExists => "File does not exist",
        };

        match &self
            .inner
            .source
        {
            Some(source) => write!(f, "{kind}: {source}"),
            None => f.write_str(kind),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.inner
            .source
            .as_deref()
            .map(|source| source as _)
    }
}

pub(crate) type BoxError = Box<dyn StdError + Send + Sync>;

#[derive(Debug)]
struct Inner {
    kind: Kind,
    source: Option<BoxError>,
}

#[derive(Debug)]
pub(crate) enum Kind {
    Request,
    Serialization,
    Io,
    Image,
    FileNotExists,

    #[cfg(feature = "video")]
    Ffmpeg,
}

#[allow(clippy::use_self)]
impl Error {
    pub(crate) fn new<E>(kind: Kind, source: Option<E>) -> Error
    where
        E: Into<BoxError>,
    {
        Error {
            inner: Box::new(Inner {
                kind,
                source: source.map(Into::into),
            }),
        }
    }
}

pub(crate) fn request<E>(e: E) -> Error
where
    E: Into<BoxError>,
{
    Error::new(Kind::Request, Some(e))
}

pub(crate) fn serialization<E>(e: E) -> Error
where
    E: Into<BoxError>,
{
    Error::new(Kind::Serialization, Some(e))
}

#[cfg(feature = "video")]
pub(crate) fn ffmpeg<E>(e: E) -> Error
where
    E: Into<BoxError>,
{
    Error::new(Kind::Ffmpeg, Some(e))
}

pub(crate) fn io<E>(e: E) -> Error
where
    E: Into<BoxError>,
{
    Error::new(Kind::Io, Some(e))
}

pub(crate) fn file_does_not_exist<E>(e: E) -> Error
where
    E: Into<BoxError>,
{
    Error::new(Kind::FileNotExists, Some(e))
}

pub(crate) fn image<E>(e: E) -> Error
where
    E: Into<BoxError>,
{
    Error::new(Kind::Image, Some(e))
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut builder = f.debug_struct("image_flow::Error");
        builder.field(
            "kind",
            &self
                .inner
                .kind,
        );

        if let Some(ref source) = self
            .inner
            .source
        {
            builder.field("source", source);
        }

        builder.finish()
    }
}

impl From<serde_json::Error> for crate::Error {
    fn from(value: serde_json::Error) -> Self {
        serialization(value)
    }
}

impl From<image::ImageError> for crate::Error {
    fn from(value: image::ImageError) -> Self {
        image(value)
    }
}

#[cfg(feature = "video")]
impl From<ffmpeg_next::Error> for crate::Error {
    fn from(value: ffmpeg_next::Error) -> Self {
        ffmpeg(value)
    }
}

impl From<io::Error> for crate::Error {
    fn from(value: io::Error) -> Self {
        io(value)
    }
}

impl From<InferenceError> for crate::Error {
    fn from(value: InferenceError) -> Self {
        image(value)
    }
}
