use aqueduct::{BundleBuilder, BundleReader, MemoryRetention};
use aqueduct_bpv7::{
    BlockFlags, BundleAge, BundleFlags, CanonicalBlock, Crc, CrcHasher, CreationTimestamp, Eid,
    Extension, HopCount, PreviousNode,
};

#[test]
fn roundtrip_minimal_bundle() {
    let payload = b"hello";
    let bundle = BundleBuilder::new(
        Eid::Null,
        Eid::Null,
        3_600_000_000,
        payload,
        MemoryRetention::new(),
    )
    .unwrap()
    .build()
    .unwrap();
    let encoded = bundle.encode().unwrap();
    let decoded = BundleReader::new()
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap();

    assert_eq!(decoded.primary().version, 7);
    assert_eq!(decoded.primary().dest_eid, Eid::Null);
    assert_eq!(decoded.primary().lifetime, 3_600_000_000);

    let reencoded = decoded.encode().unwrap();
    assert_eq!(encoded, reencoded);
}

#[test]
fn roundtrip_empty_payload() {
    let bundle = BundleBuilder::new(Eid::Null, Eid::Null, 1000, b"", MemoryRetention::new())
        .unwrap()
        .build()
        .unwrap();
    let encoded = bundle.encode().unwrap();
    let decoded = BundleReader::new()
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap();

    assert_eq!(decoded.payload_len(), 0);

    let reencoded = decoded.encode().unwrap();
    assert_eq!(encoded, reencoded);
}

#[test]
fn roundtrip_with_extensions() {
    let dest = Eid::Ipn {
        allocator_id: 0,
        node_number: 1,
        service_number: 2,
    };
    let payload = b"hello world";

    let bundle = BundleBuilder::new(
        dest.clone(),
        Eid::Ipn {
            allocator_id: 0,
            node_number: 3,
            service_number: 4,
        },
        60_000_000,
        payload,
        MemoryRetention::new(),
    )
    .unwrap()
    .creation_ts(CreationTimestamp { time: 1000, seq: 1 })
    .extension(CanonicalBlock::from_ext(
        2,
        BlockFlags::from_bits(0),
        Crc::None,
        &HopCount {
            limit: 30,
            count: 0,
        },
    ))
    .extension(CanonicalBlock::from_ext(
        3,
        BlockFlags::from_bits(0),
        Crc::None,
        &BundleAge { millis: 12345 },
    ))
    .build()
    .unwrap();

    let encoded = bundle.encode().unwrap();
    let decoded = BundleReader::new()
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap();

    assert_eq!(decoded.primary().dest_eid, dest);
    assert_eq!(decoded.extensions().count(), 2);

    let hop = decoded
        .extensions()
        .next()
        .unwrap()
        .parse_ext::<HopCount>()
        .unwrap();
    assert_eq!(hop.limit, 30);

    let age = decoded
        .extensions()
        .nth(1)
        .unwrap()
        .parse_ext::<BundleAge>()
        .unwrap();
    assert_eq!(age.millis, 12345);

    let reencoded = decoded.encode().unwrap();
    assert_eq!(encoded, reencoded);
}

#[test]
fn roundtrip_with_dtn_eids() {
    let bundle = BundleBuilder::new(
        Eid::Dtn("//node1/incoming".into()),
        Eid::Ipn {
            allocator_id: 0,
            node_number: 42,
            service_number: 0,
        },
        0,
        b"",
        MemoryRetention::new(),
    )
    .unwrap()
    .build()
    .unwrap();

    let encoded = bundle.encode().unwrap();
    let decoded = BundleReader::new()
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap();

    assert_eq!(
        decoded.primary().dest_eid,
        Eid::Dtn("//node1/incoming".into())
    );
    assert_eq!(
        decoded.primary().src_node_id,
        Eid::Ipn {
            allocator_id: 0,
            node_number: 42,
            service_number: 0
        }
    );
}

#[test]
fn roundtrip_fragment() {
    let payload = b"abc";
    let bundle = BundleBuilder::new(Eid::Null, Eid::Null, 1000, payload, MemoryRetention::new())
        .unwrap()
        .fragment(100, 5000)
        .build()
        .unwrap();

    let encoded = bundle.encode().unwrap();
    let decoded = BundleReader::new()
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap();

    let frag = decoded.primary().fragment.unwrap();
    assert_eq!(frag.offset, 100);
    assert_eq!(frag.total_adu_len, 5000);

    let reencoded = decoded.encode().unwrap();
    assert_eq!(encoded, reencoded);
}

#[test]
fn roundtrip_crc_values_nonzero() {
    let payload = b"test payload";
    let bundle = BundleBuilder::new(Eid::Null, Eid::Null, 1000, payload, MemoryRetention::new())
        .unwrap()
        .build()
        .unwrap();
    let encoded = bundle.encode().unwrap();
    let decoded = BundleReader::new()
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap();

    match decoded.primary().crc {
        Crc::Crc32c(v) => assert_ne!(v, 0),
        other => panic!("expected Crc32c, got {other:?}"),
    }
}

