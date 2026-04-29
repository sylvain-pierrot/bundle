//! Async bundle reader with optional ingress filter pipeline.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;

use futures_io::AsyncRead;

use super::reader::{BlockEvent, OpenBundleReader};
use crate::bundle::Bundle;
use crate::bundle::canonical::CanonicalBlock;
use crate::bundle::crc::Crc;
use crate::bundle::primary::PrimaryBlock;
use crate::error::Error;
use crate::filter::{BundleFilter, BundleMetadata, BundleMutator, FilterChain};
use crate::retention::{AsyncRetention, NoopRetention};

use aqueduct_cbor::StreamDecoder;

const HEADER_BUF_SIZE: usize = 65536;

/// Async bundle reader with optional ingress filter pipeline.
///
/// Filters run in-flight before the payload touches retention.
/// Rejected bundles waste zero storage I/O.
pub struct BundleAsyncReader {
    chain: Arc<FilterChain>,
}

impl BundleAsyncReader {
    pub fn new() -> Self {
        BundleAsyncReader {
            chain: Arc::new(FilterChain::new()),
        }
    }

    pub fn filter(mut self, f: impl BundleFilter + 'static) -> Self {
        Arc::get_mut(&mut self.chain)
            .expect("filter must be called before cloning the reader")
            .add_filter(f);
        self
    }

    pub fn mutator(mut self, m: impl BundleMutator + 'static) -> Self {
        Arc::get_mut(&mut self.chain)
            .expect("mutator must be called before cloning the reader")
            .add_mutator(m);
        self
    }

    /// Receive a bundle from an async source.
    ///
    /// Bytes are async-read. Headers are buffered locally and filters
    /// run before any bytes hit retention. Rejected bundles waste
    /// zero retention I/O.
    pub async fn read_from<R, S>(&self, mut source: R, mut retention: S) -> Result<Bundle<S>, Error>
    where
        R: AsyncRead + Unpin,
        S: AsyncRetention,
    {
        let mut total = 0u64;
        let mut header_buf = Vec::with_capacity(HEADER_BUF_SIZE);
        let mut overflow: Vec<Vec<u8>> = Vec::new();
        let mut chunk = [0u8; 65536];
        let mut headers_done = false;

        // Phase 1: buffer headers locally (up to 64KB)
        while !headers_done {
            let n = poll_read(&mut source, &mut chunk).await?;
            if n == 0 {
                break;
            }
            if header_buf.len() < HEADER_BUF_SIZE {
                let take = n.min(HEADER_BUF_SIZE - header_buf.len());
                header_buf.extend_from_slice(&chunk[..take]);
                if take < n {
                    overflow.push(chunk[take..n].to_vec());
                    headers_done = true;
                }
            } else {
                overflow.push(chunk[..n].to_vec());
                headers_done = true;
            }
            total += n as u64;
        }

        if total == 0 {
            return Err(Error::EmptyRetention);
        }

        // Phase 2: run filters BEFORE touching retention (reject early, zero I/O)
        // Mutators run later during the real parse in phase 5.
        if !self.chain.is_empty() {
            let (primary, extensions, payload_len) = parse_metadata(&header_buf)?;
            let meta = BundleMetadata {
                primary: &primary,
                extensions: &extensions,
                payload_len,
            };
            self.chain.run_filters(&meta)?;
        }

        // Phase 3: write buffered bytes to retention
        retention
            .write_all(&header_buf)
            .await
            .map_err(|e| Error::Cbor(aqueduct_cbor::Error::from(e)))?;
        for buf in &overflow {
            retention
                .write_all(buf)
                .await
                .map_err(|e| Error::Cbor(aqueduct_cbor::Error::from(e)))?;
        }
        drop(overflow);

        // Phase 4: continue streaming remaining source bytes to retention
        loop {
            let n = poll_read(&mut source, &mut chunk).await?;
            if n == 0 {
                break;
            }
            retention
                .write_all(&chunk[..n])
                .await
                .map_err(|e| Error::Cbor(aqueduct_cbor::Error::from(e)))?;
            total += n as u64;
        }

        retention
            .flush()
            .await
            .map_err(|e| Error::Cbor(aqueduct_cbor::Error::from(e)))?;

        // Phase 5: parse bundle from retention
        // Filters re-run (idempotent, nanoseconds). Mutators run here.
        if total <= HEADER_BUF_SIZE as u64 {
            match OpenBundleReader::open(&header_buf[..], NoopRetention, self.chain.clone())
                .into_bundle()
            {
                Ok(noop_bundle) => return Ok(noop_bundle.swap_retention(retention)),
                Err(e) => {
                    let _ = retention.discard().await;
                    return Err(e);
                }
            }
        }

        match parse_large(&header_buf, total, &mut retention, &self.chain).await {
            Ok((primary, blocks)) => Ok(Bundle::from_parts(primary, blocks, retention)),
            Err(e) => {
                let _ = retention.discard().await;
                Err(e)
            }
        }
    }
}

/// Parse primary + extension blocks from a header buffer to run filters.
fn parse_metadata(header_buf: &[u8]) -> Result<(PrimaryBlock, Vec<CanonicalBlock>, u64), Error> {
    let chain = Arc::new(FilterChain::new());
    let mut reader = OpenBundleReader::open(header_buf, NoopRetention, chain);

    let payload_len = loop {
        match reader.next_block()? {
            Some(BlockEvent::Extension(_)) => {}
            Some(BlockEvent::Payload { len }) => break len,
            None => return Err(Error::IncompleteRead),
        }
    };

    let primary = reader.primary().cloned().ok_or(Error::IncompleteRead)?;
    let blocks: Vec<_> = reader
        .blocks()
        .iter()
        .filter(|b| !b.is_payload())
        .cloned()
        .collect();
    Ok((primary, blocks, payload_len))
}

/// Parse a large bundle by reading headers from a local buffer prefix
/// and fetching only the payload CRC tail from retention.
async fn parse_large<S: AsyncRetention>(
    header_buf: &[u8],
    total: u64,
    retention: &mut S,
    chain: &FilterChain,
) -> Result<(PrimaryBlock, Vec<CanonicalBlock>), Error> {
    let empty = Arc::new(FilterChain::new());
    let mut reader = OpenBundleReader::open(header_buf, NoopRetention, empty);

    loop {
        match reader.next_block()? {
            Some(BlockEvent::Extension(_)) => {}
            Some(BlockEvent::Payload { .. }) => break,
            None => return Err(Error::IncompleteRead),
        }
    }

    let mut primary = reader.primary().cloned().ok_or(Error::IncompleteRead)?;
    let mut blocks = reader.blocks().to_vec();

    chain.run_mutators(&mut primary, &mut blocks);

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
