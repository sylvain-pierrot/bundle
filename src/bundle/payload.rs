use super::canonical::BlockFlags;
use super::crc::Crc;

/// Coordinates of the payload data within the original input.
///
/// The payload bytes are never held by the bundle — they stay wherever the
/// caller stored them (S3, file, memory). This struct records where to find them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadRef {
    pub flags: BlockFlags,
    pub crc: Crc,
    /// Byte offset in the original input where payload data starts.
    pub data_offset: u64,
    /// Length of the payload data in bytes.
    pub data_len: u64,
}

impl PayloadRef {
    /// Extract the payload data from the source buffer.
    pub fn data<'a>(&self, source: &'a [u8]) -> Option<&'a [u8]> {
        let start = self.data_offset as usize;
        source.get(start..start.checked_add(self.data_len as usize)?)
    }
}
