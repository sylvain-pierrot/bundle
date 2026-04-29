//! Async bundle reader.

use std::io;
use std::pin::Pin;
use std::task::Poll;

use futures_io::AsyncRead;

use super::reader::OpenBundleReader;
use crate::bundle::Bundle;
use crate::error::Error;
use crate::retention::{AsyncRetention, NoopRetention};

/// Async bundle reader. Counterpart of [`BundleReader`](super::BundleReader).
///
/// Async-reads from the source, async-writes to the retention, then
/// sync-parses from the retained bytes.
pub struct BundleAsyncReader;

impl BundleAsyncReader {
    pub fn new() -> Self {
        BundleAsyncReader
    }

    /// Receive a bundle from an async source.
    ///
    /// Bytes are async-read and async-written to the retention. After
    /// EOF, the bundle is sync-parsed from the retained bytes.
    /// Bounded memory — only metadata is parsed into memory.
    pub async fn read_from<R, S>(&self, mut source: R, mut retention: S) -> Result<Bundle<S>, Error>
    where
        R: AsyncRead + Unpin,
        S: AsyncRetention,
    {
        let mut total = 0u64;
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
            total += n as u64;
        }

        retention
            .flush()
            .await
            .map_err(|e| Error::Cbor(aqueduct_cbor::Error::from(e)))?;

        if total == 0 {
            return Err(Error::EmptyRetention);
        }

        let source = retention
            .reader(0, total)
            .await
            .map_err(aqueduct_cbor::Error::from)?;
        match OpenBundleReader::open(source, NoopRetention).into_bundle() {
            Ok(noop_bundle) => Ok(noop_bundle.swap_retention(retention)),
            Err(e) => {
                let _ = retention.discard().await;
                Err(e)
            }
        }
    }
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
