use std::fmt;

/// Application level error that can be handed back to the webview.
///
/// Tauri requires command errors to be serializable, so this wraps a plain
/// message and serializes as a string. The frontend then receives the same
/// shape it always did (`String(err)` in `App.tsx`).
#[derive(Debug)]
pub struct Error(String);

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn msg(message: impl Into<String>) -> Self {
        Error(message.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl serde::Serialize for Error {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

macro_rules! from_error {
    ($($kind:ty),* $(,)?) => {
        $(
            impl From<$kind> for Error {
                fn from(err: $kind) -> Self {
                    Error(err.to_string())
                }
            }
        )*
    };
}

from_error!(
    std::io::Error,
    std::string::FromUtf8Error,
    serde_json::Error,
    regex::Error,
    reqwest::Error,
    zip::result::ZipError,
    walkdir::Error,
    tokio::task::JoinError,
);

#[cfg(windows)]
from_error!(windows_service::Error);
