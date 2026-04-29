use std::io::Read;

use aqueduct_cbor::{Encoder, StreamDecoder, ToCbor};

use crate::bundle::crc::Crc;
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
    pub(crate) fn decode<R: Read>(dec: &mut StreamDecoder<R>) -> Result<Self, Error> {
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
        let dest_eid = Eid::decode(dec)?;
        let src_node_id = Eid::decode(dec)?;
        let rpt_eid = Eid::decode(dec)?;

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
            Crc::decode(dec, crc_type)?
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
