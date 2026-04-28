use std::borrow::Cow;

use aqueduct::{
    BlockFlags, Bundle, BundleAge, BundleFlags, CanonicalBlock, Crc, CreationTimestamp, Eid,
    Extension, FragmentInfo, HopCount, PayloadRef, PreviousNode, PrimaryBlock,
};

fn minimal_bundle<'a>(payload: &[u8], crc: Crc, payload_crc: Crc) -> Bundle<'a> {
    Bundle {
        primary: PrimaryBlock {
            version: 7,
            flags: BundleFlags::from_bits(0),
            crc,
            dest_eid: Eid::Null,
            src_node_id: Eid::Null,
            rpt_eid: Eid::Null,
            creation_ts: CreationTimestamp { time: 0, seq: 0 },
            lifetime: 3_600_000_000,
            fragment: None,
        },
        extensions: vec![],
        payload: PayloadRef {
            flags: BlockFlags::from_bits(0),
            crc: payload_crc,
            data_offset: 0,
            data_len: payload.len() as u64,
        },
    }
}

// ---------------------------------------------------------------------------
// Happy-path roundtrips
// ---------------------------------------------------------------------------

#[test]
fn roundtrip_minimal_bundle() {
    let payload = b"hello";
    let bundle = minimal_bundle(payload, Crc::None, Crc::None);
    let encoded = bundle.encode(payload);
    let decoded = Bundle::decode(&encoded).expect("decode failed");

    assert_eq!(decoded.primary.version, 7);
    assert_eq!(decoded.primary.dest_eid, Eid::Null);
    assert_eq!(decoded.primary.lifetime, 3_600_000_000);
    assert_eq!(decoded.payload.data_len, 5);
    assert_eq!(decoded.payload.data(&encoded), b"hello");

    let reencoded = decoded.encode(decoded.payload.data(&encoded));
    assert_eq!(encoded, reencoded);
}

#[test]
fn roundtrip_empty_payload() {
    let bundle = minimal_bundle(b"", Crc::None, Crc::None);
    let encoded = bundle.encode(b"");
    let decoded = Bundle::decode(&encoded).unwrap();

    assert_eq!(decoded.payload.data_len, 0);
    assert_eq!(decoded.payload.data(&encoded), b"");

    let reencoded = decoded.encode(decoded.payload.data(&encoded));
    assert_eq!(encoded, reencoded);
}

#[test]
fn roundtrip_with_extensions() {
    let hop_count = HopCount {
        limit: 30,
        count: 0,
    };
    let bundle_age = BundleAge { millis: 12345 };

    let bundle = Bundle {
        primary: PrimaryBlock {
            version: 7,
            flags: BundleFlags::from_bits(0),
            crc: Crc::None,
            dest_eid: Eid::Ipn {
                allocator_id: 0,
                node_number: 1,
                service_number: 2,
            },
            src_node_id: Eid::Ipn {
                allocator_id: 0,
                node_number: 3,
                service_number: 4,
            },
            rpt_eid: Eid::Null,
            creation_ts: CreationTimestamp { time: 1000, seq: 1 },
            lifetime: 60_000_000,
            fragment: None,
        },
        extensions: vec![
            CanonicalBlock::from_ext(2, BlockFlags::from_bits(0), Crc::None, &hop_count),
            CanonicalBlock::from_ext(3, BlockFlags::from_bits(0), Crc::None, &bundle_age),
        ],
        payload: PayloadRef {
            flags: BlockFlags::from_bits(0),
            crc: Crc::None,
            data_offset: 0,
            data_len: 11,
        },
    };

    let payload = b"hello world";
    let encoded = bundle.encode(payload);
    let decoded = Bundle::decode(&encoded).unwrap();

    assert_eq!(
        decoded.primary.dest_eid,
        Eid::Ipn {
            allocator_id: 0,
            node_number: 1,
            service_number: 2,
        }
    );
    assert_eq!(decoded.extensions.len(), 2);

    let hop = decoded.extensions[0].parse_ext::<HopCount>().unwrap();
    assert_eq!(hop.limit, 30);
    assert_eq!(hop.count, 0);

    let age = decoded.extensions[1].parse_ext::<BundleAge>().unwrap();
    assert_eq!(age.millis, 12345);

    assert_eq!(decoded.payload.data(&encoded), b"hello world");

    let reencoded = decoded.encode(decoded.payload.data(&encoded));
    assert_eq!(encoded, reencoded);
}

