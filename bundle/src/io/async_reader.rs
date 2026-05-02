//! Async bundle reader with optional ingress filter pipeline.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;

use bundle_bpv7::{BlockData, BlockFlags, CanonicalBlock, Crc, Error, PrimaryBlock};
use bundle_cbor::{Encoder, StreamDecoder, ToCbor};
use bundle_io::Error as IoError;
use futures_io::AsyncRead;

use super::reader::{BlockEvent, OpenBundleReader, ReadResult};
use crate::bundle::Bundle;
use crate::filter::{BundleFilter, BundleMetadata, BundleMutator, FilterChain, FilterRejection};
use crate::retention::{AsyncRetention, NoopRetention};

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
    pub async fn read_from<R, S>(
        &self,
        mut source: R,
        mut retention: S,
    ) -> Result<ReadResult<S>, Error>
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

        // Phase 2: run filters/mutators if configured, write to retention.
        if !self.chain.is_empty() {
            let parsed = parse_metadata(&header_buf)?;
            let mut primary = parsed.primary;
            let mut extensions = parsed.extensions;
            let mut payload_offset = parsed.payload_data_offset as u64;

            let meta = BundleMetadata {
                primary: &primary,
                extensions: &extensions,
                payload_len: parsed.payload_len,
            };
            if let Err(rejection) = self.chain.run_filters(&meta) {
                return Ok(ReadResult::Rejected(rejection));
            }

            let mutated = self.chain.run_mutators(&mut primary, &mut extensions);

            if mutated {
                // Re-encode mutated headers so retention holds the mutated version.
                let mut enc = Encoder::with_capacity(256);
                enc.write_indefinite_array();
                primary.encode(&mut enc);
                for ext in &extensions {
                    ext.encode(&mut enc);
                }
                let has_crc = parsed.payload_crc_type != 0;
                enc.write_array(if has_crc { 6 } else { 5 });
                enc.write_uint(1);
                enc.write_uint(1);
                enc.write_uint(parsed.payload_flags);
                enc.write_uint(parsed.payload_crc_type);
                enc.write_bstr_header(parsed.payload_len);

                payload_offset = enc.position() as u64;

                retention
                    .write_all(enc.as_bytes())
                    .await
                    .map_err(Error::from)?;
                retention
                    .write_all(&header_buf[parsed.payload_data_offset..])
                    .await
                    .map_err(Error::from)?;
            } else {
                retention
                    .write_all(&header_buf)
                    .await
                    .map_err(Error::from)?;
            }
            for buf in &overflow {
                retention.write_all(buf).await.map_err(Error::from)?;
            }
            drop(overflow);

            // Stream remaining source bytes to retention.
            let mut retention_total = total;
            if mutated {
                retention_total =
                    retention_total - parsed.payload_data_offset as u64 + payload_offset;
            }
            loop {
                let n = poll_read(&mut source, &mut chunk).await?;
                if n == 0 {
                    break;
                }
                retention
                    .write_all(&chunk[..n])
                    .await
                    .map_err(Error::from)?;
                retention_total += n as u64;
            }

            retention.flush().await.map_err(Error::from)?;

            // Build Bundle<S> directly from parsed structs.
            let payload_crc = if parsed.payload_crc_type != 0 {
                let tail_start = payload_offset + parsed.payload_len;
                let tail_len = retention_total - tail_start;
                let tail = retention.reader(tail_start, tail_len).await?;
                let mut tail_dec = StreamDecoder::new(tail);
                Crc::decode_stream(&mut tail_dec, parsed.payload_crc_type)?
            } else {
                Crc::None
            };

            extensions.push(CanonicalBlock {
                block_type: 1,
                block_number: 1,
                flags: BlockFlags::from_bits(parsed.payload_flags),
                crc: payload_crc,
                data: BlockData::Retained {
                    offset: payload_offset,
                    len: parsed.payload_len,
                },
            });

            Ok(ReadResult::Accepted(Bundle::from_parts(
                primary, extensions, retention,
            )))
        } else {
            // No filters/mutators: write everything to retention, parse once.
            retention
                .write_all(&header_buf)
                .await
                .map_err(Error::from)?;
            for buf in &overflow {
                retention.write_all(buf).await.map_err(Error::from)?;
            }
            drop(overflow);

            loop {
                let n = poll_read(&mut source, &mut chunk).await?;
                if n == 0 {
                    break;
                }
                retention
                    .write_all(&chunk[..n])
                    .await
                    .map_err(Error::from)?;
            }

            retention.flush().await.map_err(Error::from)?;

            // Parse from header_buf to build Bundle<S>.
            let empty = Arc::new(FilterChain::new());
            match OpenBundleReader::open(&header_buf[..], NoopRetention, empty).into_bundle() {
                Ok(ReadResult::Accepted(bundle)) => {
                    Ok(ReadResult::Accepted(bundle.swap_retention(retention)))
                }
                Err(e) => {
                    let _ = retention.discard().await;
                    Err(e)
                }
                _ => unreachable!("no filters cofigured"),
            }
        }
    }
}

/// Parsed metadata from a header buffer.
struct ParsedMetadata {
    primary: PrimaryBlock,
    extensions: Vec<CanonicalBlock>,
    payload_len: u64,
    /// Byte offset in header_buf where payload data starts.
    payload_data_offset: usize,
    /// The payload block's CRC type.
    payload_crc_type: u64,
    /// The payload block's flags.
    payload_flags: u64,
}

/// Parse primary + extension blocks from a header buffer to run filters.
fn parse_metadata(header_buf: &[u8]) -> Result<ParsedMetadata, Error> {
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

    let payload_block = reader
        .blocks()
        .iter()
        .find(|b| b.is_payload())
        .ok_or(Error::IncompleteRead)?;
    let payload_crc_type = payload_block.crc.crc_type();
    let payload_flags = payload_block.flags.bits();
    let payload_data_offset = payload_block
        .retained_range()
        .ok_or(Error::PayloadNotInline)?
        .0 as usize;

    let extensions: Vec<_> = reader
        .blocks()
        .iter()
        .filter(|b| !b.is_payload())
        .cloned()
        .collect();

    Ok(ParsedMetadata {
        primary,
        extensions,
        payload_len,
        payload_data_offset,
        payload_crc_type,
        payload_flags,
    })
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
    .map_err(|e| Error::Cbor(bundle_cbor::Error::Io(IoError::Io(e))))
}
