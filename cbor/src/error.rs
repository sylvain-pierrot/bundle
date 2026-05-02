use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("unexpected end of CBOR input")]
    UnexpectedEof,
    #[error("expected CBOR {expected}, found initial byte 0x{actual:02x}")]
    UnexpectedCborType { expected: &'static str, actual: u8 },
    #[error("invalid CBOR encoding")]
    InvalidCbor,
    #[error("invalid UTF-8 in CBOR text string")]
    InvalidUtf8,
    #[error(transparent)]
    Io(#[from] bundle_io::Error),
}
