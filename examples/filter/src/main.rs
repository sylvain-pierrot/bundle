//! Filtering and mutation on receive and send paths.
//!
//! - BundleReader: filters reject before payload touches storage.
//! - BundleWriter: hop count and previous node are stamped on forward.
//!
//! Run: cargo run -p filter-example

use bundle::{BundleBuilder, BundleReader, BundleWriter, MemoryRetention, ReadResult};
use bundle_bpv7::filter::builtin::{
    HopCountFilter, HopCountIncrementMutator, MaxPayloadSizeFilter, PreviousNodeMutator,
};
use bundle_bpv7::{Eid, HopCount, PreviousNode};

fn main() {
    let local = Eid::Ipn {
        allocator_id: 0,
        node_number: 1,
        service_number: 0,
    };
    let remote = Eid::Ipn {
        allocator_id: 0,
        node_number: 99,
        service_number: 1,
    };

    // Build a bundle with a hop count.
    let bundle = BundleBuilder::new(
        local.clone(),
        remote.clone(),
        1000,
        b"hello DTN",
        MemoryRetention::new(),
    )
    .unwrap()
    .extension(HopCount {
        limit: 30,
        count: 5,
    })
    .build()
    .unwrap();

    let mut wire = Vec::new();
    bundle.encode_to(&mut wire).unwrap();

    // --- Receive: filter only ---

    let reader = BundleReader::new()
        .filter(MaxPayloadSizeFilter::new(1_000_000))
        .filter(HopCountFilter);

    let ReadResult::Accepted(received) = reader
        .read_from(wire.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };

    let hc = received
        .extensions()
        .find_map(|b| b.parse_ext::<HopCount>().ok())
        .unwrap();
    println!("Received: hop count {}/{} (unchanged)", hc.count, hc.limit);

    // --- Forward: increment hop count + stamp previous node ---

    let writer = BundleWriter::new()
        .mutator(HopCountIncrementMutator::new(30))
        .mutator(PreviousNodeMutator::new(local.clone()));

    let mut forwarded = Vec::new();
    writer.write_to(&received, &mut forwarded).unwrap();

    // Next hop sees the mutated bundle.
    let ReadResult::Accepted(next_hop) = BundleReader::new()
        .read_from(forwarded.as_slice(), MemoryRetention::new())
        .unwrap()
    else {
        panic!("expected accepted");
    };

    let hc = next_hop
        .extensions()
        .find_map(|b| b.parse_ext::<HopCount>().ok())
        .unwrap();
    println!(
        "Forwarded: hop count {}/{} (incremented)",
        hc.count, hc.limit
    );

    let pn = next_hop
        .extensions()
        .find_map(|b| b.parse_ext::<PreviousNode>().ok())
        .unwrap();
    println!("Forwarded: previous node = {:?}", pn.node_id);

    // --- Rejected bundle: zero storage I/O ---

    let big = BundleBuilder::new(
        local,
        remote,
        1000,
        &vec![0u8; 2_000_000],
        MemoryRetention::new(),
    )
    .unwrap()
    .build()
    .unwrap();

    let mut big_wire = Vec::new();
    big.encode_to(&mut big_wire).unwrap();

    let result = reader
        .read_from(big_wire.as_slice(), MemoryRetention::new())
        .unwrap();
    match result {
        ReadResult::Rejected(r) => println!("Rejected: {}", r.filter_name),
        _ => panic!("expected Rejected"),
    }
}
