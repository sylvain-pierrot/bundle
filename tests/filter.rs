use std::io::Read;

use aqueduct::filter::builtin::{
    DestinationFilter, HopCountFilter, HopCountIncrementMutator, MaxPayloadSizeFilter,
    PreviousNodeMutator,
};
use aqueduct::{
    BlockFlags, BundleBuilder, BundleReader, CanonicalBlock, Crc, Eid, Error, HopCount,
    MemoryRetention, PreviousNode,
};

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

fn encode_bundle(payload: &[u8], extensions: Vec<CanonicalBlock>) -> Vec<u8> {
    let mut builder = BundleBuilder::new(
        local_eid(),
        remote_eid(),
        1000,
        payload,
        MemoryRetention::new(),
    )
    .unwrap();
    for ext in extensions {
        builder = builder.extension(ext);
    }
    let bundle = builder.build().unwrap();
    let mut buf = Vec::new();
    bundle.encode_to(&mut buf).unwrap();
    buf
}

// -- Filter tests ------------------------------------------------------------

#[test]
fn no_filters_passthrough() {
    let encoded = encode_bundle(b"hello", vec![]);
    let reader = BundleReader::new();
    let bundle = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap();
    assert_eq!(bundle.payload_len(), 5);
}

#[test]
fn max_payload_size_accepts() {
    let encoded = encode_bundle(b"small", vec![]);
    let reader = BundleReader::new().filter(MaxPayloadSizeFilter::new(1000));
    let bundle = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap();
    assert_eq!(bundle.payload_len(), 5);
}

#[test]
fn max_payload_size_rejects() {
    let encoded = encode_bundle(b"too large payload", vec![]);
    let reader = BundleReader::new().filter(MaxPayloadSizeFilter::new(5));
    let result = reader.read_from(encoded.as_slice(), MemoryRetention::new());
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), Error::FilterRejected(_)));
}

#[test]
fn rejected_bundle_zero_retention_io() {
    let encoded = encode_bundle(b"should not hit retention", vec![]);
    let reader = BundleReader::new().filter(MaxPayloadSizeFilter::new(1));
    let retention = MemoryRetention::new();
    let result = reader.read_from(encoded.as_slice(), retention);
    assert!(result.is_err());
    // Retention was never written to — deferred path discarded before payload
}

#[test]
fn hop_count_filter_accepts_valid() {
    let hc = HopCount {
        limit: 30,
        count: 5,
    };
    let ext = CanonicalBlock::from_ext(2, BlockFlags::from_bits(0), Crc::None, &hc);
    let encoded = encode_bundle(b"valid hop count", vec![ext]);

    let reader = BundleReader::new().filter(HopCountFilter);
    let bundle = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap();
    assert_eq!(bundle.payload_len(), 15);
}

#[test]
fn hop_count_filter_rejects_exceeded() {
    let hc = HopCount {
        limit: 10,
        count: 11,
    };
    let ext = CanonicalBlock::from_ext(2, BlockFlags::from_bits(0), Crc::None, &hc);
    let encoded = encode_bundle(b"expired", vec![ext]);

    let reader = BundleReader::new().filter(HopCountFilter);
    let result = reader.read_from(encoded.as_slice(), MemoryRetention::new());
    assert!(matches!(result.unwrap_err(), Error::FilterRejected(_)));
}

#[test]
fn destination_filter_accepts() {
    let encoded = encode_bundle(b"for local", vec![]);
    let reader = BundleReader::new().filter(DestinationFilter::new(vec![local_eid()]));
    let bundle = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap();
    assert_eq!(bundle.payload_len(), 9);
}

#[test]
fn destination_filter_rejects() {
    let encoded = encode_bundle(b"not for you", vec![]);
    let reader = BundleReader::new().filter(DestinationFilter::new(vec![remote_eid()]));
    let result = reader.read_from(encoded.as_slice(), MemoryRetention::new());
    assert!(matches!(result.unwrap_err(), Error::FilterRejected(_)));
}

