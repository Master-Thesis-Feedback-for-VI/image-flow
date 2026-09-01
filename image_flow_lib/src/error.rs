use std::{error::Error as StdError, fmt, io};

pub type Result<T> = std::result::Result<T, Error>;

pub struct Error {
    inner: Box<Inner>,
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

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut builder = f.debug_struct("image_flow::Error");
        builder.field("kind", &self.inner.kind);

        if let Some(ref source) = self.inner.source {
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
