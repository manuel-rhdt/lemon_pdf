use serde::{de, ser};

use std::str::Utf8Error;
use std::string::FromUtf8Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Display, From)]
pub enum Error {
    Io(std::io::Error),
    Utf8(Utf8Error),
    FromUtf8(FromUtf8Error),
    SpuriousDictEnd,
    /// An error reported by a `Serialize` or `Deserialize` implementation, or a value that
    /// cannot be represented in PDF.
    #[from(ignore)]
    Custom(String),
}

impl std::error::Error for Error {}

impl ser::Error for Error {
    fn custom<T>(msg: T) -> Self
    where
        T: std::fmt::Display,
    {
        Error::Custom(msg.to_string())
    }
}

impl de::Error for Error {
    fn custom<T>(msg: T) -> Self
    where
        T: std::fmt::Display,
    {
        Error::Custom(msg.to_string())
    }
}
