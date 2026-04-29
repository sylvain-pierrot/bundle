use std::io::Read;

use aqueduct_cbor::{Encoder, StreamDecoder, ToCbor, UintOrString};

use crate::error::Error;

/// Endpoint identifier (RFC 9171 §4.2.5, updated by RFC 9758).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Eid {
    Null,
    Dtn(String),
    Ipn {
        allocator_id: u32,
        node_number: u32,
        service_number: u64,
    },
}

impl Eid {
    #[inline]
    pub fn is_null(&self) -> bool {
        matches!(
            self,
            Eid::Null
                | Eid::Ipn {
                    allocator_id: 0,
                    node_number: 0,
                    service_number: 0,
                }
        )
    }

    pub(crate) fn decode<R: Read>(dec: &mut StreamDecoder<R>) -> Result<Self, Error> {
        let len = dec.read_array_len()?;
        if len != 2 {
            return Err(Error::InvalidEid);
        }
        let scheme = dec.read_uint()?;
        match scheme {
            1 => match dec.read_uint_or_tstr()? {
                UintOrString::Uint(0) => Ok(Eid::Null),
                UintOrString::Uint(_) => Err(Error::InvalidEid),
                UintOrString::Tstr(s) => Ok(Eid::Dtn(s)),
            },
            2 => {
                let inner_len = dec.read_array_len()?;
                match inner_len {
                    2 => Ok(decode_ipn_2elem(dec.read_uint()?, dec.read_uint()?)),
                    3 => decode_ipn_3elem(dec.read_uint()?, dec.read_uint()?, dec.read_uint()?),
                    _ => Err(Error::InvalidEid),
                }
            }
            _ => Err(Error::InvalidEidScheme(scheme)),
        }
    }
}

pub(crate) fn decode_ipn_2elem(fqnn: u64, service_number: u64) -> Eid {
    let allocator_id = (fqnn >> 32) as u32;
    let node_number = fqnn as u32;
    if allocator_id == 0 && node_number == 0 && service_number == 0 {
        Eid::Null
    } else {
        Eid::Ipn {
            allocator_id,
            node_number,
            service_number,
        }
    }
}

pub(crate) fn decode_ipn_3elem(
    raw_alloc: u64,
    raw_node: u64,
    service_number: u64,
) -> Result<Eid, Error> {
    let allocator_id = u32::try_from(raw_alloc).map_err(|_| Error::IntegerOverflow)?;
    let node_number = u32::try_from(raw_node).map_err(|_| Error::IntegerOverflow)?;
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

impl ToCbor for Eid {
    fn encode(&self, enc: &mut Encoder) {
        match self {
            Eid::Null => {
                enc.write_array(2);
                enc.write_uint(1);
                enc.write_uint(0);
            }
            Eid::Dtn(s) => {
                enc.write_array(2);
                enc.write_uint(1);
                enc.write_tstr(s);
            }
            Eid::Ipn {
                allocator_id,
                node_number,
                service_number,
            } => {
                enc.write_array(2);
                enc.write_uint(2);
                if *allocator_id == 0 {
                    enc.write_array(2);
                    enc.write_uint(*node_number as u64);
                    enc.write_uint(*service_number);
                } else {
                    enc.write_array(3);
                    enc.write_uint(*allocator_id as u64);
                    enc.write_uint(*node_number as u64);
                    enc.write_uint(*service_number);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_null() {
        let mut enc = Encoder::new();
        Eid::Null.encode(&mut enc);
        let mut dec = StreamDecoder::new(enc.as_bytes());
        assert_eq!(Eid::decode(&mut dec).unwrap(), Eid::Null);
    }

    #[test]
    fn roundtrip_dtn() {
        let eid = Eid::Dtn("//node1/incoming".into());
        let mut enc = Encoder::new();
        eid.encode(&mut enc);
        let mut dec = StreamDecoder::new(enc.as_bytes());
        assert_eq!(Eid::decode(&mut dec).unwrap(), eid);
    }

    #[test]
    fn roundtrip_ipn_default_allocator() {
        let eid = Eid::Ipn {
            allocator_id: 0,
            node_number: 42,
            service_number: 7,
        };
        let mut enc = Encoder::new();
        eid.encode(&mut enc);
        let mut dec = StreamDecoder::new(enc.as_bytes());
        assert_eq!(Eid::decode(&mut dec).unwrap(), eid);
    }

    #[test]
    fn roundtrip_ipn_with_allocator() {
        let eid = Eid::Ipn {
            allocator_id: 977000,
            node_number: 100,
            service_number: 1,
        };
        let mut enc = Encoder::new();
        eid.encode(&mut enc);
        let mut dec = StreamDecoder::new(enc.as_bytes());
        assert_eq!(Eid::decode(&mut dec).unwrap(), eid);
    }

    #[test]
    fn ipn_null_is_null() {
        assert!(
            Eid::Ipn {
                allocator_id: 0,
                node_number: 0,
                service_number: 0,
            }
            .is_null()
        );
    }

    #[test]
    fn decode_two_element_with_allocator() {
        let mut enc = Encoder::new();
        enc.write_array(2);
        enc.write_uint(2);
        enc.write_array(2);
        enc.write_uint(0x000EE868_00000064);
        enc.write_uint(1);

        let mut dec = StreamDecoder::new(enc.as_bytes());
        assert_eq!(
            Eid::decode(&mut dec).unwrap(),
            Eid::Ipn {
                allocator_id: 977000,
                node_number: 100,
                service_number: 1,
            }
        );
    }

    #[test]
    fn ipn_null_decoded_from_cbor() {
        let mut enc = Encoder::new();
        enc.write_array(2);
        enc.write_uint(2);
        enc.write_array(2);
        enc.write_uint(0);
        enc.write_uint(0);

        let mut dec = StreamDecoder::new(enc.as_bytes());
        assert_eq!(Eid::decode(&mut dec).unwrap(), Eid::Null);
    }
}
