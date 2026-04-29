pub mod builder;
pub mod canonical;
pub mod crc;
pub(crate) mod payload;
pub mod primary;

use std::io::Read;
#[cfg(feature = "async")]
use std::pin::Pin;
#[cfg(feature = "async")]
use std::task::Poll;

use aqueduct_cbor::{Encoder, ToCbor};
#[cfg(feature = "async")]
use futures_io::AsyncRead;

use canonical::CanonicalBlock;
use crc::Crc;
use payload::PayloadRef;
use primary::PrimaryBlock;

use crate::eid::Eid;
use crate::error::Error;
use crate::io::BundleReader;
#[cfg(feature = "async")]
use crate::io::retention::AsyncRetention;
use crate::io::retention::{NoopRetention, Retention};

/// A BPv7 bundle (RFC 9171 §4.1).
#[derive(Debug, Clone)]
pub struct Bundle<S> {
    primary: PrimaryBlock<'static>,
    extensions: Vec<CanonicalBlock>,
    payload: PayloadRef,
    retention: S,
}

impl<S> Bundle<S> {
    pub(crate) fn from_parts(
        primary: PrimaryBlock<'static>,
        extensions: Vec<CanonicalBlock>,
        payload: PayloadRef,
        retention: S,
    ) -> Self {
        Bundle {
            primary,
            extensions,
            payload,
            retention,
        }
    }

    pub fn primary(&self) -> &PrimaryBlock<'static> {
        &self.primary
    }

    pub fn primary_mut(&mut self) -> &mut PrimaryBlock<'static> {
        &mut self.primary
    }

    pub fn extensions(&self) -> &[CanonicalBlock] {
        &self.extensions
    }

    pub fn extensions_mut(&mut self) -> &mut Vec<CanonicalBlock> {
        &mut self.extensions
    }

    pub fn payload_len(&self) -> u64 {
        self.payload.data_len
    }

    pub fn payload_crc(&self) -> Crc {
        self.payload.crc
    }

    pub(crate) fn payload_ref(&self) -> &PayloadRef {
        &self.payload
    }

    pub fn retention(&self) -> &S {
        &self.retention
    }

    pub fn block_by_type(&self, block_type: u64) -> Option<&CanonicalBlock> {
        self.extensions.iter().find(|b| b.block_type == block_type)
    }

    pub fn block_by_number(&self, number: u64) -> Option<&CanonicalBlock> {
        self.extensions.iter().find(|b| b.block_number == number)
    }

    pub fn validate(&self) -> Result<(), Error> {
        self.primary.validate()?;

        const PAYLOAD_BLOCK_NUMBER: u64 = 1;

        for (i, a) in self.extensions.iter().enumerate() {
            if a.block_number == PAYLOAD_BLOCK_NUMBER {
                return Err(Error::DuplicateBlockNumber(a.block_number));
            }
            for b in &self.extensions[i + 1..] {
                if a.block_number == b.block_number {
                    return Err(Error::DuplicateBlockNumber(a.block_number));
                }
            }
        }

        Ok(())
    }
}

impl<S: Retention> Bundle<S> {
    pub fn builder(
        dest_eid: Eid<'_>,
        src_node_id: Eid<'_>,
        lifetime: u64,
        payload: &[u8],
        retention: S,
    ) -> Result<builder::BundleBuilder<S>, Error> {
        builder::BundleBuilder::new(
            dest_eid.into_owned(),
            src_node_id.into_owned(),
            lifetime,
            payload,
            retention,
        )
    }

    pub fn from_bytes(data: &[u8], retention: S) -> Result<Self, Error> {
        Self::from_stream(data, retention)
    }

    pub fn from_stream<R: Read>(source: R, retention: S) -> Result<Self, Error> {
        BundleReader::new(source, retention).into_bundle()
    }

    /// Parse a bundle from a retention that already contains the wire bytes.
    ///
    /// Used after async reception: bytes were written to the retention
    /// Receive a bundle from an async source.
    ///
    /// Bytes are read asynchronously until EOF and written to the retention.
    /// CBOR parsing happens synchronously from the retained bytes.
    #[cfg(feature = "async")]
    pub async fn from_async_stream<R>(mut source: R, mut retention: S) -> Result<Self, Error>
    where
        R: AsyncRead + Unpin,
        S: AsyncRetention,
    {
        let mut total = 0u64;
        let mut buf = [0u8; 65536];
        loop {
            let n: usize = std::future::poll_fn(|cx| -> Poll<std::io::Result<usize>> {
                Pin::new(&mut source).poll_read(cx, &mut buf)
            })
            .await
            .map_err(|e| Error::Cbor(aqueduct_cbor::Error::from(e)))?;
            if n == 0 {
                break;
            }
            let mut remaining = &buf[..n];
            while !remaining.is_empty() {
                let w: usize = std::future::poll_fn(|cx| -> Poll<std::io::Result<usize>> {
                    Pin::new(&mut retention).poll_write(cx, remaining)
                })
                .await
                .map_err(|e| Error::Cbor(aqueduct_cbor::Error::from(e)))?;
                remaining = &remaining[w..];
            }
            total += n as u64;
        }
        std::future::poll_fn(|cx| -> Poll<std::io::Result<()>> {
            Pin::new(&mut retention).poll_flush(cx)
        })
        .await
        .map_err(|e| Error::Cbor(aqueduct_cbor::Error::from(e)))?;

        Self::from_retention(retention, total)
    }

    pub fn from_retention(retention: S, len: u64) -> Result<Self, Error> {
        if len == 0 {
            return Err(Error::EmptyRetention);
        }
        let source = retention.reader(0, len);
        // Use a no-op sink — bytes are already in retention, no teeing needed.
        let noop = NoopRetention;
        let reader = BundleReader::new(source, noop);
        let (primary, extensions, payload) = {
            let noop_bundle = reader.into_bundle()?;
            (
                noop_bundle.primary().clone(),
                noop_bundle.extensions().to_vec(),
                noop_bundle.payload_ref().clone(),
            )
        };
        Ok(Bundle::from_parts(primary, extensions, payload, retention))
    }

    pub fn payload_reader(&self) -> S::Reader<'_> {
        self.retention
            .reader(self.payload.data_offset, self.payload.data_len)
    }

    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        let mut payload_data = Vec::with_capacity(self.payload.data_len as usize);
        self.payload_reader()
            .read_to_end(&mut payload_data)
            .map_err(aqueduct_cbor::Error::from)?;

        let mut enc = Encoder::new();
        enc.write_indefinite_array();
        self.primary.encode(&mut enc);
        for ext in &self.extensions {
            ext.encode(&mut enc);
        }

        let has_crc = !self.payload.crc.is_none();
        let block_start = enc.position();
        enc.write_array(if has_crc { 6 } else { 5 });
        enc.write_uint(1);
        enc.write_uint(1);
        enc.write_uint(self.payload.flags.bits());
        enc.write_uint(self.payload.crc.crc_type());
        enc.write_bstr(&payload_data);
        self.payload.crc.encode_and_finalize(&mut enc, block_start);
        enc.write_break();
        Ok(enc.into_bytes())
    }
}
