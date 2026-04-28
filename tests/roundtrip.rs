use std::borrow::Cow;

use aqueduct::{
    BlockFlags, Bundle, BundleAge, BundleFlags, CanonicalBlock, Crc, CreationTimestamp, Eid,
    Extension, FragmentInfo, HopCount, PreviousNode,
};

// ---------------------------------------------------------------------------
// Happy-path roundtrips
// ---------------------------------------------------------------------------

#[test]
fn roundtrip_minimal_bundle() {
    let payload = b"hello";
    let bundle = Bundle::builder(Eid::Null, Eid::Null, 3_600_000_000, payload).build();
    let encoded = bundle.encode(payload);
    let decoded = Bundle::decode(&encoded).unwrap();

    assert_eq!(decoded.primary.version, 7);
    assert_eq!(decoded.primary.dest_eid, Eid::Null);
    assert_eq!(decoded.primary.lifetime, 3_600_000_000);
    assert_eq!(decoded.payload.data(&encoded), b"hello");

    let reencoded = decoded.encode(decoded.payload.data(&encoded));
    assert_eq!(encoded, reencoded);
}

#[test]
fn roundtrip_empty_payload() {
    let bundle = Bundle::builder(Eid::Null, Eid::Null, 1000, b"").build();
    let encoded = bundle.encode(b"");
    let decoded = Bundle::decode(&encoded).unwrap();

    assert_eq!(decoded.payload.data_len, 0);
    assert_eq!(decoded.payload.data(&encoded), b"");

    let reencoded = decoded.encode(decoded.payload.data(&encoded));
    assert_eq!(encoded, reencoded);
}

#[test]
fn roundtrip_with_extensions() {
    let dest = Eid::Ipn {
        allocator_id: 0,
        node_number: 1,
        service_number: 2,
    };
    let src = Eid::Ipn {
        allocator_id: 0,
        node_number: 3,
        service_number: 4,
    };
    let payload = b"hello world";

    let bundle = Bundle::builder(dest.clone(), src, 60_000_000, payload)
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
        .build();

    let encoded = bundle.encode(payload);
    let decoded = Bundle::decode(&encoded).unwrap();

    assert_eq!(decoded.primary.dest_eid, dest);
    assert_eq!(decoded.extensions.len(), 2);

    let hop = decoded.extensions[0].parse_ext::<HopCount>().unwrap();
    assert_eq!(hop.limit, 30);
    assert_eq!(hop.count, 0);

    let age = decoded.extensions[1].parse_ext::<BundleAge>().unwrap();
    assert_eq!(age.millis, 12345);

    let reencoded = decoded.encode(decoded.payload.data(&encoded));
    assert_eq!(encoded, reencoded);
}

#[test]
fn roundtrip_with_dtn_eids() {
    let payload = b"";
    let bundle = Bundle::builder(
        Eid::Dtn(Cow::Borrowed("//node1/incoming")),
        Eid::Ipn {
            allocator_id: 0,
            node_number: 42,
            service_number: 0,
        },
        0,
        payload,
    )
    .build();

    let encoded = bundle.encode(payload);
    let decoded = Bundle::decode(&encoded).unwrap();

    assert_eq!(
        decoded.primary.dest_eid,
        Eid::Dtn(Cow::Borrowed("//node1/incoming"))
    );
    assert_eq!(
        decoded.primary.src_node_id,
        Eid::Ipn {
            allocator_id: 0,
            node_number: 42,
            service_number: 0,
        }
    );
}

#[test]
fn roundtrip_fragment() {
    let payload = b"abc";
    let bundle = Bundle::builder(Eid::Null, Eid::Null, 1000, payload)
        .fragment(100, 5000)
        .build();

    let encoded = bundle.encode(payload);
    let decoded = Bundle::decode(&encoded).unwrap();

    let frag = decoded.primary.fragment.unwrap();
    assert_eq!(frag.offset, 100);
    assert_eq!(frag.total_adu_len, 5000);

    let reencoded = decoded.encode(decoded.payload.data(&encoded));
    assert_eq!(encoded, reencoded);
}

