use std::io::Read;

use aqueduct::{
    BlockEvent, BlockFlags, BundleBuilder, BundleReader, BundleWriter, CanonicalBlock, Crc,
    CreationTimestamp, Eid, HopCount, MemoryRetention, PrimaryBlock,
};

#[test]
fn stream_roundtrip_minimal() {
    let primary = PrimaryBlock {
        version: 7,
        flags: aqueduct::BundleFlags::from_bits(0x000004),
        crc: Crc::None,
        dest_eid: Eid::Null,
        src_node_id: Eid::Null,
        rpt_eid: Eid::Null,
        creation_ts: CreationTimestamp { time: 0, seq: 0 },
        lifetime: 3_600_000_000,
        fragment: None,
    };
    let payload = b"hello streaming";

    let mut buf = Vec::new();
    let mut writer = BundleWriter::new(&mut buf).unwrap();
    writer.write_primary(&primary).unwrap();
    writer
        .begin_payload(BlockFlags::from_bits(0), Crc::None, payload.len() as u64)
        .unwrap();
    writer.write_payload_data(payload).unwrap();
    writer.end_payload().unwrap();
    writer.finish().unwrap();

    let reader = BundleReader::new();
    let mut session = reader.open(buf.as_slice(), MemoryRetention::new());

    let mut read_payload = Vec::new();
    while let Some(event) = session.next_block().unwrap() {
        match event {
            BlockEvent::Extension(_) => {}
            BlockEvent::Payload { len } => {
                assert_eq!(len, payload.len() as u64);
                session
                    .payload_reader()
                    .unwrap()
                    .read_to_end(&mut read_payload)
                    .unwrap();
            }
        }
    }
    assert_eq!(read_payload, payload);
    assert_eq!(session.primary().unwrap().version, 7);
    let bundle = session.into_bundle().unwrap();
    assert!(bundle.payload_crc().is_none());
}

#[test]
fn stream_roundtrip_with_crc() {
    let primary = PrimaryBlock {
        version: 7,
        flags: aqueduct::BundleFlags::from_bits(0x000004),
        crc: Crc::crc32c(),
        dest_eid: Eid::Ipn {
            allocator_id: 0,
            node_number: 1,
            service_number: 0,
        },
        src_node_id: Eid::Null,
        rpt_eid: Eid::Null,
        creation_ts: CreationTimestamp { time: 100, seq: 0 },
        lifetime: 1000,
        fragment: None,
    };
    let payload = b"payload with crc";

    let mut buf = Vec::new();
    let mut writer = BundleWriter::new(&mut buf).unwrap();
    writer.write_primary(&primary).unwrap();
    writer
        .begin_payload(BlockFlags::from_bits(0), Crc::crc16(), payload.len() as u64)
        .unwrap();
    writer.write_payload_data(&payload[..8]).unwrap();
    writer.write_payload_data(&payload[8..]).unwrap();
    writer.end_payload().unwrap();
    writer.finish().unwrap();

    let mut session = BundleReader::new().open(buf.as_slice(), MemoryRetention::new());
    let mut read_payload = Vec::new();
    while let Some(event) = session.next_block().unwrap() {
        match event {
            BlockEvent::Extension(_) => {}
            BlockEvent::Payload { .. } => {
                session
                    .payload_reader()
                    .unwrap()
                    .read_to_end(&mut read_payload)
                    .unwrap();
            }
        }
    }
    assert_eq!(read_payload, payload);
    let bundle = session.into_bundle().unwrap();
    assert_eq!(bundle.payload_crc().crc_type(), 1);
}

#[test]
fn stream_roundtrip_with_extensions() {
    let primary = PrimaryBlock {
        version: 7,
        flags: aqueduct::BundleFlags::from_bits(0x000004),
        crc: Crc::None,
        dest_eid: Eid::Null,
        src_node_id: Eid::Null,
        rpt_eid: Eid::Null,
        creation_ts: CreationTimestamp { time: 0, seq: 0 },
        lifetime: 1000,
        fragment: None,
    };
    let hc = HopCount {
        limit: 30,
        count: 5,
    };
    let ext = CanonicalBlock::from_ext(2, BlockFlags::from_bits(0), Crc::None, &hc);

    let mut buf = Vec::new();
    let mut writer = BundleWriter::new(&mut buf).unwrap();
    writer.write_primary(&primary).unwrap();
    writer.write_extension(&ext).unwrap();
    writer
        .begin_payload(BlockFlags::from_bits(0), Crc::None, 0)
        .unwrap();
    writer.end_payload().unwrap();
    writer.finish().unwrap();

    let mut session = BundleReader::new().open(buf.as_slice(), MemoryRetention::new());
    let mut ext_count = 0;
    while let Some(event) = session.next_block().unwrap() {
        match event {
            BlockEvent::Extension(idx) => {
                let parsed = session.blocks()[idx].parse_ext::<HopCount>().unwrap();
                assert_eq!(parsed.limit, 30);
                ext_count += 1;
            }
            BlockEvent::Payload { len } => session.walk(len).unwrap(),
        }
    }
    assert_eq!(ext_count, 1);
}

