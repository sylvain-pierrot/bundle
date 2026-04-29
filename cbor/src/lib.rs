//! Minimal CBOR codec for the BPv7 subset.
//!
//! Buffer-based ([`Decoder`]/[`Encoder`]) for in-memory bundles.
//! Streaming ([`StreamDecoder`]/[`StreamEncoder`]) for any I/O source.
//! Both available in `no_std` environments.

#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

mod consts;
mod decode;
mod encode;
mod error;
mod io;
mod stream;

pub use decode::{Decoder, UintOrTstr};
pub use encode::Encoder;
pub use error::Error;
pub use io::{CborRead, CborWrite};
pub use stream::{StreamDecoder, StreamEncoder, UintOrString};

/// Decode a value from a CBOR [`Decoder`].
pub trait FromCbor<'a>: Sized {
    type Error;
    fn decode(dec: &mut Decoder<'a>) -> Result<Self, Self::Error>;
}

/// Encode a value to a CBOR [`Encoder`].
pub trait ToCbor {
    fn encode(&self, enc: &mut Encoder);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_uint() {
        for v in [
            0u64,
            1,
            23,
            24,
            255,
            256,
            65535,
            65536,
            0xFFFF_FFFF,
            u64::MAX,
        ] {
            let mut enc = Encoder::new();
            enc.write_uint(v);
            let mut dec = Decoder::new(enc.as_bytes());
            assert_eq!(dec.read_uint().unwrap(), v, "failed for {v}");
        }
    }

    #[test]
    fn roundtrip_bstr() {
        let data = b"hello world";
        let mut enc = Encoder::new();
        enc.write_bstr(data);
        let mut dec = Decoder::new(enc.as_bytes());
        assert_eq!(dec.read_bstr().unwrap(), data);
    }

    #[test]
    fn roundtrip_tstr() {
        let s = "dtn://node1/svc";
        let mut enc = Encoder::new();
        enc.write_tstr(s);
        let mut dec = Decoder::new(enc.as_bytes());
        assert_eq!(dec.read_tstr().unwrap(), s);
    }

    #[test]
    fn roundtrip_array() {
        let mut enc = Encoder::new();
        enc.write_array(3);
        enc.write_uint(1);
        enc.write_uint(2);
        enc.write_uint(3);
        let mut dec = Decoder::new(enc.as_bytes());
        assert_eq!(dec.read_array_len().unwrap(), 3);
        assert_eq!(dec.read_uint().unwrap(), 1);
        assert_eq!(dec.read_uint().unwrap(), 2);
        assert_eq!(dec.read_uint().unwrap(), 3);
    }

    #[test]
    fn indefinite_array() {
        let mut enc = Encoder::new();
        enc.write_indefinite_array();
        enc.write_uint(42);
        enc.write_break();
        let mut dec = Decoder::new(enc.as_bytes());
        dec.read_indefinite_array_start().unwrap();
        assert!(!dec.is_break().unwrap());
        assert_eq!(dec.read_uint().unwrap(), 42);
        assert!(dec.is_break().unwrap());
        dec.read_break().unwrap();
    }
}
