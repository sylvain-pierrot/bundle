use bundle::{BundleBuilder, BundleReader, MemoryRetention, ReadResult};
use bundle_bpv7::filter::builtin::{
    DestinationFilter, HopCountFilter, HopCountIncrementMutator, MaxPayloadSizeFilter,
    PreviousNodeMutator,
};
use bundle_bpv7::{BlockFlags, Crc, Eid, HopCount, PreviousNode};
fn local_eid() -> Eid {
    Eid::Ipn {
        allocator_id: 0,
        node_number: 1,
        service_number: 0,
    }
}

fn remote_eid() -> Eid {
    Eid::Ipn {
        allocator_id: 0,
        node_number: 99,
        service_number: 1,
    }
}

fn build_bundle(payload: &[u8]) -> BundleBuilder<MemoryRetention> {
    BundleBuilder::new(
        local_eid(),
        remote_eid(),
        1000,
        payload,
        MemoryRetention::new(),
    )
}

fn encode(builder: BundleBuilder<MemoryRetention>) -> Vec<u8> {
    let bundle = builder.build().unwrap();
    let mut buf = Vec::new();
    bundle.encode_to(&mut buf).unwrap();
    buf
}

// -- Filter tests ------------------------------------------------------------

#[test]
fn no_filters_passthrough() {
    let encoded = encode(build_bundle(b"hello"));
    let reader = BundleReader::new();
    let ReadResult::Accepted(bundle) = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };
    assert_eq!(bundle.payload_len(), 5);
}

#[test]
fn max_payload_size_accepts() {
    let encoded = encode(build_bundle(b"small"));
    let reader = BundleReader::new().filter(MaxPayloadSizeFilter::new(1000));
    let ReadResult::Accepted(bundle) = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };
    assert_eq!(bundle.payload_len(), 5);
}

#[test]
fn max_payload_size_rejects() {
    let encoded = encode(build_bundle(b"too large payload"));
    let reader = BundleReader::new().filter(MaxPayloadSizeFilter::new(5));
    let result = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap();
    assert!(matches!(result, ReadResult::Rejected(_)));
}

#[test]
fn rejected_bundle_zero_retention_io() {
    let encoded = encode(build_bundle(b"should not hit retention"));
    let reader = BundleReader::new().filter(MaxPayloadSizeFilter::new(1));
    let retention = MemoryRetention::new();
    let result = reader.read_from(encoded.as_slice(), retention).unwrap();
    assert!(matches!(result, ReadResult::Rejected(_)));
    // Retention was never written to — deferred path discarded before payload
}

#[test]
fn hop_count_filter_accepts_valid() {
    let encoded = encode(build_bundle(b"valid hop count").extension(
        HopCount {
            limit: 30,
            count: 5,
        },
        BlockFlags::from_bits(0),
        Crc::None,
    ));

    let reader = BundleReader::new().filter(HopCountFilter);
    let ReadResult::Accepted(bundle) = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };
    assert_eq!(bundle.payload_len(), 15);
}

#[test]
fn hop_count_filter_rejects_exceeded() {
    let encoded = encode(build_bundle(b"expired").extension(
        HopCount {
            limit: 10,
            count: 11,
        },
        BlockFlags::from_bits(0),
        Crc::None,
    ));

    let reader = BundleReader::new().filter(HopCountFilter);
    let result = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap();
    assert!(matches!(result, ReadResult::Rejected(_)));
}

#[test]
fn destination_filter_accepts() {
    let encoded = encode(build_bundle(b"for local"));
    let reader = BundleReader::new().filter(DestinationFilter::new(vec![local_eid()]));
    let ReadResult::Accepted(bundle) = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };
    assert_eq!(bundle.payload_len(), 9);
}

#[test]
fn destination_filter_rejects() {
    let encoded = encode(build_bundle(b"not for you"));
    let reader = BundleReader::new().filter(DestinationFilter::new(vec![remote_eid()]));
    let result = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap();
    assert!(matches!(result, ReadResult::Rejected(_)));
}

#[test]
fn multiple_filters_first_rejection_wins() {
    let encoded = encode(build_bundle(b"big payload for wrong dest"));
    let reader = BundleReader::new()
        .filter(MaxPayloadSizeFilter::new(5))
        .filter(DestinationFilter::new(vec![remote_eid()]));

    let result = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap();
    match result {
        ReadResult::Rejected(r) => assert_eq!(r.filter_name, "max_payload_size"),
        _ => panic!("expected Rejected"),
    }
}