#[test]
fn stream_walk() {
    let primary = PrimaryBlock {
        version: 7,
        flags: aqueduct::BundleFlags::from_bits(0x000004),
        crc: Crc::None,
        dest_eid: Eid::Null,
        src_node_id: Eid::Null,
        rpt_eid: Eid::Null,
        creation_ts: CreationTimestamp { time: 0, seq: 0 },
        lifetime: 1000,
        fragment: None,
    };

    let mut buf = Vec::new();
    let mut writer = BundleWriter::new(&mut buf).unwrap();
    writer.write_primary(&primary).unwrap();
    writer
        .begin_payload(BlockFlags::from_bits(0), Crc::None, 1000)
        .unwrap();
    writer.write_payload_data(&[0xAB; 1000]).unwrap();
    writer.end_payload().unwrap();
    writer.finish().unwrap();

    let mut session = BundleReader::new().open(buf.as_slice(), MemoryRetention::new());
    while let Some(event) = session.next_block().unwrap() {
        if let BlockEvent::Payload { len } = event {
            assert_eq!(len, 1000);
            session.walk(len).unwrap();
        }
    }
    let bundle = session.into_bundle().unwrap();
    assert!(bundle.payload_crc().is_none());
}

#[test]
fn stream_compatible_with_inmemory() {
    let primary = PrimaryBlock {
        version: 7,
        flags: aqueduct::BundleFlags::from_bits(0x000004),
        crc: Crc::None,
        dest_eid: Eid::Null,
        src_node_id: Eid::Null,
        rpt_eid: Eid::Null,
        creation_ts: CreationTimestamp { time: 0, seq: 0 },
        lifetime: 3_600_000_000,
        fragment: None,
    };
    let payload = b"cross-api test";

    let mut stream_buf = Vec::new();
    let mut writer = BundleWriter::new(&mut stream_buf).unwrap();
    writer.write_primary(&primary).unwrap();
    writer
        .begin_payload(BlockFlags::from_bits(0), Crc::None, payload.len() as u64)
        .unwrap();
    writer.write_payload_data(payload).unwrap();
    writer.end_payload().unwrap();
    writer.finish().unwrap();

    let bundle = BundleReader::new()
        .read_from(stream_buf.as_slice(), MemoryRetention::new())
        .unwrap();
    assert_eq!(bundle.primary().version, 7);

    let inmem_buf = bundle.encode().unwrap();
    assert_eq!(stream_buf, inmem_buf);
}

