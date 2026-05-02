use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use bundle_io::{Error as IoError, Read as AqRead, Write as AqWrite};

use super::Retention;

const WRITE_BUF_SIZE: usize = 512 * 1024;
const READ_BUF_SIZE: usize = 512 * 1024;

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

impl AqWrite for DiskRetention {
    fn write_all(&mut self, buf: &[u8]) -> Result<(), IoError> {
        io::Write::write_all(&mut self.writer, buf).map_err(IoError::Io)
    }

    fn flush(&mut self) -> Result<(), IoError> {
        io::Write::flush(&mut self.writer).map_err(IoError::Io)?;
        self.writer.get_ref().sync_data().map_err(IoError::Io)
    }
}

/// Buffered reader for a byte range from a [`DiskRetention`] file.
pub struct DiskReader {
    file: BufReader<File>,
    remaining: u64,
}

impl AqRead for DiskReader {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        if self.remaining == 0 {
            return Ok(0);
        }
        let max = buf.len().min(self.remaining as usize);
        let n = io::Read::read(&mut self.file, &mut buf[..max]).map_err(IoError::Io)?;
        self.remaining -= n as u64;
        Ok(n)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), IoError> {
        if buf.len() as u64 > self.remaining {
            return Err(IoError::UnexpectedEof);
        }
        io::Read::read_exact(&mut self.file, buf).map_err(IoError::Io)?;
        self.remaining -= buf.len() as u64;
        Ok(())
    }
}

impl Retention for DiskRetention {
    type Reader<'a> = DiskReader;

    fn reader(&self, offset: u64, len: u64) -> Result<Self::Reader<'_>, IoError> {
        let mut file = self.reader_handle.try_clone().map_err(IoError::Io)?;
        file.seek(SeekFrom::Start(offset)).map_err(IoError::Io)?;
        Ok(DiskReader {
            file: BufReader::with_capacity(READ_BUF_SIZE, file),
            remaining: len,
        })
    }

    fn discard(&mut self) -> Result<(), IoError> {
        std::fs::remove_file(&self.path).map_err(IoError::Io)
    }
}