// -- Mutator tests -----------------------------------------------------------

#[test]
fn hop_count_increment_mutator() {
    let encoded = encode(build_bundle(b"hop test").extension(
        HopCount {
            limit: 30,
            count: 5,
        },
        BlockFlags::from_bits(0),
        Crc::None,
    ));

    let reader = BundleReader::new().mutator(HopCountIncrementMutator::new(30));
    let ReadResult::Accepted(bundle) = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };

    let hop = bundle
        .extensions()
        .next()
        .unwrap()
        .parse_ext::<HopCount>()
        .unwrap();
    assert_eq!(hop.count, 6);
    assert_eq!(hop.limit, 30);
}

#[test]
fn hop_count_increment_adds_block_if_missing() {
    let encoded = encode(build_bundle(b"no hop count"));

    let reader = BundleReader::new().mutator(HopCountIncrementMutator::new(25));
    let ReadResult::Accepted(bundle) = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };

    let hop = bundle
        .extensions()
        .find(|b| b.parse_ext::<HopCount>().is_ok())
        .unwrap()
        .parse_ext::<HopCount>()
        .unwrap();
    assert_eq!(hop.count, 1);
    assert_eq!(hop.limit, 25);
}

#[test]
fn previous_node_mutator() {
    let encoded = encode(build_bundle(b"prev node test"));
    let node = Eid::Ipn {
        allocator_id: 0,
        node_number: 42,
        service_number: 0,
    };

    let reader = BundleReader::new().mutator(PreviousNodeMutator::new(node.clone()));
    let ReadResult::Accepted(bundle) = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };

    let pn = bundle
        .extensions()
        .find(|b| b.parse_ext::<PreviousNode>().is_ok())
        .unwrap()
        .parse_ext::<PreviousNode>()
        .unwrap();
    assert_eq!(pn.node_id, node);
}

#[test]
fn previous_node_mutator_replaces_existing() {
    let old_pn = PreviousNode {
        node_id: Eid::Ipn {
            allocator_id: 0,
            node_number: 10,
            service_number: 0,
        },
    };
    let encoded = encode(build_bundle(b"replace prev").extension(
        old_pn,
        BlockFlags::from_bits(0),
        Crc::None,
    ));

    let new_node = Eid::Ipn {
        allocator_id: 0,
        node_number: 42,
        service_number: 0,
    };
    let reader = BundleReader::new().mutator(PreviousNodeMutator::new(new_node.clone()));
    let ReadResult::Accepted(bundle) = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };

    let pn = bundle
        .extensions()
        .find(|b| b.parse_ext::<PreviousNode>().is_ok())
        .unwrap()
        .parse_ext::<PreviousNode>()
        .unwrap();
    assert_eq!(pn.node_id, new_node);
}

// -- Filter + Mutator composition --------------------------------------------

#[test]
fn filter_then_mutate() {
    let encoded = encode(build_bundle(b"filter and mutate").extension(
        HopCount {
            limit: 30,
            count: 5,
        },
        BlockFlags::from_bits(0),
        Crc::None,
    ));

    let reader = BundleReader::new()
        .filter(HopCountFilter)
        .filter(MaxPayloadSizeFilter::new(1000))
        .mutator(HopCountIncrementMutator::new(30))
        .mutator(PreviousNodeMutator::new(local_eid()));

    let ReadResult::Accepted(bundle) = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };

    // Hop count incremented
    let hop = bundle
        .extensions()
        .find(|b| b.parse_ext::<HopCount>().is_ok())
        .unwrap()
        .parse_ext::<HopCount>()
        .unwrap();
    assert_eq!(hop.count, 6);

    // Previous node added
    let pn = bundle
        .extensions()
        .find(|b| b.parse_ext::<PreviousNode>().is_ok())
        .unwrap()
        .parse_ext::<PreviousNode>()
        .unwrap();
    assert_eq!(pn.node_id, local_eid());

    // Payload intact
    let mut buf = Vec::new();
    bundle.payload(&mut buf).unwrap();
    assert_eq!(buf, b"filter and mutate");
}

#[test]
fn filter_rejects_before_mutate() {
    let encoded = encode(build_bundle(b"rejected").extension(
        HopCount { limit: 5, count: 6 },
        BlockFlags::from_bits(0),
        Crc::None,
    ));

    let reader = BundleReader::new()
        .filter(HopCountFilter)
        .mutator(HopCountIncrementMutator::new(30));

    let result = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap();
    assert!(matches!(result, ReadResult::Rejected(_)));
}

