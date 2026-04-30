use aqueduct_cbor::{Decoder, Encoder, ToCbor};

use crate::crc::Crc;
use crate::eid::Eid;
use crate::error::Error;

/// Bundle processing control flags (RFC 9171 §4.2.3).
///
/// Stored as the raw CBOR unsigned integer — no parsing overhead.
/// Bits are extracted on access via inline methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BundleFlags(u64);

const REPORT_MASK: u64 = 0x004000 | 0x010000 | 0x020000 | 0x040000;

impl BundleFlags {
    #[inline]
    pub fn from_bits(bits: u64) -> Self {
        Self(bits)
    }
    #[inline]
    pub fn bits(self) -> u64 {
        self.0
    }
    #[inline]
    pub fn is_fragment(self) -> bool {
        self.0 & 0x000001 != 0
    }
    #[inline]
    pub fn is_admin(self) -> bool {
        self.0 & 0x000002 != 0
    }
    #[inline]
    pub fn no_fragment(self) -> bool {
        self.0 & 0x000004 != 0
    }
    #[inline]
    pub fn ack_requested(self) -> bool {
        self.0 & 0x000020 != 0
    }
    #[inline]
    pub fn time_in_reports(self) -> bool {
        self.0 & 0x000040 != 0
    }
    #[inline]
    pub fn rpt_reception(self) -> bool {
        self.0 & 0x004000 != 0
    }
    #[inline]
    pub fn rpt_forwarding(self) -> bool {
        self.0 & 0x010000 != 0
    }
    #[inline]
    pub fn rpt_delivery(self) -> bool {
        self.0 & 0x020000 != 0
    }
    #[inline]
    pub fn rpt_deletion(self) -> bool {
        self.0 & 0x040000 != 0
    }
    #[inline]
    pub fn any_report(self) -> bool {
        self.0 & REPORT_MASK != 0
    }

    pub fn validate(self, src_is_null: bool) -> Result<(), Error> {
        if self.is_admin() && self.any_report() {
            return Err(Error::InvalidFlags);
        }
        if src_is_null && (!self.no_fragment() || self.any_report()) {
            return Err(Error::InvalidFlags);
        }
        Ok(())
    }
}

/// Creation timestamp (RFC 9171 §4.2.7).
///
/// Two unsigned integers: DTN time of creation and sequence number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreationTimestamp {
    pub time: u64,
    pub seq: u64,
}

/// Fragment metadata, present only when `BundleFlags::is_fragment()` is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FragmentInfo {
    pub offset: u64,
    pub total_adu_len: u64,
}

/// Primary bundle block (RFC 9171 §4.3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimaryBlock {
    pub version: u8,
    pub flags: BundleFlags,
    pub crc: Crc,
    pub dest_eid: Eid,
    pub src_node_id: Eid,
    pub rpt_eid: Eid,
    pub creation_ts: CreationTimestamp,
    pub lifetime: u64,
    pub fragment: Option<FragmentInfo>,
}

impl PrimaryBlock {
    /// Decode from a buffer-based CBOR decoder.
    pub fn decode_buf(dec: &mut Decoder<'_>) -> Result<Self, Error> {
        let len = dec.read_array_len()?;
        if !(8..=11).contains(&len) {
            return Err(Error::InvalidBlockLength {
                expected: "8-11",
                actual: len,
            });
        }

        let version = dec.read_uint()? as u8;
        let flags = BundleFlags::from_bits(dec.read_uint()?);
        let crc_type = dec.read_uint()?;
        let dest_eid = Eid::decode_buf(dec)?;
        let src_node_id = Eid::decode_buf(dec)?;
        let rpt_eid = Eid::decode_buf(dec)?;

        let ts_len = dec.read_array_len()?;
        if ts_len != 2 {
            return Err(Error::InvalidBlockLength {
                expected: "2",
                actual: ts_len,
            });
        }
        let creation_ts = CreationTimestamp {
            time: dec.read_uint()?,
            seq: dec.read_uint()?,
        };

        let lifetime = dec.read_uint()?;

        let has_crc = crc_type != 0;
        let has_fragment = match (len, has_crc) {
            (8, false) | (9, true) => false,
            (10, false) | (11, true) => true,
            _ => {
                return Err(Error::InvalidBlockLength {
                    expected: "8-11",
                    actual: len,
                });
            }
        };

        let fragment = if has_fragment {
            Some(FragmentInfo {
                offset: dec.read_uint()?,
                total_adu_len: dec.read_uint()?,
            })
        } else {
            None
        };

        let crc = if has_crc {
            Crc::decode_buf(dec, crc_type)?
        } else {
            Crc::None
        };

        Ok(PrimaryBlock {
            version,
            flags,
            crc,
            dest_eid,
            src_node_id,
            rpt_eid,
            creation_ts,
            lifetime,
            fragment,
        })
    }

