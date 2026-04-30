#![cfg(feature = "async")]

use aqueduct::{
    AsyncRetention, BundleAsyncReader, BundleAsyncWriter, BundleBuilder, BundleReader,
    MemoryRetention, Retention,
};
use aqueduct_bpv7::{
    BlockFlags, BundleFlags, CanonicalBlock, Crc, CreationTimestamp, Eid, HopCount, PrimaryBlock,
};
use aqueduct_io::{Error as IoError, Read, Write};
use async_trait::async_trait;

/// Test-only async wrapper around MemoryRetention.
struct AsyncMemoryRetention(MemoryRetention);

impl AsyncMemoryRetention {
    fn new() -> Self {
        Self(MemoryRetention::new())
    }
}

impl Write for AsyncMemoryRetention {
    fn write_all(&mut self, buf: &[u8]) -> Result<(), IoError> {
        self.0.write_all(buf)
    }
    fn flush(&mut self) -> Result<(), IoError> {
        self.0.flush()
    }
}

impl Retention for AsyncMemoryRetention {
    type Reader<'a> = <MemoryRetention as Retention>::Reader<'a>;

    fn reader(&self, offset: u64, len: u64) -> Result<Self::Reader<'_>, IoError> {
        self.0.reader(offset, len)
    }
    fn discard(&mut self) -> Result<(), IoError> {
        self.0.discard()
    }
}

#[async_trait]
impl AsyncRetention for AsyncMemoryRetention {
    type Reader<'a> = <MemoryRetention as Retention>::Reader<'a>;

    async fn write(&mut self, data: &[u8]) -> Result<usize, IoError> {
        self.0.write_all(data)?;
        Ok(data.len())
    }
    async fn flush(&mut self) -> Result<(), IoError> {
        Ok(())
    }
    async fn reader(&self, offset: u64, len: u64) -> Result<Self::Reader<'_>, IoError> {
        self.0.reader(offset, len)
    }
    async fn discard(&mut self) -> Result<(), IoError> {
        self.0.discard()
    }
}

#[tokio::test]
async fn async_reader_roundtrip() {
    let payload = b"async hello";
    let bundle = BundleBuilder::new(
        Eid::Ipn {
            allocator_id: 0,
            node_number: 1,
            service_number: 1,
        },
        Eid::Null,
        1000,
        payload,
        MemoryRetention::new(),
    )
    .unwrap()
    .build()
    .unwrap();

    let mut encoded = Vec::new();
    bundle.encode_to(&mut encoded).unwrap();

    let decoded = BundleAsyncReader::new()
        .read_from(
            futures::io::Cursor::new(&encoded),
            AsyncMemoryRetention::new(),
        )
        .await
        .unwrap();

    assert_eq!(decoded.primary().dest_eid, bundle.primary().dest_eid);
    assert_eq!(decoded.payload_len(), payload.len() as u64);

    let mut buf = Vec::new();
    decoded
        .payload_reader()
        .unwrap()
        .read_to_end(&mut buf)
        .unwrap();
    assert_eq!(buf, payload);
}

#[tokio::test]
async fn async_writer_roundtrip() {
    let payload = b"async write test";

    let primary = PrimaryBlock {
        version: 7,
        flags: BundleFlags::from_bits(0x000004),
        crc: Crc::crc32c(),
        dest_eid: Eid::Ipn {
            allocator_id: 0,
            node_number: 5,
            service_number: 1,
        },
        src_node_id: Eid::Null,
        rpt_eid: Eid::Null,
        creation_ts: CreationTimestamp { time: 100, seq: 0 },
        lifetime: 1000,
        fragment: None,
    };

    let mut buf = Vec::new();
    let cursor = futures::io::Cursor::new(&mut buf);
    let mut writer = BundleAsyncWriter::new(cursor).await.unwrap();
    writer.write_primary(&primary).await.unwrap();
    writer
        .begin_payload(BlockFlags::from_bits(0), Crc::None, payload.len() as u64)
        .await
        .unwrap();
    writer.write_payload_data(payload).await.unwrap();
    writer.end_payload().await.unwrap();
    writer.finish().await.unwrap();

    let decoded = BundleReader::new()
        .read_from(buf.as_slice(), MemoryRetention::new())
        .unwrap();

    assert_eq!(decoded.primary().dest_eid, primary.dest_eid);

    let mut read_payload = Vec::new();
    decoded
        .payload_reader()
        .unwrap()
        .read_to_end(&mut read_payload)
        .unwrap();
    assert_eq!(read_payload, payload);
}

#[tokio::test]
async fn async_full_roundtrip() {
    let payload = b"full async roundtrip";

    let bundle = BundleBuilder::new(
        Eid::Ipn {
            allocator_id: 0,
            node_number: 10,
            service_number: 1,
        },
        Eid::Ipn {
            allocator_id: 0,
            node_number: 1,
            service_number: 0,
        },
        3_600_000_000,
        payload,
        MemoryRetention::new(),
    )
    .unwrap()
    .extension(CanonicalBlock::from_ext(
        2,
        BlockFlags::from_bits(0),
        Crc::None,
        &HopCount {
            limit: 30,
            count: 1,
        },
    ))
    .build()
    .unwrap();

    let mut encoded = Vec::new();
    bundle.encode_to(&mut encoded).unwrap();

    // Async decode
    let decoded = BundleAsyncReader::new()
        .read_from(
            futures::io::Cursor::new(&encoded),
            AsyncMemoryRetention::new(),
        )
        .await
        .unwrap();

    assert_eq!(decoded.primary().dest_eid, bundle.primary().dest_eid);
    assert_eq!(decoded.extensions().count(), 1);

    let hop = decoded
        .extensions()
        .next()
        .unwrap()
        .parse_ext::<HopCount>()
        .unwrap();
    assert_eq!(hop.limit, 30);
    assert_eq!(hop.count, 1);

    let mut buf = Vec::new();
    decoded
        .payload_reader()
        .unwrap()
        .read_to_end(&mut buf)
        .unwrap();
    assert_eq!(buf, payload);

    // Re-encode sync should match
    let mut reencoded = Vec::new();
    decoded.encode_to(&mut reencoded).unwrap();
    assert_eq!(encoded, reencoded);
}
