use alloc::vec::Vec;

use bundle_cbor::{Encoder, ToCbor};

use crate::eid::Eid;
use crate::error::Error;
use crate::extension::Extension;

/// Previous Node Block (RFC 9171 §4.4.1), block type 6.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviousNode {
    pub node_id: Eid,
}

impl Extension for PreviousNode {
    const BLOCK_TYPE: u64 = 6;

    fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut dec = bundle_cbor::Decoder::new(data);
        let node_id = Eid::decode_buf(&mut dec)?;
        Ok(PreviousNode { node_id })
    }

    fn encode_data(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        self.node_id.encode(&mut enc);
        enc.into_bytes()
    }
}
