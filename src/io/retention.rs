//! Storage backend trait for bundle retention.

use std::cell::RefCell;
use std::io::{self, Cursor, Read, Write};
use std::rc::Rc;

/// Storage backend where bundle bytes are retained.
pub trait Retention {
    type Reader<'a>: Read
    where
        Self: 'a;
    type Writer: Write;

    fn writer(&self) -> io::Result<Self::Writer>;
    fn reader(&self, offset: u64, len: u64) -> io::Result<Self::Reader<'_>>;
}

/// In-memory retention backed by a shared `Vec<u8>`.
#[derive(Default, Debug, Clone)]
pub struct MemoryRetention {
    data: Rc<RefCell<Vec<u8>>>,
}

impl MemoryRetention {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn as_bytes(&self) -> impl std::ops::Deref<Target = Vec<u8>> + '_ {
        self.data.borrow()
    }
}

impl Write for MemoryRetention {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.data.borrow_mut().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Retention for MemoryRetention {
    type Reader<'a> = Cursor<Vec<u8>>;
    type Writer = Self;

    fn writer(&self) -> io::Result<Self::Writer> {
        self.data.borrow_mut().clear();
        Ok(self.clone())
    }

    fn reader(&self, offset: u64, len: u64) -> io::Result<Self::Reader<'_>> {
        let data = self.data.borrow();
        let start = offset as usize;
        let end = start + len as usize;
        if end > data.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "offset + len exceeds stored data",
            ));
        }
        Ok(Cursor::new(data[start..end].to_vec()))
    }
}
