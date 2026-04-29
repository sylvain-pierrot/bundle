use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Cbor(#[from] aqueduct_cbor::Error),

    #[error("unsupported bundle version: {0}")]
    UnsupportedVersion(u8),

    #[error("expected exactly 1 payload block, found {0}")]
    InvalidPayloadCount(usize),

    #[error("duplicate block number: {0}")]
    DuplicateBlockNumber(u64),

    #[error("invalid bundle processing control flags")]
    InvalidFlags,

    #[error("invalid block array length: expected {expected}, got {actual}")]
    InvalidBlockLength {
        expected: &'static str,
        actual: usize,
    },

    #[error("block type mismatch: expected {expected}, got {actual}")]
    BlockTypeMismatch { expected: u64, actual: u64 },

    #[error("invalid CRC type code: {0}")]
    InvalidCrcType(u64),

    #[error("invalid CRC value length: expected {expected}, got {actual}")]
    InvalidCrcLength { expected: usize, actual: usize },

    #[error("CRC verification failed")]
    CrcMismatch,

    #[error("CRC offset out of bounds")]
    CrcOutOfBounds,

    #[error("invalid EID scheme: {0}")]
    InvalidEidScheme(u64),

    #[error("invalid EID structure")]
    InvalidEid,

    #[error("integer value overflows target type")]
    IntegerOverflow,

    #[error("cannot parse retained block as extension")]
    PayloadNotInline,

    #[error("payload data exceeds declared length")]
    PayloadOverflow,

    #[error("payload not consumed before next block")]
    PayloadNotConsumed,

    #[error("retention is empty")]
    EmptyRetention,

    #[error("bundle stream not fully consumed")]
    IncompleteRead,

    #[error("bundle rejected: {0}")]
    FilterRejected(#[from] crate::filter::FilterRejection),
}
