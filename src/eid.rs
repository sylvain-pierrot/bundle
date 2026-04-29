use std::borrow::Cow;

use aqueduct_cbor::{Decoder, Encoder, FromCbor, ToCbor, UintOrTstr};

use crate::error::Error;

/// Endpoint identifier (RFC 9171 §4.2.5, updated by RFC 9758).
///
/// Uses `Cow<str>` for the DTN scheme: zero-copy when borrowed from an input
/// buffer, owned when constructed or parsed from a stream.
///
/// The IPN variant carries an allocator identifier, node number, and service
/// number per RFC 9758 §3. An IPN EID with all three components set to zero
/// is the null endpoint, equivalent to `dtn:none`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Eid<'a> {
    Null,
    Dtn(Cow<'a, str>),
    Ipn {
        allocator_id: u32,
        node_number: u32,
        service_number: u64,
    },
}

impl Eid<'_> {
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

    /// Convert to an owned `Eid<'static>` by cloning any borrowed data.
    pub fn into_owned(self) -> Eid<'static> {
        match self {
            Eid::Null => Eid::Null,
            Eid::Dtn(s) => Eid::Dtn(Cow::Owned(s.into_owned())),
            Eid::Ipn {
                allocator_id,
                node_number,
                service_number,
            } => Eid::Ipn {
                allocator_id,
                node_number,
                service_number,
            },
        }
    }
}

impl<'a> FromCbor<'a> for Eid<'a> {
    type Error = Error;

    fn decode(dec: &mut Decoder<'a>) -> Result<Self, Self::Error> {
        let len = dec.read_array_len()?;
        if len != 2 {
            return Err(Error::InvalidCbor);
        }
        let scheme = dec.read_uint()?;
        match scheme {
            1 => {
                // dtn scheme: SSP is 0 for dtn:none, or a text string
                match dec.read_uint_or_tstr()? {
                    UintOrTstr::Uint(0) => Ok(Eid::Null),
                    UintOrTstr::Uint(_) => Err(Error::InvalidCbor),
                    UintOrTstr::Tstr(s) => Ok(Eid::Dtn(Cow::Borrowed(s))),
                }
            }
            2 => {
                // ipn scheme (RFC 9758 §6): SSP is 2-element or 3-element array
                let inner_len = dec.read_array_len()?;
                match inner_len {
                    2 => {
                        // Two-element: [fqnn: u64, service_number: u64]
                        // fqnn = (allocator_id << 32) | node_number
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
                        // Three-element: [allocator_id, node_number, service_number]
                        let raw_alloc = dec.read_uint()?;
                        let raw_node = dec.read_uint()?;
                        let allocator_id =
                            u32::try_from(raw_alloc).map_err(|_| Error::InvalidCbor)?;
                        let node_number =
                            u32::try_from(raw_node).map_err(|_| Error::InvalidCbor)?;
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
                    _ => Err(Error::InvalidCbor),
                }
            }
            _ => Err(Error::InvalidEidScheme(scheme)),
        }
    }
}

impl ToCbor for Eid<'_> {
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
                    // Default allocator: use two-element encoding (more compact)
                    enc.write_array(2);
                    enc.write_uint(*node_number as u64);
                    enc.write_uint(*service_number);
                } else {
                    // Non-default allocator: use three-element encoding
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
        let mut dec = Decoder::new(enc.as_bytes());
        assert_eq!(Eid::decode(&mut dec).unwrap(), Eid::Null);
    }

    #[test]
    fn roundtrip_dtn() {
        let eid = Eid::Dtn(Cow::Borrowed("//node1/incoming"));
        let mut enc = Encoder::new();
        eid.encode(&mut enc);
        let mut dec = Decoder::new(enc.as_bytes());
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
        let mut dec = Decoder::new(enc.as_bytes());
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
        let mut dec = Decoder::new(enc.as_bytes());
        assert_eq!(Eid::decode(&mut dec).unwrap(), eid);
    }

    #[test]
    fn ipn_null_is_null() {
        let eid = Eid::Ipn {
            allocator_id: 0,
            node_number: 0,
            service_number: 0,
        };
        assert!(eid.is_null());
    }

    #[test]
    fn decode_two_element_with_allocator() {
        // Two-element encoding of ipn:977000.100.1
        // fqnn = (977000 << 32) | 100 = 0x000EE868_00000064
        let mut enc = Encoder::new();
        enc.write_array(2);
        enc.write_uint(2); // ipn scheme
        enc.write_array(2);
        enc.write_uint(0x000EE868_00000064); // fqnn
        enc.write_uint(1); // service

        let mut dec = Decoder::new(enc.as_bytes());
        let eid = Eid::decode(&mut dec).unwrap();
        assert_eq!(
            eid,
            Eid::Ipn {
                allocator_id: 977000,
                node_number: 100,
                service_number: 1,
            }
        );
    }

    #[test]
    fn ipn_null_decoded_from_cbor() {
        // ipn:0.0.0 encoded as two-element [0, 0] should decode as Null
        let mut enc = Encoder::new();
        enc.write_array(2);
        enc.write_uint(2);
        enc.write_array(2);
        enc.write_uint(0);
        enc.write_uint(0);

        let mut dec = Decoder::new(enc.as_bytes());
        assert_eq!(Eid::decode(&mut dec).unwrap(), Eid::Null);
    }
}
