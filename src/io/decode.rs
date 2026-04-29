//! Streaming CBOR decode helpers for BPv7 types.

use std::borrow::Cow;
use std::io::Read;

use aqueduct_cbor::{StreamDecoder, UintOrString};

use crate::bundle::canonical::{BlockFlags, CanonicalBlock};
use crate::bundle::crc::Crc;
use crate::bundle::primary::{BundleFlags, CreationTimestamp, FragmentInfo, PrimaryBlock};
use crate::eid::Eid;
use crate::error::Error;

pub(crate) fn decode_eid<R: Read>(dec: &mut StreamDecoder<R>) -> Result<Eid<'static>, Error> {
    let len = dec.read_array_len()?;
    if len != 2 {
        return Err(Error::InvalidEid);
    }
    let scheme = dec.read_uint()?;
    match scheme {
        1 => match dec.read_uint_or_tstr()? {
            UintOrString::Uint(0) => Ok(Eid::Null),
            UintOrString::Uint(_) => Err(Error::InvalidEid),
            UintOrString::Tstr(s) => Ok(Eid::Dtn(Cow::Owned(s))),
        },
        2 => {
            let inner_len = dec.read_array_len()?;
            match inner_len {
                2 => {
                    let fqnn = dec.read_uint()?;
                    let service_number = dec.read_uint()?;
                    let allocator_id = (fqnn >> 32) as u32;
                    let node_number = fqnn as u32;
                    if allocator_id == 0 && node_number == 0 && service_number == 0 {
                        Ok(Eid::Null)
                    } else {
                        Ok(Eid::Ipn {
                            allocator_id,
                            node_number,
                            service_number,
                        })
                    }
                }
                3 => {
                    let raw_alloc = dec.read_uint()?;
                    let raw_node = dec.read_uint()?;
                    let allocator_id = u32::try_from(raw_alloc).map_err(|_| Error::EidOverflow)?;
                    let node_number = u32::try_from(raw_node).map_err(|_| Error::EidOverflow)?;
                    let service_number = dec.read_uint()?;
                    if allocator_id == 0 && node_number == 0 && service_number == 0 {
                        Ok(Eid::Null)
                    } else {
                        Ok(Eid::Ipn {
                            allocator_id,
                            node_number,
                            service_number,
                        })
                    }
                }
                _ => Err(Error::InvalidEid),
            }
        }
        _ => Err(Error::InvalidEidScheme(scheme)),
    }
}

pub(crate) fn decode_primary<R: Read>(
    dec: &mut StreamDecoder<R>,
) -> Result<PrimaryBlock<'static>, Error> {
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
    let dest_eid = decode_eid(dec)?;
    let src_node_id = decode_eid(dec)?;
    let rpt_eid = decode_eid(dec)?;

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
        decode_crc_value(dec, crc_type)?
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

pub(crate) fn decode_canonical_body<R: Read>(
    dec: &mut StreamDecoder<R>,
    block_type: u64,
    array_len: usize,
) -> Result<CanonicalBlock, Error> {
    if array_len != 5 && array_len != 6 {
        return Err(Error::InvalidBlockLength {
            expected: "5-6",
            actual: array_len,
        });
    }
    let block_number = dec.read_uint()?;
    let flags = BlockFlags::from_bits(dec.read_uint()?);
    let crc_type = dec.read_uint()?;
    let data = dec.read_bstr()?;

    let crc = if crc_type != 0 {
        decode_crc_value(dec, crc_type)?
    } else {
        Crc::None
    };

    Ok(CanonicalBlock {
        block_type,
        block_number,
        flags,
        crc,
        data,
    })
}

pub(crate) fn decode_crc_value<R: Read>(
    dec: &mut StreamDecoder<R>,
    crc_type: u64,
) -> Result<Crc, Error> {
    let bytes = dec.read_bstr()?;
    Crc::from_bytes(crc_type, &bytes)
}
