//! Async bundle reader.

use std::io;
use std::pin::Pin;
use std::task::Poll;

use futures_io::AsyncRead;

use super::reader::{BlockEvent, OpenBundleReader};
use crate::bundle::Bundle;
use crate::bundle::canonical::CanonicalBlock;
use crate::bundle::crc::Crc;
use crate::bundle::primary::PrimaryBlock;
use crate::error::Error;
use crate::retention::{AsyncRetention, NoopRetention};

use aqueduct_cbor::StreamDecoder;

const HEADER_BUF_SIZE: usize = 65536;

/// Async bundle reader. Counterpart of [`BundleReader`](super::BundleReader).
///
/// Async-reads from the source, async-writes to the retention, then
/// parses the bundle structure from a locally buffered header prefix.
/// Only the tail bytes (payload CRC) are read back from retention,
/// avoiding a full re-download for large bundles.
pub struct BundleAsyncReader;

impl BundleAsyncReader {
    pub fn new() -> Self {
        BundleAsyncReader
    }

    /// Receive a bundle from an async source.
    ///
    /// Bytes are async-read and async-written to the retention. The
    /// first 64KB are also buffered locally for header parsing.
    /// Bounded memory — only metadata is parsed into memory.
    pub async fn read_from<R, S>(&self, mut source: R, mut retention: S) -> Result<Bundle<S>, Error>
    where
        R: AsyncRead + Unpin,
        S: AsyncRetention,
    {
        let mut total = 0u64;
        let mut header_buf = Vec::with_capacity(HEADER_BUF_SIZE);
        let mut chunk = [0u8; 65536];

        loop {
            let n = poll_read(&mut source, &mut chunk).await?;
            if n == 0 {
                break;
            }
            retention
                .write_all(&chunk[..n])
                .await
                .map_err(|e| Error::Cbor(aqueduct_cbor::Error::from(e)))?;
            if header_buf.len() < HEADER_BUF_SIZE {
                let take = n.min(HEADER_BUF_SIZE - header_buf.len());
                header_buf.extend_from_slice(&chunk[..take]);
            }
            total += n as u64;
        }

        retention
            .flush()
            .await
            .map_err(|e| Error::Cbor(aqueduct_cbor::Error::from(e)))?;

        if total == 0 {
            return Err(Error::EmptyRetention);
        }

        // Small bundles: parse entirely from local buffer
        if total <= HEADER_BUF_SIZE as u64 {
            match OpenBundleReader::open(&header_buf[..], NoopRetention).into_bundle() {
                Ok(noop_bundle) => return Ok(noop_bundle.swap_retention(retention)),
                Err(e) => {
                    let _ = retention.discard().await;
                    return Err(e);
                }
            }
        }

        // Large bundles: parse headers from local buffer, read only
        // the payload CRC tail from retention (typically ~6 bytes).
        match parse_large(&header_buf, total, &mut retention).await {
            Ok((primary, blocks)) => Ok(Bundle::from_parts(primary, blocks, retention)),
            Err(e) => {
                let _ = retention.discard().await;
                Err(e)
            }
        }
    }
}

/// Parse a large bundle by reading headers from a local buffer prefix
/// and fetching only the payload CRC tail from retention.
async fn parse_large<S: AsyncRetention>(
    header_buf: &[u8],
    total: u64,
    retention: &mut S,
) -> Result<(PrimaryBlock, Vec<CanonicalBlock>), Error> {
    let mut reader = OpenBundleReader::open(header_buf, NoopRetention);

    loop {
        match reader.next_block()? {
            Some(BlockEvent::Extension(_)) => {}
            Some(BlockEvent::Payload { .. }) => break,
            None => return Err(Error::IncompleteRead),
        }
    }

    let primary = reader.primary().cloned().ok_or(Error::IncompleteRead)?;
    let mut blocks = reader.blocks().to_vec();

    // Read payload CRC from tail if needed
    let payload_crc_type = blocks
        .iter()
        .find(|b| b.is_payload())
        .map(|b| b.crc.crc_type())
        .unwrap_or(0);

    if payload_crc_type != 0
        && let Some(idx) = blocks.iter().position(|b| b.is_payload())
    {
        let (offset, len) = blocks[idx]
            .retained_range()
            .ok_or(Error::PayloadNotInline)?;
        let tail_start = offset + len;
        let tail_len = total - tail_start;
        let tail = retention
            .reader(tail_start, tail_len)
            .await
            .map_err(aqueduct_cbor::Error::from)?;
        let mut tail_dec = StreamDecoder::new(tail);
        blocks[idx].crc = Crc::decode(&mut tail_dec, payload_crc_type)?;
    }

    Ok((primary, blocks))
}

impl Default for BundleAsyncReader {
    fn default() -> Self {
        Self::new()
    }
}

async fn poll_read<R: AsyncRead + Unpin>(reader: &mut R, buf: &mut [u8]) -> Result<usize, Error> {
    std::future::poll_fn(|cx| -> Poll<io::Result<usize>> {
        Pin::new(&mut *reader).poll_read(cx, buf)
    })
    .await
    .map_err(|e| Error::Cbor(aqueduct_cbor::Error::from(e)))
}
