//! Builder for constructing bundles with sensible defaults.

use crate::bundle::Bundle;
use crate::bundle::canonical::{
    BlockData, BlockFlags, CanonicalBlock, PAYLOAD_BLOCK_NUMBER, PAYLOAD_BLOCK_TYPE,
};
use crate::bundle::crc::Crc;
use crate::bundle::primary::{BundleFlags, CreationTimestamp, FragmentInfo, PrimaryBlock};
use crate::eid::Eid;
use crate::io::retention::Retention;

/// Fluent builder for [`Bundle`].
pub struct BundleBuilder<S> {
    dest_eid: Eid<'static>,
    src_node_id: Eid<'static>,
    lifetime: u64,
    payload_len: u64,
    bundle_flags: u64,
    rpt_eid: Eid<'static>,
    creation_ts: CreationTimestamp,
    fragment: Option<FragmentInfo>,
    blocks: Vec<CanonicalBlock>,
    retention: S,
}

impl<S: Retention> BundleBuilder<S> {
    pub(crate) fn new(
        dest_eid: Eid<'static>,
        src_node_id: Eid<'static>,
        lifetime: u64,
        payload: &[u8],
        mut retention: S,
    ) -> Result<Self, crate::error::Error> {
        let payload_len = payload.len() as u64;
        retention
            .write_all(payload)
            .map_err(aqueduct_cbor::Error::from)?;
        Ok(Self {
            dest_eid,
            src_node_id,
            lifetime,
            payload_len,
            bundle_flags: 0,
            rpt_eid: Eid::Null,
            creation_ts: CreationTimestamp { time: 0, seq: 0 },
            fragment: None,
            blocks: Vec::new(),
            retention,
        })
    }

    pub fn is_admin_record(mut self) -> Self {
        self.bundle_flags |= 0x000002;
        self
    }

    pub fn must_not_fragment(mut self) -> Self {
        self.bundle_flags |= 0x000004;
        self
    }

    pub fn request_ack(mut self) -> Self {
        self.bundle_flags |= 0x000020;
        self
    }

    pub fn report_reception(mut self) -> Self {
        self.bundle_flags |= 0x004000;
        self
    }

    pub fn report_forwarding(mut self) -> Self {
        self.bundle_flags |= 0x010000;
        self
    }

    pub fn report_delivery(mut self) -> Self {
        self.bundle_flags |= 0x020000;
        self
    }

    pub fn report_deletion(mut self) -> Self {
        self.bundle_flags |= 0x040000;
        self
    }

    pub fn report_to(mut self, eid: Eid<'_>) -> Self {
        self.rpt_eid = eid.into_owned();
        self
    }

    pub fn creation_ts(mut self, ts: CreationTimestamp) -> Self {
        self.creation_ts = ts;
        self
    }

    pub fn fragment(mut self, offset: u64, total_adu_len: u64) -> Self {
        self.bundle_flags |= 0x000001;
        self.fragment = Some(FragmentInfo {
            offset,
            total_adu_len,
        });
        self
    }

    pub fn extension(mut self, block: CanonicalBlock) -> Self {
        self.blocks.push(block);
        self
    }

    pub fn build(mut self) -> Bundle<S> {
        // RFC 9171: null source requires no_fragment flag
        if self.src_node_id.is_null() {
            self.bundle_flags |= 0x000004;
        }

        // Add the payload block
        self.blocks.push(CanonicalBlock {
            block_type: PAYLOAD_BLOCK_TYPE,
            block_number: PAYLOAD_BLOCK_NUMBER,
            flags: BlockFlags::from_bits(0),
            crc: Crc::None,
            data: BlockData::Retained {
                offset: 0,
                len: self.payload_len,
            },
        });

        Bundle::from_parts(
            PrimaryBlock {
                version: 7,
                flags: BundleFlags::from_bits(self.bundle_flags),
                crc: Crc::crc32c(),
                dest_eid: self.dest_eid,
                src_node_id: self.src_node_id,
                rpt_eid: self.rpt_eid,
                creation_ts: self.creation_ts,
                lifetime: self.lifetime,
                fragment: self.fragment,
            },
            self.blocks,
            self.retention,
        )
    }
}