#[test]
fn roundtrip_with_crc16() {
    let payload = b"hello";
    let bundle = minimal_bundle(payload, Crc::crc16(), Crc::crc16());

    let encoded = bundle.encode(payload);
    let decoded = Bundle::decode(&encoded).unwrap();

    assert_eq!(decoded.primary.crc.crc_type(), 1);
    assert!(!decoded.primary.crc.is_none());
    assert_eq!(decoded.payload.crc.crc_type(), 1);

    let reencoded = decoded.encode(decoded.payload.data(&encoded));
    assert_eq!(encoded, reencoded);
}

#[test]
fn roundtrip_with_crc32c() {
    let bundle = Bundle {
        primary: PrimaryBlock {
            version: 7,
            flags: BundleFlags::from_bits(0),
            crc: Crc::crc32c(),
            dest_eid: Eid::Dtn(Cow::Borrowed("//node1/svc")),
            src_node_id: Eid::Dtn(Cow::Borrowed("//node2/svc")),
            rpt_eid: Eid::Null,
            creation_ts: CreationTimestamp { time: 1000, seq: 0 },
            lifetime: 3_600_000_000,
            fragment: None,
        },
        extensions: vec![],
        payload: PayloadRef {
            flags: BlockFlags::from_bits(0),
            crc: Crc::crc32c(),
            data_offset: 0,
            data_len: 5,
        },
    };

    let encoded = bundle.encode(b"world");
    let decoded = Bundle::decode(&encoded).unwrap();

    assert_eq!(decoded.primary.crc.crc_type(), 2);
    assert_eq!(
        decoded.primary.dest_eid,
        Eid::Dtn(Cow::Borrowed("//node1/svc"))
    );

    let reencoded = decoded.encode(decoded.payload.data(&encoded));
    assert_eq!(encoded, reencoded);
}

#[test]
fn roundtrip_dtn_eids() {
    let bundle = Bundle {
        primary: PrimaryBlock {
            version: 7,
            flags: BundleFlags::from_bits(0),
            crc: Crc::None,
            dest_eid: Eid::Dtn(Cow::Borrowed("//node1/incoming")),
            src_node_id: Eid::Ipn {
                allocator_id: 0,
                node_number: 42,
                service_number: 0,
            },
            rpt_eid: Eid::Null,
            creation_ts: CreationTimestamp { time: 0, seq: 0 },
            lifetime: 0,
            fragment: None,
        },
        extensions: vec![],
        payload: PayloadRef {
            flags: BlockFlags::from_bits(0),
            crc: Crc::None,
            data_offset: 0,
            data_len: 0,
        },
    };

    let encoded = bundle.encode(b"");
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
            service_number: 0
        }
    );
}

#[test]
fn roundtrip_fragment() {
    let bundle = Bundle {
        primary: PrimaryBlock {
            version: 7,
            flags: BundleFlags::from_bits(0x01),
            crc: Crc::None,
            dest_eid: Eid::Null,
            src_node_id: Eid::Null,
            rpt_eid: Eid::Null,
            creation_ts: CreationTimestamp { time: 0, seq: 0 },
            lifetime: 1000,
            fragment: Some(FragmentInfo {
                offset: 100,
                total_adu_len: 5000,
            }),
        },
        extensions: vec![],
        payload: PayloadRef {
            flags: BlockFlags::from_bits(0),
            crc: Crc::None,
            data_offset: 0,
            data_len: 3,
        },
    };

    let encoded = bundle.encode(b"abc");
    let decoded = Bundle::decode(&encoded).unwrap();

    let frag = decoded.primary.fragment.unwrap();
    assert_eq!(frag.offset, 100);
    assert_eq!(frag.total_adu_len, 5000);

    let reencoded = decoded.encode(decoded.payload.data(&encoded));
    assert_eq!(encoded, reencoded);
}

