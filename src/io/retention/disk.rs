use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use super::Retention;

const WRITE_BUF_SIZE: usize = 256 * 1024;
const READ_BUF_SIZE: usize = 256 * 1024;

/// Disk-backed retention. Writes are buffered. Reads use `try_clone`
/// to avoid re-opening the file.
pub struct DiskRetention {
    path: PathBuf,
    writer: BufWriter<File>,
    reader_handle: File,
}

impl DiskRetention {
    pub fn new(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(true)
            .open(&path)?;
        let reader_handle = file.try_clone()?;
        Ok(Self {
            path,
            writer: BufWriter::with_capacity(WRITE_BUF_SIZE, file),
            reader_handle,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Write for DiskRetention {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.writer.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()?;
        self.writer.get_ref().sync_data()
    }
}

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

    fn reader(&self, offset: u64, len: u64) -> std::io::Result<Self::Reader<'_>> {
        let mut file = self.reader_handle.try_clone()?;
        file.seek(SeekFrom::Start(offset))?;
        Ok(DiskReader {
            file: BufReader::with_capacity(READ_BUF_SIZE, file),
            remaining: len,
        })
    }
}
