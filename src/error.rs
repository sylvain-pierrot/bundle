use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Cbor(#[from] aqueduct_cbor::Error),
    #[error("invalid bundle processing control flags")]
    InvalidFlags,
    #[error("unsupported bundle version: {0}")]
    UnsupportedVersion(u8),
    #[error("invalid CRC type code: {0}")]
    InvalidCrcType(u64),
    #[error("invalid EID scheme: {0}")]
    InvalidEidScheme(u64),
    #[error("block type mismatch: expected {expected}, got {actual}")]
    BlockTypeMismatch { expected: u64, actual: u64 },
    #[error("expected exactly 1 payload block, found {0}")]
    InvalidPayloadCount(usize),
    #[error("duplicate block number: {0}")]
    DuplicateBlockNumber(u64),
    #[error("invalid CBOR structure")]
    InvalidCbor,
    #[error("CRC verification failed")]
    CrcMismatch,
}