#[test]
fn extension_hop_count_roundtrip() {
    let hc = HopCount {
        limit: 30,
        count: 5,
    };
    let data = hc.encode_data();
    let parsed = HopCount::parse(&data).unwrap();
    assert_eq!(parsed, hc);
}

#[test]
fn extension_bundle_age_roundtrip() {
    let ba = BundleAge { millis: 999999 };
    let data = ba.encode_data();
    let parsed = BundleAge::parse(&data).unwrap();
    assert_eq!(parsed, ba);
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
    let parsed = PreviousNode::parse(&data).unwrap();
    assert_eq!(parsed.node_id, pn.node_id);
}

#[test]
fn crc_values_are_nonzero() {
    let payload = b"test payload";
    let bundle = minimal_bundle(payload, Crc::crc16(), Crc::crc16());
    let encoded = bundle.encode(payload);
    let decoded = Bundle::decode(&encoded).unwrap();

    match decoded.primary.crc {
        Crc::Crc16(v) => assert_ne!(v, 0),
        _ => panic!("expected Crc16"),
    }
    match decoded.payload.crc {
        Crc::Crc16(v) => assert_ne!(v, 0),
        _ => panic!("expected Crc16"),
    }
}

#[test]
fn extension_block_with_crc() {
    let hc = HopCount {
        limit: 10,
        count: 3,
    };
    let bundle = Bundle {
        primary: PrimaryBlock {
            version: 7,
            flags: BundleFlags::from_bits(0),
            crc: Crc::None,
            dest_eid: Eid::Null,
            src_node_id: Eid::Null,
            rpt_eid: Eid::Null,
            creation_ts: CreationTimestamp { time: 0, seq: 0 },
            lifetime: 1000,
            fragment: None,
        },
        extensions: vec![CanonicalBlock::from_ext(
            2,
            BlockFlags::from_bits(0),
            Crc::crc32c(),
            &hc,
        )],
        payload: PayloadRef {
            flags: BlockFlags::from_bits(0),
            crc: Crc::None,
            data_offset: 0,
            data_len: 0,
        },
    };

    let encoded = bundle.encode(b"");
    let decoded = Bundle::decode(&encoded).unwrap();

    assert_eq!(decoded.extensions.len(), 1);
    assert_eq!(decoded.extensions[0].crc.crc_type(), 2);

    let hop = decoded.extensions[0].parse_ext::<HopCount>().unwrap();
    assert_eq!(hop.limit, 10);
    assert_eq!(hop.count, 3);

    let reencoded = decoded.encode(decoded.payload.data(&encoded));
    assert_eq!(encoded, reencoded);
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
    // Just the indefinite array start, no blocks
    assert!(Bundle::decode(&[0x9F, 0xFF]).is_err());
}

#[test]
fn decode_garbage() {
    assert!(Bundle::decode(&[0xDE, 0xAD, 0xBE, 0xEF]).is_err());
}

#[test]
fn decode_truncated_primary() {
    // indefinite array start + start of a too-short array
    let mut data = vec![0x9F, 0x83]; // indefinite array, then 3-element array (too few for primary)
    data.extend_from_slice(&[0x07, 0x00, 0x00]); // version, flags, crc_type
    data.push(0xFF); // break
    assert!(Bundle::decode(&data).is_err());
}

#[test]
fn validate_wrong_version() {
    let mut bundle = minimal_bundle(b"", Crc::None, Crc::None);
    bundle.primary.version = 6;
    assert!(bundle.primary.validate().is_err());
}

#[test]
fn validate_fragment_flag_mismatch() {
    // is_fragment flag set but no FragmentInfo
    let mut bundle = minimal_bundle(b"", Crc::None, Crc::None);
    bundle.primary.flags = BundleFlags::from_bits(0x01);
    bundle.primary.fragment = None;
    assert!(bundle.primary.validate().is_err());

    // FragmentInfo present but is_fragment flag not set
    let mut bundle2 = minimal_bundle(b"", Crc::None, Crc::None);
    bundle2.primary.flags = BundleFlags::from_bits(0);
    bundle2.primary.fragment = Some(FragmentInfo {
        offset: 0,
        total_adu_len: 100,
    });
    assert!(bundle2.primary.validate().is_err());
}

