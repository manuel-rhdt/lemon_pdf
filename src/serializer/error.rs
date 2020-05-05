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
}

impl std::error::Error for Error {}

impl ser::Error for Error {
    fn custom<T>(_msg: T) -> Self
    where
        T: std::fmt::Display,
    {
        todo!()
    }
}

impl de::Error for Error {
    fn custom<T>(_msg: T) -> Self
    where
        T: std::fmt::Display,
    {
        todo!()
    }
}
