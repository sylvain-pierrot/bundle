//! Builder for constructing bundles with sensible defaults.

use std::io::Read;
#[cfg(feature = "async")]
use std::pin::Pin;
#[cfg(feature = "async")]
use std::task::Poll;

use crate::bundle::Bundle;
use crate::bundle::canonical::{
    BlockData, BlockFlags, CanonicalBlock, PAYLOAD_BLOCK_NUMBER, PAYLOAD_BLOCK_TYPE,
};
use crate::bundle::crc::Crc;
use crate::bundle::primary::{BundleFlags, CreationTimestamp, FragmentInfo, PrimaryBlock};
use crate::eid::Eid;
use crate::error::Error;
#[cfg(feature = "async")]
use crate::retention::AsyncRetention;
use crate::retention::Retention;
#[cfg(feature = "async")]
use futures_io::AsyncRead;

/// Fluent builder for [`Bundle`].
pub struct BundleBuilder<S> {
    dest_eid: Eid,
    src_node_id: Eid,
    lifetime: u64,
    payload_len: u64,
    bundle_flags: u64,
    rpt_eid: Eid,
    creation_ts: CreationTimestamp,
    fragment: Option<FragmentInfo>,
    blocks: Vec<CanonicalBlock>,
    retention: S,
}

impl<S> BundleBuilder<S> {
    fn with_payload_len(
        dest_eid: Eid,
        src_node_id: Eid,
        lifetime: u64,
        payload_len: u64,
        retention: S,
    ) -> Self {
        Self {
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

    pub fn report_to(mut self, eid: Eid) -> Self {
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
        self.blocks.push(block);
        self
    }

    pub fn build(mut self) -> Result<Bundle<S>, Error> {
        if self.src_node_id.is_null() {
            self.bundle_flags |= 0x000004;
        }

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

        let bundle = Bundle::from_parts(
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
        );
        bundle.validate()?;
        Ok(bundle)
    }
}

impl<S: Retention> BundleBuilder<S> {
    /// Create a builder with an in-memory payload.
    pub fn new(
        dest_eid: Eid,
        src_node_id: Eid,
        lifetime: u64,
        payload: &[u8],
        mut retention: S,
    ) -> Result<Self, Error> {
        let payload_len = payload.len() as u64;
        retention
            .write_all(payload)
            .map_err(aqueduct_cbor::Error::from)?;
        retention.flush().map_err(aqueduct_cbor::Error::from)?;
        Ok(Self::with_payload_len(
            dest_eid,
            src_node_id,
            lifetime,
            payload_len,
            retention,
        ))
    }

    /// Create a builder with a sync streaming payload.
    ///
    /// Sync-reads from `source` in 64KB chunks and sync-writes to retention.
    /// `payload_len` must be the exact number of bytes that will be read.
    pub fn from_stream<R: Read>(
        dest_eid: Eid,
        src_node_id: Eid,
        lifetime: u64,
        payload_len: u64,
        mut source: R,
        mut retention: S,
    ) -> Result<Self, Error> {
        let mut buf = [0u8; 65536];
        let mut remaining = payload_len;
        while remaining > 0 {
            let to_read = buf.len().min(remaining as usize);
            let n = source
                .read(&mut buf[..to_read])
                .map_err(aqueduct_cbor::Error::from)?;
            if n == 0 {
                return Err(Error::IncompleteRead);
            }
            retention
                .write_all(&buf[..n])
                .map_err(aqueduct_cbor::Error::from)?;
            remaining -= n as u64;
        }
        retention.flush().map_err(aqueduct_cbor::Error::from)?;
        Ok(Self::with_payload_len(
            dest_eid,
            src_node_id,
            lifetime,
            payload_len,
            retention,
        ))
    }
}

#[cfg(feature = "async")]
impl<S: AsyncRetention> BundleBuilder<S> {
    /// Create a builder with an async streaming payload.
    ///
    /// Async-reads from `source` in 64KB chunks and async-writes to retention.
    /// `payload_len` must be the exact number of bytes that will be read.
    pub async fn from_async_stream<R: AsyncRead + Unpin>(
        dest_eid: Eid,
        src_node_id: Eid,
        lifetime: u64,
        payload_len: u64,
        mut source: R,
        mut retention: S,
    ) -> Result<Self, Error> {
        let mut buf = [0u8; 65536];
        let mut remaining = payload_len;
        while remaining > 0 {
            let to_read = buf.len().min(remaining as usize);
            let n = std::future::poll_fn(|cx| -> Poll<std::io::Result<usize>> {
                Pin::new(&mut source).poll_read(cx, &mut buf[..to_read])
            })
            .await
            .map_err(aqueduct_cbor::Error::from)?;
            if n == 0 {
                return Err(Error::IncompleteRead);
            }
            retention
                .write_all(&buf[..n])
                .await
                .map_err(aqueduct_cbor::Error::from)?;
            remaining -= n as u64;
        }
        retention
            .flush()
            .await
            .map_err(aqueduct_cbor::Error::from)?;
        Ok(Self::with_payload_len(
            dest_eid,
            src_node_id,
            lifetime,
            payload_len,
            retention,
        ))
    }
}
