/// CRC field (RFC 9171 §4.2.1).
///
/// Encodes both the CRC variant and the value in a single enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Crc {
    None,
    Crc16(u16),
    Crc32c(u32),
}

impl Crc {
    pub fn is_none(self) -> bool {
        matches!(self, Crc::None)
    }
}
