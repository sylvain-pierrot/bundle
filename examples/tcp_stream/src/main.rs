//! TCP streaming example with mutation.
//!
//! Demonstrates sending and receiving a BPv7 bundle over a TCP connection.
//! The receiver has a HopCountIncrementMutator configured, so the hop count
//! is incremented during reception. The mutated version is stored in retention.
//!
//! Run: cargo run -p tcp-stream

use std::net::{TcpListener, TcpStream};
use std::thread;

use bundle::{BundleBuilder, BundleReader, BundleWriter, MemoryRetention};
use bundle_bpv7::filter::builtin::HopCountIncrementMutator;
use bundle_bpv7::{Eid, HopCount};
use bundle_io::Read;

fn main() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let receiver = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();

        // Reader with hop count mutator: increments on receive.
        let bundle = BundleReader::new()
            .mutator(HopCountIncrementMutator::new(30))
            .read_from(stream, MemoryRetention::new())
            .unwrap();

        // Check the hop count was mutated.
        let hc = bundle
            .blocks()
            .iter()
            .find_map(|b| b.parse_ext::<HopCount>().ok())
            .expect("hop count block should exist");

        println!("Received bundle:");
        println!("  dest:      {:?}", bundle.primary().dest_eid);
        println!("  src:       {:?}", bundle.primary().src_node_id);
        println!("  hop count: {}/{}", hc.count, hc.limit);
        assert_eq!(hc.count, 1, "hop count should be 1 after mutation");
        assert_eq!(hc.limit, 30);

        let mut payload = Vec::new();
        bundle
            .payload_reader()
            .unwrap()
            .read_to_end(&mut payload)
            .unwrap();
        println!("  payload:   {:?}", std::str::from_utf8(&payload).unwrap());

        // Verify the mutated version is in retention by re-encoding
        // from retention and decoding again (without mutators).
        let mut wire = Vec::new();
        BundleWriter::new().write_to(&bundle, &mut wire).unwrap();

        let re_decoded = BundleReader::new()
            .read_from(wire.as_slice(), MemoryRetention::new())
            .unwrap();

        let hc2 = re_decoded
            .blocks()
            .iter()
            .find_map(|b| b.parse_ext::<HopCount>().ok())
            .expect("hop count block should survive roundtrip");

        assert_eq!(hc2.count, 1, "retention should hold the mutated version");
        println!(
            "  roundtrip: hop count still {}/{} (mutated version in retention)",
            hc2.count, hc2.limit
        );
    });

    // Sender: build a bundle with a hop count block (count=0).
    let stream = TcpStream::connect(addr).unwrap();

    let hc = HopCount {
        limit: 30,
        count: 0,
    };
    let bundle = BundleBuilder::new(
        Eid::Ipn {
            allocator_id: 0,
            node_number: 1,
            service_number: 1,
        },
        Eid::Ipn {
            allocator_id: 0,
            node_number: 2,
            service_number: 1,
        },
        3_600_000,
        b"hello over TCP",
        MemoryRetention::new(),
    )
    .unwrap()
    .extension(hc)
    .build()
    .unwrap();

    BundleWriter::new().write_to(&bundle, stream).unwrap();
    println!("Sent bundle to {addr} with hop count 0/30");

    receiver.join().unwrap();
    println!("OK: mutation verified");
}
