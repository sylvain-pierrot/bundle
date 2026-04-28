use crate::eid::Eid;
use crate::error::Error;
use crate::extension::Extension;

/// Previous Node Block (RFC 9171 §4.4.1), block type 6.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviousNode<'a> {
    pub node_id: Eid<'a>,
}

impl Extension for PreviousNode<'_> {
    const BLOCK_TYPE: u64 = 6;

    fn parse(_data: &[u8]) -> Result<Self, Error> {
        // TODO: delegate to external CBOR decoder
        todo!("CBOR decode for PreviousNode")
    }
}
