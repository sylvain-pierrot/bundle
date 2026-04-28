//! Builder for constructing bundles with sensible defaults.

use crate::bundle::Bundle;
use crate::bundle::canonical::{BlockFlags, CanonicalBlock};
use crate::bundle::crc::Crc;
use crate::bundle::payload::PayloadRef;
use crate::bundle::primary::{BundleFlags, CreationTimestamp, FragmentInfo, PrimaryBlock};
use crate::eid::Eid;

/// Fluent builder for [`Bundle`].
///
/// Required fields (destination, source, lifetime, payload) are provided
/// upfront via [`Bundle::builder`]. Everything else has sensible defaults:
/// version 7, CRC-32C on primary block, no payload CRC, report-to null,
/// timestamp zero, no fragment, no extensions.
pub struct BundleBuilder<'a> {
    dest_eid: Eid<'a>,
    src_node_id: Eid<'a>,
    lifetime: u64,
    payload: &'a [u8],
    bundle_flags: u64,
    rpt_eid: Eid<'a>,
    creation_ts: CreationTimestamp,
    fragment: Option<FragmentInfo>,
    extensions: Vec<CanonicalBlock>,
}

impl<'a> BundleBuilder<'a> {
    pub(crate) fn new(
        dest_eid: Eid<'a>,
        src_node_id: Eid<'a>,
        lifetime: u64,
        payload: &'a [u8],
    ) -> Self {
        Self {
            dest_eid,
            src_node_id,
            lifetime,
            payload,
            bundle_flags: 0,
            rpt_eid: Eid::Null,
            creation_ts: CreationTimestamp { time: 0, seq: 0 },
            fragment: None,
            extensions: Vec::new(),
        }
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

    pub fn report_to(mut self, eid: Eid<'a>) -> Self {
        self.rpt_eid = eid;
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
        self.extensions.push(block);
        self
    }

    pub fn build(self) -> Bundle<'a> {
        Bundle {
            primary: PrimaryBlock {
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
            extensions: self.extensions,
            payload: PayloadRef {
                flags: BlockFlags::from_bits(0),
                crc: Crc::None,
                data_offset: 0,
                data_len: self.payload.len() as u64,
            },
        }
    }
}