#[test]
fn roundtrip_crc_values_nonzero() {
    let payload = b"test payload";
    let bundle = Bundle::builder(Eid::Null, Eid::Null, 1000, payload).build();
    let encoded = bundle.encode(payload);
    let decoded = Bundle::decode(&encoded).unwrap();

    // Builder defaults to CRC-32C on primary
    match decoded.primary.crc {
        Crc::Crc32c(v) => assert_ne!(v, 0),
        other => panic!("expected Crc32c, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Extension roundtrips
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Error / edge-case tests
// ---------------------------------------------------------------------------

#[test]
fn decode_empty_input() {
    assert!(Bundle::decode(b"").is_err());
}

#[test]
fn decode_truncated_bundle() {
    assert!(Bundle::decode(&[0x9F, 0xFF]).is_err());
}

#[test]
fn decode_garbage() {
    assert!(Bundle::decode(&[0xDE, 0xAD, 0xBE, 0xEF]).is_err());
}

#[test]
fn decode_truncated_primary() {
    let mut data = vec![0x9F, 0x83];
    data.extend_from_slice(&[0x07, 0x00, 0x00]);
    data.push(0xFF);
    assert!(Bundle::decode(&data).is_err());
}

#[test]
fn validate_wrong_version() {
    let mut bundle = Bundle::builder(Eid::Null, Eid::Null, 1000, b"").build();
    bundle.primary.version = 6;
    assert!(bundle.primary.validate().is_err());
}

#[test]
fn validate_fragment_flag_mismatch() {
    let mut bundle = Bundle::builder(Eid::Null, Eid::Null, 1000, b"").build();
    bundle.primary.flags = BundleFlags::from_bits(0x01);
    bundle.primary.fragment = None;
    assert!(bundle.primary.validate().is_err());

    let mut bundle2 = Bundle::builder(Eid::Null, Eid::Null, 1000, b"").build();
    bundle2.primary.flags = BundleFlags::from_bits(0);
    bundle2.primary.fragment = Some(FragmentInfo {
        offset: 0,
        total_adu_len: 100,
    });
    assert!(bundle2.primary.validate().is_err());
}

#[test]
fn validate_admin_with_reports() {
    let mut bundle = Bundle::builder(Eid::Null, Eid::Null, 1000, b"").build();
    bundle.primary.flags = BundleFlags::from_bits(0x000002 | 0x004000);
    assert!(bundle.primary.validate().is_err());
}

#[test]
fn validate_null_source_constraints() {
    let mut bundle = Bundle::builder(Eid::Null, Eid::Null, 1000, b"").build();
    bundle.primary.flags = BundleFlags::from_bits(0);
    assert!(bundle.primary.validate().is_err());
}

#[test]
fn validate_duplicate_block_numbers() {
    let hc = HopCount {
        limit: 10,
        count: 0,
    };
    let mut bundle = Bundle::builder(Eid::Null, Eid::Null, 1000, b"").build();
    bundle.extensions.push(CanonicalBlock::from_ext(
        2,
        BlockFlags::from_bits(0),
        Crc::None,
        &hc,
    ));
    bundle.extensions.push(CanonicalBlock::from_ext(
        2,
        BlockFlags::from_bits(0),
        Crc::None,
        &hc,
    ));
    assert!(bundle.validate().is_err());
}

#[test]
fn validate_extension_with_payload_block_number() {
    let hc = HopCount {
        limit: 10,
        count: 0,
    };
    let mut ext = CanonicalBlock::from_ext(99, BlockFlags::from_bits(0), Crc::None, &hc);
    ext.block_number = 1;
    let mut bundle = Bundle::builder(Eid::Null, Eid::Null, 1000, b"").build();
    bundle.extensions.push(ext);
    assert!(bundle.validate().is_err());
}

#[test]
fn parse_ext_wrong_block_type() {
    let ba = BundleAge { millis: 100 };
    let block = CanonicalBlock::from_ext(2, BlockFlags::from_bits(0), Crc::None, &ba);
    assert!(block.parse_ext::<HopCount>().is_err());
}

#[test]
fn crc_tamper_detection() {
    let payload = b"important data";
    let bundle = Bundle::builder(Eid::Null, Eid::Null, 1000, payload).build();
    let mut encoded = bundle.encode(payload);

    encoded[3] ^= 0xFF;

    if let Ok(decoded) = Bundle::decode(&encoded) {
        let clean = decoded.encode(decoded.payload.data(&encoded));
        assert_ne!(encoded, clean);
    }
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
fn eid_into_owned() {
    let eid = Eid::Dtn(Cow::Borrowed("//node1/svc"));
    assert_eq!(eid.clone().into_owned(), eid);
    assert_eq!(Eid::Null.into_owned(), Eid::Null);

    let ipn = Eid::Ipn {
        allocator_id: 0,
        node_number: 1,
        service_number: 2,
    };
    assert_eq!(ipn.clone().into_owned(), ipn);
}

#[test]
fn crc_compute_invalid_type() {
    assert!(Crc::compute(3, b"data").is_err());
    assert!(Crc::compute(255, b"data").is_err());
}