#[test]
fn stream_forwarding_pattern() {
    let primary = PrimaryBlock {
        version: 7,
        flags: aqueduct::BundleFlags::from_bits(0x000004),
        crc: Crc::None,
        dest_eid: Eid::Ipn {
            allocator_id: 0,
            node_number: 10,
            service_number: 1,
        },
        src_node_id: Eid::Ipn {
            allocator_id: 0,
            node_number: 1,
            service_number: 0,
        },
        rpt_eid: Eid::Null,
        creation_ts: CreationTimestamp { time: 500, seq: 0 },
        lifetime: 60_000_000,
        fragment: None,
    };
    let payload = b"forwarded payload";

    let mut original = Vec::new();
    let mut w = BundleWriter::new(&mut original).unwrap();
    w.write_primary(&primary).unwrap();
    w.begin_payload(BlockFlags::from_bits(0), Crc::None, payload.len() as u64)
        .unwrap();
    w.write_payload_data(payload).unwrap();
    w.end_payload().unwrap();
    w.finish().unwrap();

    let mut session = BundleReader::new().open(original.as_slice(), MemoryRetention::new());

    let mut forwarded = Vec::new();
    let mut writer = BundleWriter::new(&mut forwarded).unwrap();
    let mut wrote_primary = false;

    while let Some(event) = session.next_block().unwrap() {
        if !wrote_primary {
            writer.write_primary(session.primary().unwrap()).unwrap();
            let hc = HopCount {
                limit: 30,
                count: 1,
            };
            writer
                .write_extension(&CanonicalBlock::from_ext(
                    2,
                    BlockFlags::from_bits(0),
                    Crc::None,
                    &hc,
                ))
                .unwrap();
            wrote_primary = true;
        }
        match event {
            BlockEvent::Extension(idx) => {
                writer.write_extension(&session.blocks()[idx]).unwrap();
            }
            BlockEvent::Payload { len } => {
                writer
                    .begin_payload(BlockFlags::from_bits(0), Crc::None, len)
                    .unwrap();
                let mut buf = [0u8; 8192];
                {
                    let mut pr = session.payload_reader().unwrap();
                    loop {
                        let n = pr.read(&mut buf).unwrap();
                        if n == 0 {
                            break;
                        }
                        writer.write_payload_data(&buf[..n]).unwrap();
                    }
                }
                writer.end_payload().unwrap();
            }
        }
    }
    writer.finish().unwrap();

    let mut session2 = BundleReader::new().open(forwarded.as_slice(), MemoryRetention::new());
    let mut ext_count = 0;
    let mut fwd_payload = Vec::new();
    while let Some(event) = session2.next_block().unwrap() {
        match event {
            BlockEvent::Extension(idx) => {
                let hop = session2.blocks()[idx].parse_ext::<HopCount>().unwrap();
                assert_eq!(hop.count, 1);
                ext_count += 1;
            }
            BlockEvent::Payload { .. } => {
                session2
                    .payload_reader()
                    .unwrap()
                    .read_to_end(&mut fwd_payload)
                    .unwrap();
            }
        }
    }
    assert_eq!(ext_count, 1);
    assert_eq!(fwd_payload, payload);
    assert_eq!(session2.primary().unwrap().dest_eid, primary.dest_eid);
}

// -- Retention tests ---------------------------------------------------------

#[test]
fn memory_retention_roundtrip() {
    let payload = b"memory retention test";
    let bundle = BundleBuilder::new(Eid::Null, Eid::Null, 1000, payload, MemoryRetention::new())
        .unwrap()
        .build()
        .unwrap();
    let encoded = bundle.encode().unwrap();
    let decoded = BundleReader::new()
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap();

    let mut buf = Vec::new();
    decoded
        .payload_reader()
        .unwrap()
        .read_to_end(&mut buf)
        .unwrap();
    assert_eq!(buf, payload);

    let reencoded = decoded.encode().unwrap();
    assert_eq!(encoded, reencoded);
}

#[test]
fn retention_builder_payload_reader() {
    let payload = b"builder stores in retention";
    let bundle = BundleBuilder::new(Eid::Null, Eid::Null, 1000, payload, MemoryRetention::new())
        .unwrap()
        .build()
        .unwrap();
    let mut buf = Vec::new();
    bundle
        .payload_reader()
        .unwrap()
        .read_to_end(&mut buf)
        .unwrap();
    assert_eq!(buf, payload);
}

#[test]
fn writer_rejects_excess_payload() {
    let mut buf = Vec::new();
    let mut writer = BundleWriter::new(&mut buf).unwrap();
    writer
        .write_primary(&PrimaryBlock {
            version: 7,
            flags: aqueduct::BundleFlags::from_bits(0x000004),
            crc: Crc::None,
            dest_eid: Eid::Null,
            src_node_id: Eid::Null,
            rpt_eid: Eid::Null,
            creation_ts: CreationTimestamp { time: 0, seq: 0 },
            lifetime: 1000,
            fragment: None,
        })
        .unwrap();
    writer
        .begin_payload(BlockFlags::from_bits(0), Crc::None, 5)
        .unwrap();
    assert!(writer.write_payload_data(b"too long data").is_err());
}