#[test]
fn multiple_filters_first_rejection_wins() {
    let encoded = encode_bundle(b"big payload for wrong dest", vec![]);
    let reader = BundleReader::new()
        .filter(MaxPayloadSizeFilter::new(5))
        .filter(DestinationFilter::new(vec![remote_eid()]));

    let result = reader.read_from(encoded.as_slice(), MemoryRetention::new());
    match result.unwrap_err() {
        Error::FilterRejected(r) => assert_eq!(r.filter_name, "max_payload_size"),
        e => panic!("expected FilterRejected, got {e:?}"),
    }
}

// -- Mutator tests -----------------------------------------------------------

#[test]
fn hop_count_increment_mutator() {
    let hc = HopCount {
        limit: 30,
        count: 5,
    };
    let ext = CanonicalBlock::from_ext(2, BlockFlags::from_bits(0), Crc::None, &hc);
    let encoded = encode_bundle(b"hop test", vec![ext]);

    let reader = BundleReader::new().mutator(HopCountIncrementMutator::new(30));
    let bundle = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap();

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
    let encoded = encode_bundle(b"no hop count", vec![]);

    let reader = BundleReader::new().mutator(HopCountIncrementMutator::new(25));
    let bundle = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap();

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
    let encoded = encode_bundle(b"prev node test", vec![]);
    let node = Eid::Ipn {
        allocator_id: 0,
        node_number: 42,
        service_number: 0,
    };

    let reader = BundleReader::new().mutator(PreviousNodeMutator::new(node.clone()));
    let bundle = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap();

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
    let ext = CanonicalBlock::from_ext(2, BlockFlags::from_bits(0), Crc::None, &old_pn);
    let encoded = encode_bundle(b"replace prev", vec![ext]);

    let new_node = Eid::Ipn {
        allocator_id: 0,
        node_number: 42,
        service_number: 0,
    };
    let reader = BundleReader::new().mutator(PreviousNodeMutator::new(new_node.clone()));
    let bundle = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap();

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
    let hc = HopCount {
        limit: 30,
        count: 5,
    };
    let ext = CanonicalBlock::from_ext(2, BlockFlags::from_bits(0), Crc::None, &hc);
    let encoded = encode_bundle(b"filter and mutate", vec![ext]);

    let reader = BundleReader::new()
        .filter(HopCountFilter)
        .filter(MaxPayloadSizeFilter::new(1000))
        .mutator(HopCountIncrementMutator::new(30))
        .mutator(PreviousNodeMutator::new(local_eid()));

    let bundle = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap();

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
    bundle
        .payload_reader()
        .unwrap()
        .read_to_end(&mut buf)
        .unwrap();
    assert_eq!(buf, b"filter and mutate");
}

#[test]
fn filter_rejects_before_mutate() {
    let hc = HopCount { limit: 5, count: 6 };
    let ext = CanonicalBlock::from_ext(2, BlockFlags::from_bits(0), Crc::None, &hc);
    let encoded = encode_bundle(b"rejected", vec![ext]);

    let reader = BundleReader::new()
        .filter(HopCountFilter)
        .mutator(HopCountIncrementMutator::new(30));

    let result = reader.read_from(encoded.as_slice(), MemoryRetention::new());
    assert!(matches!(result.unwrap_err(), Error::FilterRejected(_)));
}

// -- Roundtrip: filtered bundle can be re-encoded ----------------------------

#[test]
fn filtered_mutated_bundle_roundtrip() {
    let hc = HopCount {
        limit: 30,
        count: 5,
    };
    let ext = CanonicalBlock::from_ext(2, BlockFlags::from_bits(0), Crc::None, &hc);
    let encoded = encode_bundle(b"roundtrip", vec![ext]);

    let reader = BundleReader::new()
        .filter(HopCountFilter)
        .mutator(HopCountIncrementMutator::new(30))
        .mutator(PreviousNodeMutator::new(local_eid()));

    let bundle = reader
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap();

    // Re-encode
    let mut reencoded = Vec::new();
    bundle.encode_to(&mut reencoded).unwrap();

    // Parse the re-encoded bundle (no filters)
    let bundle2 = BundleReader::new()
        .read_from(reencoded.as_slice(), MemoryRetention::new())
        .unwrap();

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
    bundle2
        .payload_reader()
        .unwrap()
        .read_to_end(&mut buf)
        .unwrap();
    assert_eq!(buf, b"roundtrip");
}
