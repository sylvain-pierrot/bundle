use bundle::{BlockEvent, BundleBuilder, BundleReader, BundleWriter, MemoryRetention, ReadResult};
use bundle_bpv7::filter::builtin::HopCountIncrementMutator;
use bundle_bpv7::{
    BlockFlags, BundleFlags, CanonicalBlock, Crc, CreationTimestamp, Eid, HopCount, PrimaryBlock,
};

#[test]
fn stream_roundtrip_minimal() {
    let primary = PrimaryBlock {
        version: 7,
        flags: BundleFlags::from_bits(0x000004),
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
    let mut writer = BundleWriter::new().open(&mut buf);
    writer.write_primary(&primary).unwrap();
    writer
        .begin_payload(BlockFlags::from_bits(0), Crc::None, payload.len() as u64)
        .unwrap();
    writer.write_payload_data(payload).unwrap();
    writer.end_payload().unwrap();
    writer.finish().unwrap();

    let reader = BundleReader::new();
    let mut session = reader.open(buf.as_slice(), MemoryRetention::new());

    while let Some(event) = session.next_block().unwrap() {
        match event {
            BlockEvent::Extension(_) => {}
            BlockEvent::Payload { len } => {
                assert_eq!(len, payload.len() as u64);
                session.walk(len).unwrap();
            }
        }
    }
    assert_eq!(session.primary().unwrap().version, 7);
    let ReadResult::Accepted(bundle) = session.into_bundle().unwrap() else {
        panic!("expected accepted");
    };
    let mut read_payload = Vec::new();
    bundle.payload(&mut read_payload).unwrap();
    assert_eq!(read_payload, payload);
    assert!(bundle.payload_crc().is_none());
}

#[test]
fn stream_roundtrip_with_crc() {
    let primary = PrimaryBlock {
        version: 7,
        flags: BundleFlags::from_bits(0x000004),
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
    let mut writer = BundleWriter::new().open(&mut buf);
    writer.write_primary(&primary).unwrap();
    writer
        .begin_payload(BlockFlags::from_bits(0), Crc::crc16(), payload.len() as u64)
        .unwrap();
    writer.write_payload_data(&payload[..8]).unwrap();
    writer.write_payload_data(&payload[8..]).unwrap();
    writer.end_payload().unwrap();
    writer.finish().unwrap();

    let mut session = BundleReader::new().open(buf.as_slice(), MemoryRetention::new());
    while let Some(event) = session.next_block().unwrap() {
        match event {
            BlockEvent::Extension(_) => {}
            BlockEvent::Payload { len } => session.walk(len).unwrap(),
        }
    }
    let ReadResult::Accepted(bundle) = session.into_bundle().unwrap() else {
        panic!("expected accepted");
    };
    let mut read_payload = Vec::new();
    bundle.payload(&mut read_payload).unwrap();
    assert_eq!(read_payload, payload);
    assert_eq!(bundle.payload_crc().crc_type(), 1);
}

#[test]
fn stream_roundtrip_with_extensions() {
    let primary = PrimaryBlock {
        version: 7,
        flags: BundleFlags::from_bits(0x000004),
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
    let mut writer = BundleWriter::new().open(&mut buf);
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
        flags: BundleFlags::from_bits(0x000004),
        crc: Crc::None,
        dest_eid: Eid::Null,
        src_node_id: Eid::Null,
        rpt_eid: Eid::Null,
        creation_ts: CreationTimestamp { time: 0, seq: 0 },
        lifetime: 1000,
        fragment: None,
    };

    let mut buf = Vec::new();
    let mut writer = BundleWriter::new().open(&mut buf);
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
    let ReadResult::Accepted(bundle) = session.into_bundle().unwrap() else {
        panic!("expected accepted");
    };
    assert!(bundle.payload_crc().is_none());
}

#[test]
fn stream_compatible_with_inmemory() {
    let primary = PrimaryBlock {
        version: 7,
        flags: BundleFlags::from_bits(0x000004),
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
    let mut writer = BundleWriter::new().open(&mut stream_buf);
    writer.write_primary(&primary).unwrap();
    writer
        .begin_payload(BlockFlags::from_bits(0), Crc::None, payload.len() as u64)
        .unwrap();
    writer.write_payload_data(payload).unwrap();
    writer.end_payload().unwrap();
    writer.finish().unwrap();

    let ReadResult::Accepted(bundle) = BundleReader::new()
        .read_from(stream_buf.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };
    assert_eq!(bundle.primary().version, 7);

    let mut inmem_buf = Vec::new();
    bundle.encode_to(&mut inmem_buf).unwrap();
    assert_eq!(stream_buf, inmem_buf);
}

#[test]
fn stream_forwarding_pattern() {
    let primary = PrimaryBlock {
        version: 7,
        flags: BundleFlags::from_bits(0x000004),
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
    let mut w = BundleWriter::new().open(&mut original);
    w.write_primary(&primary).unwrap();
    w.begin_payload(BlockFlags::from_bits(0), Crc::None, payload.len() as u64)
        .unwrap();
    w.write_payload_data(payload).unwrap();
    w.end_payload().unwrap();
    w.finish().unwrap();

    // Receive
    let ReadResult::Accepted(bundle) = BundleReader::new()
        .read_from(original.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };

    // Forward with hop count mutator
    let mut forwarded = Vec::new();
    let writer = BundleWriter::new().mutator(HopCountIncrementMutator::new(30));
    writer.write_to(&bundle, &mut forwarded).unwrap();

    // Verify forwarded bundle
    let ReadResult::Accepted(fwd) = BundleReader::new()
        .read_from(forwarded.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };
    let hop = fwd
        .extensions()
        .find_map(|b| b.parse_ext::<HopCount>().ok())
        .unwrap();
    assert_eq!(hop.count, 1);
    let mut fwd_payload = Vec::new();
    fwd.payload(&mut fwd_payload).unwrap();
    assert_eq!(fwd_payload, payload);
    assert_eq!(fwd.primary().dest_eid, primary.dest_eid);
}

// -- Retention tests ---------------------------------------------------------

#[test]
fn memory_retention_roundtrip() {
    let payload = b"memory retention test";
    let bundle = BundleBuilder::new(Eid::Null, Eid::Null, 1000, payload, MemoryRetention::new())
        .unwrap()
        .build()
        .unwrap();
    let mut encoded = Vec::new();
    bundle.encode_to(&mut encoded).unwrap();
    let ReadResult::Accepted(decoded) = BundleReader::new()
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };

    let mut buf = Vec::new();
    decoded.payload(&mut buf).unwrap();
    assert_eq!(buf, payload);

    let mut reencoded = Vec::new();
    decoded.encode_to(&mut reencoded).unwrap();
    assert_eq!(encoded, reencoded);
}

#[test]
fn retention_builder_payload() {
    let payload = b"builder stores in retention";
    let bundle = BundleBuilder::new(Eid::Null, Eid::Null, 1000, payload, MemoryRetention::new())
        .unwrap()
        .build()
        .unwrap();
    let mut buf = Vec::new();
    bundle.payload(&mut buf).unwrap();
    assert_eq!(buf, payload);
}

#[test]
fn writer_rejects_excess_payload() {
    let mut buf = Vec::new();
    let mut writer = BundleWriter::new().open(&mut buf);
    writer
        .write_primary(&PrimaryBlock {
            version: 7,
            flags: BundleFlags::from_bits(0x000004),
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
        flags: BundleFlags::from_bits(0x000004),
        crc: Crc::None,
        dest_eid: Eid::Null,
        src_node_id: Eid::Null,
        rpt_eid: Eid::Null,
        creation_ts: CreationTimestamp { time: 0, seq: 0 },
        lifetime: 1000,
        fragment: None,
    };

    let mut buf = Vec::new();
    let mut writer = BundleWriter::new().open(&mut buf);
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
    let ReadResult::Accepted(bundle) = session.into_bundle().unwrap() else {
        panic!("expected accepted");
    };
    assert_eq!(bundle.payload_len(), 100);
}

#[test]
fn disk_retention_roundtrip() {
    let path = "/tmp/bundle_test_disk_retention.bin";
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

    let mut encoded = Vec::new();
    bundle.encode_to(&mut encoded).unwrap();
    let disk = bundle::DiskRetention::new(path).unwrap();
    let ReadResult::Accepted(decoded) = BundleReader::new()
        .read_from(encoded.as_slice(), disk)
        .unwrap()
    else {
        panic!("expected accepted");
    };

    let mut buf = Vec::new();
    decoded.payload(&mut buf).unwrap();
    assert_eq!(buf, payload);

    let mut reencoded = Vec::new();
    decoded.encode_to(&mut reencoded).unwrap();
    assert_eq!(encoded, reencoded);

    std::fs::remove_file(path).unwrap();
}

#[test]
fn disk_retention_from_stream() {
    let bundle_path = "/tmp/bundle_test_bundle_stream.bin";
    let retention_path = "/tmp/bundle_test_retention_stream.bin";
    let payload = b"streaming to disk";

    let bundle = BundleBuilder::new(Eid::Null, Eid::Null, 1000, payload, MemoryRetention::new())
        .unwrap()
        .build()
        .unwrap();
    let mut encoded = Vec::new();
    bundle.encode_to(&mut encoded).unwrap();
    std::fs::write(bundle_path, &encoded).unwrap();

    let file = std::fs::File::open(bundle_path).unwrap();
    let disk = bundle::DiskRetention::new(retention_path).unwrap();
    let ReadResult::Accepted(decoded) = BundleReader::new().read_from(file, disk).unwrap() else {
        panic!("expected accepted");
    };

    let mut buf = Vec::new();
    decoded.payload(&mut buf).unwrap();
    assert_eq!(buf, payload);

    std::fs::remove_file(bundle_path).unwrap();
    std::fs::remove_file(retention_path).unwrap();
}

#[test]
fn streaming_crc_matches_inmemory_crc() {
    let payload = b"crc cross-validation payload";

    let primary = PrimaryBlock {
        version: 7,
        flags: BundleFlags::from_bits(0x000004),
        crc: Crc::crc32c(),
        dest_eid: Eid::Null,
        src_node_id: Eid::Null,
        rpt_eid: Eid::Null,
        creation_ts: CreationTimestamp { time: 0, seq: 0 },
        lifetime: 1000,
        fragment: None,
    };

    let mut stream_buf = Vec::new();
    let mut writer = BundleWriter::new().open(&mut stream_buf);
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
    let mut inmem_buf = Vec::new();
    bundle.encode_to(&mut inmem_buf).unwrap();

    let ReadResult::Accepted(stream_bundle) = BundleReader::new()
        .read_from(stream_buf.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };
    let ReadResult::Accepted(inmem_bundle) = BundleReader::new()
        .read_from(inmem_buf.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };

    assert_eq!(
        stream_bundle.primary().crc.crc_type(),
        inmem_bundle.primary().crc.crc_type()
    );

    let mut re_stream = Vec::new();
    stream_bundle.encode_to(&mut re_stream).unwrap();
    let mut re_inmem = Vec::new();
    inmem_bundle.encode_to(&mut re_inmem).unwrap();
    assert_eq!(stream_buf, re_stream);
    assert_eq!(inmem_buf, re_inmem);
}