    /// Decode from a streaming CBOR decoder.
    pub fn decode_stream<R: aqueduct_cbor::Read>(
        dec: &mut aqueduct_cbor::StreamDecoder<R>,
    ) -> Result<Self, Error> {
        let len = dec.read_array_len()?;
        if !(8..=11).contains(&len) {
            return Err(Error::InvalidBlockLength {
                expected: "8-11",
                actual: len,
            });
        }

        let version = dec.read_uint()? as u8;
        let flags = BundleFlags::from_bits(dec.read_uint()?);
        let crc_type = dec.read_uint()?;
        let dest_eid = Eid::decode_stream(dec)?;
        let src_node_id = Eid::decode_stream(dec)?;
        let rpt_eid = Eid::decode_stream(dec)?;

        let ts_len = dec.read_array_len()?;
        if ts_len != 2 {
            return Err(Error::InvalidBlockLength {
                expected: "2",
                actual: ts_len,
            });
        }
        let creation_ts = CreationTimestamp {
            time: dec.read_uint()?,
            seq: dec.read_uint()?,
        };

        let lifetime = dec.read_uint()?;

        let has_crc = crc_type != 0;
        let has_fragment = match (len, has_crc) {
            (8, false) | (9, true) => false,
            (10, false) | (11, true) => true,
            _ => {
                return Err(Error::InvalidBlockLength {
                    expected: "8-11",
                    actual: len,
                });
            }
        };

        let fragment = if has_fragment {
            Some(FragmentInfo {
                offset: dec.read_uint()?,
                total_adu_len: dec.read_uint()?,
            })
        } else {
            None
        };

        let crc = if has_crc {
            Crc::decode_stream(dec, crc_type)?
        } else {
            Crc::None
        };

        Ok(PrimaryBlock {
            version,
            flags,
            crc,
            dest_eid,
            src_node_id,
            rpt_eid,
            creation_ts,
            lifetime,
            fragment,
        })
    }

    pub fn verify_crc(&self) -> Result<(), Error> {
        if self.crc.is_none() {
            return Ok(());
        }
        let mut enc = Encoder::with_capacity(128);
        self.encode(&mut enc);
        let bytes = enc.as_bytes();
        let crc_size = self.crc.value_size();
        let crc_data_offset = bytes.len() - crc_size;
        self.crc.verify(bytes, crc_data_offset)
    }

    pub fn validate(&self) -> Result<(), Error> {
        if self.version != 7 {
            return Err(Error::UnsupportedVersion(self.version));
        }
        self.flags.validate(self.src_node_id.is_null())?;

        if self.flags.is_fragment() != self.fragment.is_some() {
            return Err(Error::InvalidFlags);
        }

        Ok(())
    }
}

impl ToCbor for PrimaryBlock {
    fn encode(&self, enc: &mut Encoder) {
        let has_crc = !self.crc.is_none();
        let has_frag = self.fragment.is_some();
        let len = 8 + if has_frag { 2 } else { 0 } + if has_crc { 1 } else { 0 };

        let block_start = enc.position();
        enc.write_array(len);
        enc.write_uint(self.version as u64);
        enc.write_uint(self.flags.bits());
        enc.write_uint(self.crc.crc_type());
        self.dest_eid.encode(enc);
        self.src_node_id.encode(enc);
        self.rpt_eid.encode(enc);
        enc.write_array(2);
        enc.write_uint(self.creation_ts.time);
        enc.write_uint(self.creation_ts.seq);
        enc.write_uint(self.lifetime);

        if let Some(frag) = &self.fragment {
            enc.write_uint(frag.offset);
            enc.write_uint(frag.total_adu_len);
        }

        self.crc.encode_and_finalize(enc, block_start);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_primary() -> PrimaryBlock {
        PrimaryBlock {
            version: 7,
            flags: BundleFlags::from_bits(0),
            crc: Crc::None,
            dest_eid: Eid::Ipn {
                allocator_id: 0,
                node_number: 2,
                service_number: 1,
            },
            src_node_id: Eid::Ipn {
                allocator_id: 0,
                node_number: 1,
                service_number: 0,
            },
            rpt_eid: Eid::Null,
            creation_ts: CreationTimestamp { time: 1000, seq: 1 },
            lifetime: 3600,
            fragment: None,
        }
    }

    #[test]
    fn roundtrip_primary_no_crc() {
        let primary = sample_primary();
        let mut enc = Encoder::new();
        primary.encode(&mut enc);
        let mut dec = Decoder::new(enc.as_bytes());
        let decoded = PrimaryBlock::decode_buf(&mut dec).unwrap();
        assert_eq!(decoded, primary);
    }

    #[test]
    fn roundtrip_primary_with_crc16() {
        let mut primary = sample_primary();
        primary.crc = Crc::crc16();
        let mut enc = Encoder::new();
        primary.encode(&mut enc);
        let mut dec = Decoder::new(enc.as_bytes());
        let decoded = PrimaryBlock::decode_buf(&mut dec).unwrap();
        decoded.verify_crc().unwrap();
    }

    #[test]
    fn roundtrip_primary_with_fragment() {
        let mut primary = sample_primary();
        primary.flags = BundleFlags::from_bits(0x000001); // is_fragment
        primary.fragment = Some(FragmentInfo {
            offset: 0,
            total_adu_len: 1024,
        });
        let mut enc = Encoder::new();
        primary.encode(&mut enc);
        let mut dec = Decoder::new(enc.as_bytes());
        let decoded = PrimaryBlock::decode_buf(&mut dec).unwrap();
        assert_eq!(decoded, primary);
    }

    #[test]
    fn validate_version() {
        let mut primary = sample_primary();
        primary.version = 6;
        assert!(primary.validate().is_err());
    }
}
