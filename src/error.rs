use std::error::Error as StdError;
use std::fmt;
use std::io;
use std::path::PathBuf;

/// Errors that can be return from various operatiors
///
#[derive(Debug)]
pub enum Error {
    /// Loading a library failed
    Load(io::Error),
    /// File copy operation failed
    Copy(io::Error, PathBuf, PathBuf),
    /// Timeout of file copy happend.
    CopyTimeOut(PathBuf, PathBuf),
    /// Failed to find library
    Find(String),
}

impl StdError for Error {
    fn description(&self) -> &str {
        match *self {
            Error::Load(_) => "Unable to load library",
            Error::Copy(_, _, _) => "Unable to copy",
            Error::CopyTimeOut(_, _) => "Unable to copy due to time out",
            Error::Find(_) => "Unable to find",
        }
    }

    #[allow(deprecated)]
    fn cause(&self) -> Option<&dyn StdError> {
        match *self {
            Error::Load(ref e) => e.cause(),
            Error::Copy(ref e, _, _) => e.cause(),
            Error::CopyTimeOut(_, _) => None,
            Error::Find(_) => None,
        }
    }
}

impl fmt::Display for Error {
    #[allow(deprecated)]
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::Load(ref e) => write!(
                fmt,
                "{} {}\nDue to: {:?}",
                self.description(),
                e.description(),
                self.cause()
            ),
            Error::Copy(ref e, ref src, ref dest) => write!(
                fmt,
                "{} {:?} to {:?}\n{}\nDue to: {:?}",
                self.description(),
                src,
                dest,
                e.description(),
                self.cause()
            ),
            Error::CopyTimeOut(ref src, ref dest) => {
                write!(fmt, "{} {:?} to {:?}", self.description(), src, dest)
            }
            Error::Find(ref name) => write!(fmt, "{} {}", self.description(), name),
        }
    }
}
