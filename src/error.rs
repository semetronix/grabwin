use std::fmt;

#[derive(Debug)]
pub enum Error {
    /// Selector did not match any window.
    NotFound(String),
    /// The captured window was closed (or the capture was closed by the user).
    Closed,
    /// Window is minimized and no frame is buffered yet.
    Minimized,
    /// No frame arrived within the timeout (milliseconds).
    Timeout(u32),
    /// WGC or D3D11 unavailable on this system.
    Unsupported(String),
    /// Any Win32 / WinRT failure.
    Windows(windows::core::Error),
    /// PNG encoding failed.
    Encode(String),
    /// Bad argument from the caller (unknown pixel format, bad selector combo, ...).
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<windows::core::Error> for Error {
    fn from(e: windows::core::Error) -> Self {
        Error::Windows(e)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotFound(s) => write!(f, "window not found: {s}"),
            Error::Closed => write!(f, "window was closed"),
            Error::Minimized => write!(f, "window is minimized and no frame is buffered"),
            Error::Timeout(ms) => write!(f, "no frame within {ms} ms"),
            Error::Unsupported(s) => write!(f, "capture unsupported: {s}"),
            Error::Windows(e) => write!(f, "windows error: {e}"),
            Error::Encode(s) => write!(f, "png encode error: {s}"),
            Error::Invalid(s) => write!(f, "invalid argument: {s}"),
        }
    }
}

impl std::error::Error for Error {}
