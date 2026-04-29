//! Storage backends for bundle retention.

mod disk;
mod memory;

use std::io::{Read, Write};

pub use disk::DiskRetention;
pub use memory::MemoryRetention;

/// Storage backend where bundle bytes are retained.
///
/// The retention itself is the writer — bytes are written directly
/// via the [`Write`] impl. Later, byte ranges can be read back via
/// [`reader`](Self::reader).
pub trait Retention: Write {
    type Reader<'a>: Read
    where
        Self: 'a;

    fn reader(&self, offset: u64, len: u64) -> Self::Reader<'_>;
}
