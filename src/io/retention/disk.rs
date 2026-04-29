use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use super::Retention;

const WRITE_BUF_SIZE: usize = 256 * 1024;

/// Disk-backed retention. Writes are buffered for throughput.
/// Reads open a new file handle and seek.
pub struct DiskRetention {
    path: PathBuf,
    file: BufWriter<File>,
}

impl DiskRetention {
    pub fn new(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)?;
        Ok(Self {
            path,
            file: BufWriter::with_capacity(WRITE_BUF_SIZE, file),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Write for DiskRetention {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.file.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()?;
        self.file.get_ref().sync_data()
    }
}

const READ_BUF_SIZE: usize = 256 * 1024;

/// Buffered reader for a byte range from a [`DiskRetention`] file.
pub struct DiskReader {
    file: BufReader<File>,
    remaining: u64,
}

impl Read for DiskReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.remaining == 0 {
            return Ok(0);
        }
        let max = buf.len().min(self.remaining as usize);
        let n = self.file.read(&mut buf[..max])?;
        self.remaining -= n as u64;
        Ok(n)
    }
}

impl Retention for DiskRetention {
    type Reader<'a> = DiskReader;

    fn reader(&self, offset: u64, len: u64) -> Self::Reader<'_> {
        let mut file = File::open(&self.path).unwrap_or_else(|e| {
            panic!("failed to open retention file {}: {e}", self.path.display())
        });
        file.seek(SeekFrom::Start(offset)).unwrap_or_else(|e| {
            panic!(
                "failed to seek retention file {} to offset {offset}: {e}",
                self.path.display()
            )
        });
        DiskReader {
            file: std::io::BufReader::with_capacity(READ_BUF_SIZE, file),
            remaining: len,
        }
    }
}