#[test]
fn reader_walk_partial() {
    let primary = PrimaryBlock {
        version: 7,
        flags: aqueduct::BundleFlags::from_bits(0x000004),
        crc: Crc::None,
        dest_eid: Eid::Null,
        src_node_id: Eid::Null,
        rpt_eid: Eid::Null,
        creation_ts: CreationTimestamp { time: 0, seq: 0 },
        lifetime: 1000,
        fragment: None,
    };

    let mut buf = Vec::new();
    let mut writer = BundleWriter::new(&mut buf).unwrap();
    writer.write_primary(&primary).unwrap();
    writer
        .begin_payload(BlockFlags::from_bits(0), Crc::None, 100)
        .unwrap();
    writer.write_payload_data(&[0xAA; 100]).unwrap();
    writer.end_payload().unwrap();
    writer.finish().unwrap();

    let mut session = BundleReader::new().open(buf.as_slice(), MemoryRetention::new());
    while let Some(event) = session.next_block().unwrap() {
        if let BlockEvent::Payload { .. } = event {
            session.walk(50).unwrap();
            session.walk(50).unwrap();
        }
    }
    let bundle = session.into_bundle().unwrap();
    assert_eq!(bundle.payload_len(), 100);
}

#[test]
fn disk_retention_roundtrip() {
    let path = "/tmp/aqueduct_test_disk_retention.bin";
    let payload = b"disk retention test payload data";

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

    let encoded = bundle.encode().unwrap();
    let disk = aqueduct::DiskRetention::new(path).unwrap();
    let decoded = BundleReader::new()
        .read_from(encoded.as_slice(), disk)
        .unwrap();

    let mut buf = Vec::new();
    decoded
        .payload_reader()
        .unwrap()
        .read_to_end(&mut buf)
        .unwrap();
    assert_eq!(buf, payload);

    let reencoded = decoded.encode().unwrap();
    assert_eq!(encoded, reencoded);

    std::fs::remove_file(path).unwrap();
}

#[test]
fn disk_retention_from_stream() {
    let bundle_path = "/tmp/aqueduct_test_bundle_stream.bin";
    let retention_path = "/tmp/aqueduct_test_retention_stream.bin";
    let payload = b"streaming to disk";

    let bundle = BundleBuilder::new(Eid::Null, Eid::Null, 1000, payload, MemoryRetention::new())
        .unwrap()
        .build()
        .unwrap();
    let encoded = bundle.encode().unwrap();
    std::fs::write(bundle_path, &encoded).unwrap();

    let file = std::fs::File::open(bundle_path).unwrap();
    let disk = aqueduct::DiskRetention::new(retention_path).unwrap();
    let decoded = BundleReader::new().read_from(file, disk).unwrap();

    let mut buf = Vec::new();
    decoded
        .payload_reader()
        .unwrap()
        .read_to_end(&mut buf)
        .unwrap();
    assert_eq!(buf, payload);

    std::fs::remove_file(bundle_path).unwrap();
    std::fs::remove_file(retention_path).unwrap();
}

#[test]
fn streaming_crc_matches_inmemory_crc() {
    let payload = b"crc cross-validation payload";

    let primary = PrimaryBlock {
        version: 7,
        flags: aqueduct::BundleFlags::from_bits(0x000004),
        crc: Crc::crc32c(),
        dest_eid: Eid::Null,
        src_node_id: Eid::Null,
        rpt_eid: Eid::Null,
        creation_ts: CreationTimestamp { time: 0, seq: 0 },
        lifetime: 1000,
        fragment: None,
    };

    let mut stream_buf = Vec::new();
    let mut writer = BundleWriter::new(&mut stream_buf).unwrap();
    writer.write_primary(&primary).unwrap();
    writer
        .begin_payload(BlockFlags::from_bits(0), Crc::crc16(), payload.len() as u64)
        .unwrap();
    writer.write_payload_data(payload).unwrap();
    writer.end_payload().unwrap();
    writer.finish().unwrap();

    let bundle = BundleBuilder::new(Eid::Null, Eid::Null, 1000, payload, MemoryRetention::new())
        .unwrap()
        .build()
        .unwrap();
    let inmem_buf = bundle.encode().unwrap();

    let stream_bundle = BundleReader::new()
        .read_from(stream_buf.as_slice(), MemoryRetention::new())
        .unwrap();
    let inmem_bundle = BundleReader::new()
        .read_from(inmem_buf.as_slice(), MemoryRetention::new())
        .unwrap();

    assert_eq!(
        stream_bundle.primary().crc.crc_type(),
        inmem_bundle.primary().crc.crc_type()
    );

    let re_stream = stream_bundle.encode().unwrap();
    let re_inmem = inmem_bundle.encode().unwrap();
    assert_eq!(stream_buf, re_stream);
    assert_eq!(inmem_buf, re_inmem);
}
