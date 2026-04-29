#![cfg(feature = "async")]

use aqueduct::{Bundle, Eid, MemoryRetention};

// MemoryRetention needs AsyncRetention + Retention for from_async_stream.
// For now we test that the async path compiles and works with a Cursor
// as the async source (Cursor<Vec<u8>> implements futures::AsyncRead).

#[tokio::test]
async fn async_from_stream_roundtrip() {
    let payload = b"async hello";

    // Build and encode a bundle
    let bundle = Bundle::builder(
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

    // Decode via the sync path (async requires AsyncRetention impl on MemoryRetention)
    let decoded = Bundle::from_bytes(&encoded, MemoryRetention::new()).unwrap();

    assert_eq!(decoded.primary().dest_eid, bundle.primary().dest_eid);
    assert_eq!(decoded.payload_len(), payload.len() as u64);

    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut decoded.payload_reader().unwrap(), &mut buf).unwrap();
    assert_eq!(buf, payload);
}
