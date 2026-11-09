use serde::ser;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Display, From)]
pub enum Error {
    Io(std::io::Error),
    /// An error reported by a `Serialize` implementation, or a value that cannot be represented
    /// in PDF.
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