#[test]
fn validate_admin_with_reports() {
    // Admin flag + report flag is invalid
    let mut bundle = minimal_bundle(b"", Crc::None, Crc::None);
    bundle.primary.flags = BundleFlags::from_bits(0x000002 | 0x004000); // is_admin | rpt_reception
    assert!(bundle.primary.validate().is_err());
}

#[test]
fn validate_null_source_constraints() {
    // Null source must have no_fragment set and no reports
    let mut bundle = minimal_bundle(b"", Crc::None, Crc::None);
    bundle.primary.src_node_id = Eid::Null;
    bundle.primary.flags = BundleFlags::from_bits(0); // missing no_fragment
    assert!(bundle.primary.validate().is_err());
}

#[test]
fn validate_duplicate_block_numbers() {
    let hc = HopCount {
        limit: 10,
        count: 0,
    };
    let mut bundle = minimal_bundle(b"", Crc::None, Crc::None);
    bundle.extensions.push(CanonicalBlock::from_ext(
        2,
        BlockFlags::from_bits(0),
        Crc::None,
        &hc,
    ));
    bundle.extensions.push(CanonicalBlock::from_ext(
        2, // same block number
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
    ext.block_number = 1; // payload block number — not allowed for extensions
    let mut bundle = minimal_bundle(b"", Crc::None, Crc::None);
    bundle.extensions.push(ext);
    assert!(bundle.validate().is_err());
}

#[test]
fn parse_ext_wrong_block_type() {
    let ba = BundleAge { millis: 100 };
    let block = CanonicalBlock::from_ext(2, BlockFlags::from_bits(0), Crc::None, &ba);
    // block_type is 7 (BundleAge), try parsing as HopCount (type 10)
    assert!(block.parse_ext::<HopCount>().is_err());
}

#[test]
fn crc_tamper_detection() {
    let payload = b"important data";
    let bundle = minimal_bundle(payload, Crc::crc16(), Crc::None);
    let mut encoded = bundle.encode(payload);

    // Tamper with a byte in the primary block area (after the indefinite array marker)
    encoded[3] ^= 0xFF;

    // Decoding may succeed (we don't verify CRC on decode) but the CRC
    // should not match if we re-encode
    if let Ok(decoded) = Bundle::decode(&encoded) {
        let clean = decoded.encode(decoded.payload.data(&encoded));
        // Tampered bytes means re-encode won't match
        assert_ne!(encoded, clean);
    }
    // If decode itself fails due to structural corruption, that's also acceptable
}

#[test]
fn hop_count_exceeded() {
    let hc = HopCount {
        limit: 10,
        count: 11,
    };
    assert!(hc.exceeded());

    let hc2 = HopCount {
        limit: 10,
        count: 10,
    };
    assert!(!hc2.exceeded());
}

#[test]
fn eid_into_owned() {
    let eid = Eid::Dtn(Cow::Borrowed("//node1/svc"));
    let owned = eid.clone().into_owned();
    assert_eq!(eid, owned);

    let null_owned = Eid::Null.into_owned();
    assert_eq!(null_owned, Eid::Null);

    let ipn = Eid::Ipn {
        allocator_id: 0,
        node_number: 1,
        service_number: 2,
    };
    assert_eq!(ipn.clone().into_owned(), ipn);
}

#[test]
fn crc_type_codes() {
    assert_eq!(Crc::None.crc_type(), 0);
    assert!(Crc::None.is_none());
    assert_eq!(Crc::crc16().crc_type(), 1);
    assert!(!Crc::crc16().is_none());
    assert_eq!(Crc::crc32c().crc_type(), 2);
    assert!(!Crc::crc32c().is_none());
}

#[test]
fn crc_compute_invalid_type() {
    assert!(Crc::compute(3, b"data").is_err());
    assert!(Crc::compute(255, b"data").is_err());
}
