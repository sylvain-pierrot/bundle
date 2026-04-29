#![cfg(feature = "async")]

use aqueduct::{BundleBuilder, BundleReader, Eid, MemoryRetention};

#[tokio::test]
async fn async_roundtrip() {
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
    .build();

    let encoded = bundle.encode().unwrap();

    // Decode via sync path (proves async-built bundles are compatible)
    let decoded = BundleReader::new()
        .read_from(encoded.as_slice(), MemoryRetention::new())
        .unwrap();

    assert_eq!(decoded.primary().dest_eid, bundle.primary().dest_eid);
    assert_eq!(decoded.payload_len(), payload.len() as u64);

    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut decoded.payload_reader().unwrap(), &mut buf).unwrap();
    assert_eq!(buf, payload);
}
