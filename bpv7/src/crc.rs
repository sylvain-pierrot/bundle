use aqueduct_cbor::{Decoder, Encoder};

use crate::error::Error;

// `static` (not `const`) so that `CrcAlgo::digest()` can borrow with 'static lifetime.
static CRC16: crc::Crc<u16> = crc::Crc::<u16>::new(&crc::CRC_16_IBM_SDLC);
static CRC32C: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISCSI);

/// CRC field (RFC 9171 §4.2.1).
///
/// Encodes both the CRC variant and the value in a single enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Crc {
    None,
    Crc16(u16),
    Crc32c(u32),
}

impl Crc {
    /// Create a CRC-16 placeholder (value computed during encode).
    pub fn crc16() -> Self {
        Crc::Crc16(0)
    }

    /// Create a CRC-32C placeholder (value computed during encode).
    pub fn crc32c() -> Self {
        Crc::Crc32c(0)
    }

    /// Create a placeholder from a CRC type code (0, 1, or 2).
    pub fn placeholder(crc_type: u64) -> Result<Self, Error> {
        match crc_type {
            0 => Ok(Crc::None),
            1 => Ok(Crc::Crc16(0)),
            2 => Ok(Crc::Crc32c(0)),
            _ => Err(Error::InvalidCrcType(crc_type)),
        }
    }

    #[inline]
    pub fn is_none(self) -> bool {
        matches!(self, Crc::None)
    }

    /// CRC type code: 0 = none, 1 = CRC-16, 2 = CRC-32C.
    #[inline]
    pub fn crc_type(self) -> u64 {
        match self {
            Crc::None => 0,
            Crc::Crc16(_) => 1,
            Crc::Crc32c(_) => 2,
        }
    }

    /// Compute CRC-16 (X-25 / IBM-SDLC) checksum.
    pub fn compute_crc16(data: &[u8]) -> u16 {
        CRC16.checksum(data)
    }

    /// Compute CRC-32C (Castagnoli) checksum.
    pub fn compute_crc32c(data: &[u8]) -> u32 {
        CRC32C.checksum(data)
    }

    /// Compute a CRC of the given type over data.
    pub fn compute(crc_type: u64, data: &[u8]) -> Result<Self, Error> {
        match crc_type {
            0 => Ok(Crc::None),
            1 => Ok(Crc::Crc16(Self::compute_crc16(data))),
            2 => Ok(Crc::Crc32c(Self::compute_crc32c(data))),
            _ => Err(Error::InvalidCrcType(crc_type)),
        }
    }

    /// Verify this CRC against serialized block bytes.
    ///
    /// Hashes in three segments (before CRC, zeroed CRC, after CRC)
    /// to avoid cloning the block bytes.
    pub fn verify(&self, block_bytes: &[u8], crc_data_offset: usize) -> Result<(), Error> {
        match self {
            Crc::None => Ok(()),
            Crc::Crc16(expected) => {
                let crc_end = crc_data_offset
                    .checked_add(2)
                    .filter(|&e| e <= block_bytes.len())
                    .ok_or(Error::CrcOutOfBounds)?;
                let mut digest = CRC16.digest();
                digest.update(&block_bytes[..crc_data_offset]);
                digest.update(&[0, 0]);
                digest.update(&block_bytes[crc_end..]);
                if digest.finalize() == *expected {
                    Ok(())
                } else {
                    Err(Error::CrcMismatch)
                }
            }
            Crc::Crc32c(expected) => {
                let crc_end = crc_data_offset
                    .checked_add(4)
                    .filter(|&e| e <= block_bytes.len())
                    .ok_or(Error::CrcOutOfBounds)?;
                let mut digest = CRC32C.digest();
                digest.update(&block_bytes[..crc_data_offset]);
                digest.update(&[0, 0, 0, 0]);
                digest.update(&block_bytes[crc_end..]);
                if digest.finalize() == *expected {
                    Ok(())
                } else {
                    Err(Error::CrcMismatch)
                }
            }
        }
    }

    /// Decode a CRC value from a buffer-based CBOR decoder.
    pub fn decode_buf(dec: &mut Decoder<'_>, crc_type: u64) -> Result<Self, Error> {
        let bytes = dec.read_bstr()?;
        Self::from_bytes(crc_type, bytes)
    }

    /// Decode a CRC value byte string from a streaming CBOR decoder.
    pub fn decode_stream<R: aqueduct_cbor::Read>(
        dec: &mut aqueduct_cbor::StreamDecoder<R>,
        crc_type: u64,
    ) -> Result<Self, Error> {
        let bytes = dec.read_bstr()?;
        Self::from_bytes(crc_type, &bytes)
    }