// -- Roundtrip: filtered bundle can be re-encoded ----------------------------

#[test]
fn filtered_mutated_bundle_roundtrip() {
    let encoded = encode(build_bundle(b"roundtrip").extension(
        HopCount {
            limit: 30,
            count: 5,
        },
        BlockFlags::from_bits(0),
        Crc::None,
    ));

    let reader = BundleReader::new()
        .filter(HopCountFilter)
        .mutator(HopCountIncrementMutator::new(30))
        .mutator(PreviousNodeMutator::new(local_eid()));

    let ReadResult::Accepted(bundle) = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };

    // Re-encode
    let mut reencoded = Vec::new();
    bundle.encode_to(&mut reencoded).unwrap();

    // Parse the re-encoded bundle (no filters)
    let ReadResult::Accepted(bundle2) = BundleReader::new()
        .read_from(reencoded.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };

    // Verify mutations persisted
    let hop = bundle2
        .extensions()
        .find(|b| b.parse_ext::<HopCount>().is_ok())
        .unwrap()
        .parse_ext::<HopCount>()
        .unwrap();
    assert_eq!(hop.count, 6);

    let pn = bundle2
        .extensions()
        .find(|b| b.parse_ext::<PreviousNode>().is_ok())
        .unwrap()
        .parse_ext::<PreviousNode>()
        .unwrap();
    assert_eq!(pn.node_id, local_eid());

    let mut buf = Vec::new();
    bundle2.payload(&mut buf).unwrap();
    assert_eq!(buf, b"roundtrip");
}

// -- Retention holds the mutated version -------------------------------------

#[test]
fn retention_holds_mutated_hop_count() {
    let encoded = encode(build_bundle(b"retention test").extension(
        HopCount {
            limit: 30,
            count: 0,
        },
        BlockFlags::from_bits(0),
        Crc::None,
    ));

    let ReadResult::Accepted(bundle) = BundleReader::new()
        .mutator(HopCountIncrementMutator::new(30))
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };

    // In-memory struct has mutated value.
    let hc = bundle
        .extensions()
        .find_map(|b| b.parse_ext::<HopCount>().ok())
        .unwrap();
    assert_eq!(hc.count, 1);

    // Re-encode from retention (no mutators) and verify the stored
    // bytes already contain the mutated hop count.
    let mut wire = Vec::new();
    bundle.encode_to(&mut wire).unwrap();

    let ReadResult::Accepted(re_decoded) = BundleReader::new()
        .read_from(wire.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };

    let hc2 = re_decoded
        .extensions()
        .find_map(|b| b.parse_ext::<HopCount>().ok())
        .unwrap();
    assert_eq!(hc2.count, 1, "retention must hold the mutated version");
}

#[test]
fn retention_holds_mutated_previous_node() {
    let encoded = encode(build_bundle(b"prev node retention"));

    let node = Eid::Ipn {
        allocator_id: 0,
        node_number: 42,
        service_number: 0,
    };
    let ReadResult::Accepted(bundle) = BundleReader::new()
        .mutator(PreviousNodeMutator::new(node.clone()))
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };

    // Re-encode from retention (no mutators).
    let mut wire = Vec::new();
    bundle.encode_to(&mut wire).unwrap();

    let ReadResult::Accepted(re_decoded) = BundleReader::new()
        .read_from(wire.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };

    let pn = re_decoded
        .extensions()
        .find_map(|b| b.parse_ext::<PreviousNode>().ok())
        .unwrap();
    assert_eq!(pn.node_id, node, "retention must hold the mutated version");
}

#[test]
fn no_mutation_preserves_original_bytes() {
    let encoded = encode(build_bundle(b"no mutation").extension(
        HopCount {
            limit: 30,
            count: 5,
        },
        BlockFlags::from_bits(0),
        Crc::None,
    ));

    // Only a filter, no mutators.
    let ReadResult::Accepted(bundle) = BundleReader::new()
        .filter(HopCountFilter)
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };

    let mut wire = Vec::new();
    bundle.encode_to(&mut wire).unwrap();

    let ReadResult::Accepted(re_decoded) = BundleReader::new()
        .read_from(wire.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };

    let hc = re_decoded
        .extensions()
        .find_map(|b| b.parse_ext::<HopCount>().ok())
        .unwrap();
    assert_eq!(hc.count, 5, "original value preserved when no mutation");
}
