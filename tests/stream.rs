use std::io::Read;

use aqueduct::{
    BlockEvent, BlockFlags, BundleReader, BundleWriter, CanonicalBlock, Crc, CreationTimestamp,
    Eid, HopCount, PrimaryBlock,
};

#[test]
fn stream_roundtrip_minimal() {
    let primary = PrimaryBlock {
        version: 7,
        flags: aqueduct::BundleFlags::from_bits(0),
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

    // Step-by-step reading
    let mut reader = BundleReader::new(buf.as_slice(), aqueduct::MemoryRetention::new());

    let mut read_payload = Vec::new();
    while let Some(event) = reader.next_block().unwrap() {
        match event {
            BlockEvent::Extension(_) => {}
            BlockEvent::Payload { len } => {
                assert_eq!(len, payload.len() as u64);
                reader
                    .payload_reader()
                    .read_to_end(&mut read_payload)
                    .unwrap();
            }
        }
    }
    assert_eq!(read_payload, payload);
    assert_eq!(reader.primary().unwrap().version, 7);
    assert_eq!(reader.primary().unwrap().lifetime, 3_600_000_000);
    let bundle = reader.into_bundle().unwrap();
    assert!(bundle.payload_crc().is_none());
}

#[test]
fn stream_roundtrip_with_crc() {
    let primary = PrimaryBlock {
        version: 7,
        flags: aqueduct::BundleFlags::from_bits(0),
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

    let mut reader = BundleReader::new(buf.as_slice(), aqueduct::MemoryRetention::new());

    let mut read_payload = Vec::new();
    while let Some(event) = reader.next_block().unwrap() {
        match event {
            BlockEvent::Extension(_) => {}
            BlockEvent::Payload { .. } => {
                reader
                    .payload_reader()
                    .read_to_end(&mut read_payload)
                    .unwrap();
            }
        }
    }
    assert_eq!(read_payload, payload);

    let bundle = reader.into_bundle().unwrap();
    assert_eq!(bundle.payload_crc().crc_type(), 1);
    match bundle.payload_crc() {
        Crc::Crc16(v) => assert_ne!(v, 0),
        _ => panic!("expected Crc16"),
    }
}

#[test]
fn stream_roundtrip_with_extensions() {
    let primary = PrimaryBlock {
        version: 7,
        flags: aqueduct::BundleFlags::from_bits(0),
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

    let mut reader = BundleReader::new(buf.as_slice(), aqueduct::MemoryRetention::new());

    let mut ext_count = 0;
    while let Some(event) = reader.next_block().unwrap() {
        match event {
            BlockEvent::Extension(idx) => {
                let parsed = reader.extensions()[idx].parse_ext::<HopCount>().unwrap();
                assert_eq!(parsed.limit, 30);
                assert_eq!(parsed.count, 5);
                ext_count += 1;
            }
            BlockEvent::Payload { len } => {
                reader.walk(len).unwrap();
            }
        }
    }
    assert_eq!(ext_count, 1);
}

#[test]
fn stream_walk() {
    let primary = PrimaryBlock {
        version: 7,
        flags: aqueduct::BundleFlags::from_bits(0),
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

    let mut reader = BundleReader::new(buf.as_slice(), aqueduct::MemoryRetention::new());

    while let Some(event) = reader.next_block().unwrap() {
        match event {
            BlockEvent::Extension(_) => {}
            BlockEvent::Payload { len } => {
                assert_eq!(len, 1000);
                reader.walk(len).unwrap();
            }
        }
    }

    let bundle = reader.into_bundle().unwrap();
    assert!(bundle.payload_crc().is_none());
}

#[test]
fn stream_compatible_with_inmemory() {
    let primary = PrimaryBlock {
        version: 7,
        flags: aqueduct::BundleFlags::from_bits(0),
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

    let bundle =
        aqueduct::Bundle::from_bytes(&stream_buf, aqueduct::MemoryRetention::new()).unwrap();
    assert_eq!(bundle.primary().version, 7);

    let inmem_buf = bundle.encode().unwrap();
    assert_eq!(stream_buf, inmem_buf);
}

#[test]
fn stream_forwarding_pattern() {
    let primary = PrimaryBlock {
        version: 7,
        flags: aqueduct::BundleFlags::from_bits(0),
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
    let payload = b"forwarded payload data that streams through";

    // Encode original
    let mut original = Vec::new();
    let mut w = BundleWriter::new(&mut original).unwrap();
    w.write_primary(&primary).unwrap();
    w.begin_payload(BlockFlags::from_bits(0), Crc::None, payload.len() as u64)
        .unwrap();
    w.write_payload_data(payload).unwrap();
    w.end_payload().unwrap();
    w.finish().unwrap();

    // Forward: read step-by-step, add hop count, stream payload through
    let mut reader = BundleReader::new(original.as_slice(), aqueduct::MemoryRetention::new());

    let mut forwarded = Vec::new();
    let mut writer = BundleWriter::new(&mut forwarded).unwrap();
    let mut wrote_primary = false;

    while let Some(event) = reader.next_block().unwrap() {
        // After first next_block(), primary is available
        if !wrote_primary {
            writer.write_primary(reader.primary().unwrap()).unwrap();
            let hc = HopCount {
                limit: 30,
                count: 1,
            };
            let ext = CanonicalBlock::from_ext(2, BlockFlags::from_bits(0), Crc::None, &hc);
            writer.write_extension(&ext).unwrap();
            wrote_primary = true;
        }

        match event {
            BlockEvent::Extension(idx) => {
                writer.write_extension(&reader.extensions()[idx]).unwrap();
            }
            BlockEvent::Payload { len } => {
                writer
                    .begin_payload(reader.payload_flags(), Crc::None, len)
                    .unwrap();

                let mut buf = [0u8; 8192];
                {
                    let mut pr = reader.payload_reader();
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

    // Verify forwarded bundle
    let mut reader2 = BundleReader::new(forwarded.as_slice(), aqueduct::MemoryRetention::new());
    let mut ext_count = 0;
    let mut fwd_payload = Vec::new();
    while let Some(event) = reader2.next_block().unwrap() {
        match event {
            BlockEvent::Extension(idx) => {
                let hop = reader2.extensions()[idx].parse_ext::<HopCount>().unwrap();
                assert_eq!(hop.count, 1);
                ext_count += 1;
            }
            BlockEvent::Payload { .. } => {
                reader2
                    .payload_reader()
                    .read_to_end(&mut fwd_payload)
                    .unwrap();
            }
        }
    }
    assert_eq!(ext_count, 1);
    assert_eq!(fwd_payload, payload);
    assert_eq!(reader2.primary().unwrap().dest_eid, primary.dest_eid);
}

#[test]
fn retention_from_bytes_roundtrip() {
    let payload = b"retained payload";
    let bundle = aqueduct::Bundle::builder(
        Eid::Null,
        Eid::Null,
        1000,
        payload,
        aqueduct::MemoryRetention::new(),
    )
    .unwrap()
    .build();

    let encoded = bundle.encode().unwrap();
    let decoded = aqueduct::Bundle::from_bytes(&encoded, aqueduct::MemoryRetention::new()).unwrap();

    // Read payload back from retention
    let mut buf = Vec::new();
    decoded
        .payload_reader()
        .unwrap()
        .read_to_end(&mut buf)
        .unwrap();
    assert_eq!(buf, payload);

    // Re-encode from retention
    let reencoded = decoded.encode().unwrap();
    assert_eq!(encoded, reencoded);
}

#[test]
fn retention_from_stream_roundtrip() {
    let payload = b"streamed and retained";
    let bundle = aqueduct::Bundle::builder(
        Eid::Ipn {
            allocator_id: 0,
            node_number: 5,
            service_number: 1,
        },
        Eid::Null,
        60_000_000,
        payload,
        aqueduct::MemoryRetention::new(),
    )
    .unwrap()
    .build();

    let encoded = bundle.encode().unwrap();

    // from_stream with a fresh retention
    let decoded =
        aqueduct::Bundle::from_stream(encoded.as_slice(), aqueduct::MemoryRetention::new())
            .unwrap();

    assert_eq!(
        decoded.primary().dest_eid,
        Eid::Ipn {
            allocator_id: 0,
            node_number: 5,
            service_number: 1
        }
    );

    let mut buf = Vec::new();
    decoded
        .payload_reader()
        .unwrap()
        .read_to_end(&mut buf)
        .unwrap();
    assert_eq!(buf, payload);
}

#[test]
fn retention_builder_payload_reader() {
    let payload = b"builder stores in retention";
    let bundle = aqueduct::Bundle::builder(
        Eid::Null,
        Eid::Null,
        1000,
        payload,
        aqueduct::MemoryRetention::new(),
    )
    .unwrap()
    .build();

    // payload_reader works immediately after build (no encode/decode cycle)
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
            flags: aqueduct::BundleFlags::from_bits(0),
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
        flags: aqueduct::BundleFlags::from_bits(0),
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

    let mut reader = BundleReader::new(buf.as_slice(), aqueduct::MemoryRetention::new());

    while let Some(event) = reader.next_block().unwrap() {
        if let BlockEvent::Payload { .. } = event {
            reader.walk(50).unwrap();
            reader.walk(50).unwrap();
        }
    }
    let bundle = reader.into_bundle().unwrap();
    assert_eq!(bundle.payload_len(), 100);
}

// ---------------------------------------------------------------------------
// Retention tests
// ---------------------------------------------------------------------------

#[test]
fn memory_retention_roundtrip() {
    let payload = b"memory retention test";
    let bundle = aqueduct::Bundle::builder(
        Eid::Null,
        Eid::Null,
        1000,
        payload,
        aqueduct::MemoryRetention::new(),
    )
    .unwrap()
    .build();

    let encoded = bundle.encode().unwrap();
    let decoded = aqueduct::Bundle::from_bytes(&encoded, aqueduct::MemoryRetention::new()).unwrap();

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
fn disk_retention_roundtrip() {
    let path = "/tmp/aqueduct_test_disk_retention.bin";
    let payload = b"disk retention test payload data";

    let bundle = aqueduct::Bundle::builder(
        Eid::Ipn {
            allocator_id: 0,
            node_number: 1,
            service_number: 1,
        },
        Eid::Null,
        1000,
        payload,
        aqueduct::MemoryRetention::new(),
    )
    .unwrap()
    .build();

    let encoded = bundle.encode().unwrap();

    let disk = aqueduct::DiskRetention::new(path).unwrap();
    let decoded = aqueduct::Bundle::from_bytes(&encoded, disk).unwrap();

    assert_eq!(decoded.payload_len(), payload.len() as u64);
    assert_eq!(
        decoded.primary().dest_eid,
        Eid::Ipn {
            allocator_id: 0,
            node_number: 1,
            service_number: 1,
        }
    );

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
fn disk_retention_large_payload() {
    let path = "/tmp/aqueduct_test_disk_large.bin";
    let payload = vec![0xCDu8; 1024 * 1024]; // 1 MB

    let bundle = aqueduct::Bundle::builder(
        Eid::Null,
        Eid::Null,
        1000,
        &payload,
        aqueduct::MemoryRetention::new(),
    )
    .unwrap()
    .build();

    let encoded = bundle.encode().unwrap();

    let disk = aqueduct::DiskRetention::new(path).unwrap();
    let decoded = aqueduct::Bundle::from_bytes(&encoded, disk).unwrap();

    let mut buf = Vec::new();
    decoded
        .payload_reader()
        .unwrap()
        .read_to_end(&mut buf)
        .unwrap();
    assert_eq!(buf.len(), payload.len());
    assert!(buf.iter().all(|&b| b == 0xCD));

    std::fs::remove_file(path).unwrap();
}

#[test]
fn disk_retention_from_stream() {
    let bundle_path = "/tmp/aqueduct_test_bundle_stream.bin";
    let retention_path = "/tmp/aqueduct_test_retention_stream.bin";
    let payload = b"streaming to disk";

    // Write bundle to file
    let bundle = aqueduct::Bundle::builder(
        Eid::Null,
        Eid::Null,
        1000,
        payload,
        aqueduct::MemoryRetention::new(),
    )
    .unwrap()
    .build();
    let encoded = bundle.encode().unwrap();
    std::fs::write(bundle_path, &encoded).unwrap();

    // Read from file stream → DiskRetention
    let file = std::fs::File::open(bundle_path).unwrap();
    let disk = aqueduct::DiskRetention::new(retention_path).unwrap();
    let decoded = aqueduct::Bundle::from_stream(file, disk).unwrap();

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

    // Encode via BundleWriter (streaming CRC)
    let primary = aqueduct::PrimaryBlock {
        version: 7,
        flags: aqueduct::BundleFlags::from_bits(0),
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

    // Encode via Bundle.encode() (in-memory CRC)
    let bundle = aqueduct::Bundle::builder(
        Eid::Null,
        Eid::Null,
        1000,
        payload,
        aqueduct::MemoryRetention::new(),
    )
    .unwrap()
    .build();
    let inmem_buf = bundle.encode().unwrap();

    // Both should decode to the same CRC values
    let stream_bundle =
        aqueduct::Bundle::from_bytes(&stream_buf, aqueduct::MemoryRetention::new()).unwrap();
    let inmem_bundle =
        aqueduct::Bundle::from_bytes(&inmem_buf, aqueduct::MemoryRetention::new()).unwrap();

    // Primary CRC should match (both use CRC-32C)
    assert_eq!(
        stream_bundle.primary().crc.crc_type(),
        inmem_bundle.primary().crc.crc_type()
    );
    // Payload CRC — streaming used crc16, builder defaults to None
    assert_eq!(stream_bundle.payload_crc().crc_type(), 1);
    match stream_bundle.payload_crc() {
        Crc::Crc16(v) => assert_ne!(v, 0),
        _ => panic!("expected Crc16"),
    }

    // Re-encode both and verify round-trip
    let re_stream = stream_bundle.encode().unwrap();
    let re_inmem = inmem_bundle.encode().unwrap();
    assert_eq!(stream_buf, re_stream);
    assert_eq!(inmem_buf, re_inmem);
}