    /// Parse a CRC value from raw bytes.
    pub fn from_bytes(crc_type: u64, bytes: &[u8]) -> Result<Self, Error> {
        match crc_type {
            1 => {
                if bytes.len() != 2 {
                    return Err(Error::InvalidCrcLength {
                        expected: 2,
                        actual: bytes.len(),
                    });
                }
                Ok(Crc::Crc16(u16::from_be_bytes([bytes[0], bytes[1]])))
            }
            2 => {
                if bytes.len() != 4 {
                    return Err(Error::InvalidCrcLength {
                        expected: 4,
                        actual: bytes.len(),
                    });
                }
                Ok(Crc::Crc32c(u32::from_be_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3],
                ])))
            }
            _ => Err(Error::InvalidCrcType(crc_type)),
        }
    }

    /// Write the CRC placeholder, compute over the block, and patch.
    ///
    /// Call after encoding all other block fields. `block_start` is the
    /// encoder position at the beginning of the block.
    pub fn encode_and_finalize(&self, enc: &mut Encoder, block_start: usize) {
        match self {
            Crc::None => {}
            Crc::Crc16(_) => {
                let crc_data_pos = enc.position() + 1;
                enc.write_bstr(&[0, 0]);
                let checksum = Self::compute_crc16(&enc.as_bytes()[block_start..enc.position()]);
                enc.patch(crc_data_pos, &checksum.to_be_bytes());
            }
            Crc::Crc32c(_) => {
                let crc_data_pos = enc.position() + 1;
                enc.write_bstr(&[0, 0, 0, 0]);
                let checksum = Self::compute_crc32c(&enc.as_bytes()[block_start..enc.position()]);
                enc.patch(crc_data_pos, &checksum.to_be_bytes());
            }
        }
    }

    /// CRC value size in bytes (0 for None, 2 for CRC-16, 4 for CRC-32C).
    pub fn value_size(&self) -> usize {
        match self {
            Crc::None => 0,
            Crc::Crc16(_) => 2,
            Crc::Crc32c(_) => 4,
        }
    }

    /// Write CRC value bytes into `buf`. Returns number of bytes written.
    pub fn write_value(&self, buf: &mut [u8]) -> usize {
        match self {
            Crc::None => 0,
            Crc::Crc16(v) => {
                buf[..2].copy_from_slice(&v.to_be_bytes());
                2
            }
            Crc::Crc32c(v) => {
                buf[..4].copy_from_slice(&v.to_be_bytes());
                4
            }
        }
    }
}

/// Incremental CRC hasher for streaming computation.
pub enum CrcHasher {
    Crc16(crc::Digest<'static, u16>),
    Crc32c(crc::Digest<'static, u32>),
}

impl CrcHasher {
    /// Create a hasher matching the given CRC type. Returns `None` for `Crc::None`.
    pub fn new(crc: &Crc) -> Option<Self> {
        match crc {
            Crc::None => None,
            Crc::Crc16(_) => Some(CrcHasher::Crc16(CRC16.digest())),
            Crc::Crc32c(_) => Some(CrcHasher::Crc32c(CRC32C.digest())),
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        match self {
            CrcHasher::Crc16(d) => d.update(data),
            CrcHasher::Crc32c(d) => d.update(data),
        }
    }

    pub fn finalize(self) -> Crc {
        match self {
            CrcHasher::Crc16(d) => Crc::Crc16(d.finalize()),
            CrcHasher::Crc32c(d) => Crc::Crc32c(d.finalize()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc16_check_value() {
        assert_eq!(Crc::compute_crc16(b"123456789"), 0x906E);
    }

    #[test]
    fn crc32c_check_value() {
        assert_eq!(Crc::compute_crc32c(b"123456789"), 0xE3069283);
    }

    #[test]
    fn crc_type_codes() {
        assert_eq!(Crc::None.crc_type(), 0);
        assert_eq!(Crc::crc16().crc_type(), 1);
        assert_eq!(Crc::crc32c().crc_type(), 2);
    }

    #[test]
    fn compute_by_type() {
        let data = b"hello";
        let crc16 = Crc::compute(1, data).unwrap();
        assert_eq!(crc16, Crc::Crc16(Crc::compute_crc16(data)));

        let crc32 = Crc::compute(2, data).unwrap();
        assert_eq!(crc32, Crc::Crc32c(Crc::compute_crc32c(data)));

        assert_eq!(Crc::compute(0, data).unwrap(), Crc::None);
        assert!(Crc::compute(3, data).is_err());
    }

    #[test]
    fn incremental_crc16() {
        let mut hasher = CrcHasher::new(&Crc::crc16()).unwrap();
        hasher.update(b"12345");
        hasher.update(b"6789");
        assert_eq!(hasher.finalize(), Crc::Crc16(0x906E));
    }

    #[test]
    fn incremental_crc32c() {
        let mut hasher = CrcHasher::new(&Crc::crc32c()).unwrap();
        hasher.update(b"123");
        hasher.update(b"456");
        hasher.update(b"789");
        assert_eq!(hasher.finalize(), Crc::Crc32c(0xE3069283));
    }

    #[test]
    fn decode_buf_crc16() {
        let mut enc = aqueduct_cbor::Encoder::new();
        enc.write_bstr(&0x906E_u16.to_be_bytes());
        let mut dec = Decoder::new(enc.as_bytes());
        let crc = Crc::decode_buf(&mut dec, 1).unwrap();
        assert_eq!(crc, Crc::Crc16(0x906E));
    }

    #[test]
    fn decode_buf_crc32c() {
        let mut enc = aqueduct_cbor::Encoder::new();
        enc.write_bstr(&0xE3069283_u32.to_be_bytes());
        let mut dec = Decoder::new(enc.as_bytes());
        let crc = Crc::decode_buf(&mut dec, 2).unwrap();
        assert_eq!(crc, Crc::Crc32c(0xE3069283));
    }
}
