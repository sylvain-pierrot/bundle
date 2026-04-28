use aqueduct_cbor::{Decoder, Encoder};
use crc::{CRC_16_IBM_SDLC, CRC_32_ISCSI, Crc as CrcAlgo};

use crate::error::Error;

const CRC16: CrcAlgo<u16> = CrcAlgo::<u16>::new(&CRC_16_IBM_SDLC);
const CRC32C: CrcAlgo<u32> = CrcAlgo::<u32>::new(&CRC_32_ISCSI);

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
    /// `block_bytes` is the full serialized block.
    /// `crc_data_offset` is the byte offset of the CRC value bytes within
    /// the block (after the CBOR bstr header).
    pub fn verify(&self, block_bytes: &[u8], crc_data_offset: usize) -> Result<(), Error> {
        let valid = match self {
            Crc::None => return Ok(()),
            Crc::Crc16(expected) => {
                let mut buf = block_bytes.to_vec();
                buf.get_mut(crc_data_offset..crc_data_offset + 2)
                    .ok_or(Error::InvalidCbor)?
                    .fill(0);
                Self::compute_crc16(&buf) == *expected
            }
            Crc::Crc32c(expected) => {
                let mut buf = block_bytes.to_vec();
                buf.get_mut(crc_data_offset..crc_data_offset + 4)
                    .ok_or(Error::InvalidCbor)?
                    .fill(0);
                Self::compute_crc32c(&buf) == *expected
            }
        };
        if valid {
            Ok(())
        } else {
            Err(Error::CrcMismatch)
        }
    }

    /// Decode a CRC value byte string from CBOR.
    pub fn decode_value(dec: &mut Decoder, crc_type: u64) -> Result<Self, Error> {
        let crc_bstr = dec.read_bstr()?;
        match crc_type {
            1 => {
                if crc_bstr.len() != 2 {
                    return Err(Error::InvalidCbor);
                }
                Ok(Crc::Crc16(u16::from_be_bytes([crc_bstr[0], crc_bstr[1]])))
            }
            2 => {
                if crc_bstr.len() != 4 {
                    return Err(Error::InvalidCbor);
                }
                Ok(Crc::Crc32c(u32::from_be_bytes([
                    crc_bstr[0],
                    crc_bstr[1],
                    crc_bstr[2],
                    crc_bstr[3],
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc16_check_value() {
        // CRC-16/IBM-SDLC (X-25) check value for "123456789"
        assert_eq!(Crc::compute_crc16(b"123456789"), 0x906E);
    }

    #[test]
    fn crc32c_check_value() {
        // CRC-32C (Castagnoli) check value for "123456789"
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
}