#[test]
fn extension_hop_count_roundtrip() {
    let hc = HopCount {
        limit: 30,
        count: 5,
    };
    let data = hc.encode_data();
    assert_eq!(HopCount::parse(&data).unwrap(), hc);
}

#[test]
fn extension_bundle_age_roundtrip() {
    let ba = BundleAge { millis: 999999 };
    let data = ba.encode_data();
    assert_eq!(BundleAge::parse(&data).unwrap(), ba);
}

#[test]
fn extension_previous_node_roundtrip() {
    let pn = PreviousNode {
        node_id: Eid::Ipn {
            allocator_id: 0,
            node_number: 10,
            service_number: 0,
        },
    };
    let data = pn.encode_data();
    assert_eq!(PreviousNode::parse(&data).unwrap().node_id, pn.node_id);
}

#[test]
fn decode_empty_input() {
    assert!(
        BundleReader::new()
            .read_from(&b""[..], MemoryRetention::new())
            .is_err()
    );
}

#[test]
fn decode_truncated_bundle() {
    assert!(
        BundleReader::new()
            .read_from(&[0x9F, 0xFF][..], MemoryRetention::new())
            .is_err()
    );
}

#[test]
fn decode_garbage() {
    assert!(
        BundleReader::new()
            .read_from(&[0xDE, 0xAD, 0xBE, 0xEF][..], MemoryRetention::new())
            .is_err()
    );
}

#[test]
fn validate_wrong_version() {
    let mut bundle = BundleBuilder::new(Eid::Null, Eid::Null, 1000, b"", MemoryRetention::new())
        .unwrap()
        .build()
        .unwrap();
    bundle.primary_mut().version = 6;
    assert!(bundle.primary().validate().is_err());
}

#[test]
fn validate_fragment_flag_mismatch() {
    let mut bundle = BundleBuilder::new(Eid::Null, Eid::Null, 1000, b"", MemoryRetention::new())
        .unwrap()
        .build()
        .unwrap();
    bundle.primary_mut().flags = BundleFlags::from_bits(0x01);
    bundle.primary_mut().fragment = None;
    assert!(bundle.primary().validate().is_err());
}

#[test]
fn validate_duplicate_block_numbers() {
    let hc = HopCount {
        limit: 10,
        count: 0,
    };
    let mut bundle = BundleBuilder::new(Eid::Null, Eid::Null, 1000, b"", MemoryRetention::new())
        .unwrap()
        .build()
        .unwrap();
    bundle.blocks_mut().push(CanonicalBlock::from_ext(
        2,
        BlockFlags::from_bits(0),
        Crc::None,
        &hc,
    ));
    bundle.blocks_mut().push(CanonicalBlock::from_ext(
        2,
        BlockFlags::from_bits(0),
        Crc::None,
        &hc,
    ));
    assert!(bundle.validate().is_err());
}

#[test]
fn parse_ext_wrong_block_type() {
    let ba = BundleAge { millis: 100 };
    let block = CanonicalBlock::from_ext(2, BlockFlags::from_bits(0), Crc::None, &ba);
    assert!(block.parse_ext::<HopCount>().is_err());
}

#[test]
fn hop_count_exceeded() {
    assert!(
        HopCount {
            limit: 10,
            count: 11
        }
        .exceeded()
    );
    assert!(
        !HopCount {
            limit: 10,
            count: 10
        }
        .exceeded()
    );
}

#[test]
fn crc_compute_invalid_type() {
    assert!(Crc::compute(3, b"data").is_err());
    assert!(Crc::compute(255, b"data").is_err());
}

#[test]
fn crc_incremental_matches_oneshot() {
    let data = b"hello world test data for crc";

    let mut h16 = CrcHasher::new(&Crc::crc16()).unwrap();
    h16.update(&data[..10]);
    h16.update(&data[10..]);
    assert_eq!(h16.finalize(), Crc::Crc16(Crc::compute_crc16(data)));

    let mut h32 = CrcHasher::new(&Crc::crc32c()).unwrap();
    h32.update(&data[..5]);
    h32.update(&data[5..20]);
    h32.update(&data[20..]);
    assert_eq!(h32.finalize(), Crc::Crc32c(Crc::compute_crc32c(data)));
}

#[test]
fn crc_verify_detects_corruption() {
    let payload = b"verify me";
    let bundle = BundleBuilder::new(Eid::Null, Eid::Null, 1000, payload, MemoryRetention::new())
        .unwrap()
        .build()
        .unwrap();
    let encoded = bundle.encode().unwrap();

    let mut corrupted = encoded.clone();
    corrupted[5] ^= 0xFF;
    if let Ok(bad) = BundleReader::new().read_from(corrupted.as_slice(), MemoryRetention::new()) {
        let reencoded = bad.encode().unwrap();
        assert_ne!(reencoded, corrupted);
    }
}
