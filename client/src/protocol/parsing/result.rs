use std::io;
use std::convert::From;
use std::string::FromUtf8Error;

#[derive(Debug)]
pub enum ParseError {
    IoError(io::Error),
    EncodingError(FromUtf8Error),
    PredicateFailed(&'static str),
    InvalidDiscriminator(u64)
}
impl ParseError {
    pub fn is_eof(&self) -> bool {
        if let ParseError::IoError(ref io_error) = *self {
            io_error.kind() == io::ErrorKind::UnexpectedEof
        } else {
            false
        }
    }
}

pub type ParseResult<T, Input> = Result<(T, Input), (ParseError, Input)>;
pub type BytePosition = usize;
pub type ParseEndResult<T> = Result<T, (ParseError, BytePosition)>;

#[derive(Debug)]
pub enum WriteError {
    IoError(io::Error),
    ConversionFailed(&'static str),
    PredicateFailed(&'static str)
}
impl WriteError {
    pub fn is_io_error(&self) -> bool {
        match *self {
            WriteError::IoError(_) => true,
            _ => false
        }
    }
}

pub type WriteResult = Result<(), WriteError>;

#[macro_export]
macro_rules! try_parse {
    ( $input:expr, $e:expr ) => {
        match $e {
            Ok(x) => x,
            Err(error) => return Err(
                (::std::convert::From::from(error), $input))
        }
    }
}

impl From<FromUtf8Error> for ParseError {
    fn from(error : FromUtf8Error) -> Self {
        ParseError::EncodingError(error)
    }
}
impl From<io::Error> for ParseError {
    fn from(error : io::Error) -> Self {
        ParseError::IoError(error)
    }
}

impl From<io::Error> for WriteError {
    fn from(error : io::Error) -> Self {
        WriteError::IoError(error)
    }
}
