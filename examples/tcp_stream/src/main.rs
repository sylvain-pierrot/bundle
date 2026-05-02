//! Send and receive a BPv7 bundle over TCP.
//!
//! Run: cargo run -p tcp-stream

use std::net::{TcpListener, TcpStream};
use std::thread;

use bundle::{BundleBuilder, BundleReader, BundleWriter, MemoryRetention, ReadResult};
use bundle_bpv7::Eid;

fn main() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    // Receive
    let receiver = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();

        let reader = BundleReader::new();

        let retention = MemoryRetention::new();
        let ReadResult::Accepted(bundle) = reader.read_from(stream, retention).unwrap() else {
            panic!("expected accepted");
        };

        let mut payload = Vec::new();
        bundle.payload(&mut payload).unwrap();

        println!("Received: {:?}", std::str::from_utf8(&payload).unwrap());
    });

    // Send
    let stream = TcpStream::connect(addr).unwrap();

    let dest = Eid::Ipn {
        allocator_id: 0,
        node_number: 1,
        service_number: 1,
    };
    let src = Eid::Ipn {
        allocator_id: 0,
        node_number: 2,
        service_number: 1,
    };

    let retention = MemoryRetention::new();
    let bundle = BundleBuilder::new(dest, src, 3_600_000, b"hello over TCP", retention)
        .build()
        .unwrap();

    let writer = BundleWriter::new();
    writer.write_to(&bundle, stream).unwrap();

    receiver.join().unwrap();
}
