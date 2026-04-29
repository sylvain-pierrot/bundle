//! Buffer-based CBOR encoder to `Vec<u8>`.

use alloc::vec::Vec;

use crate::consts::*;

/// CBOR encoder that writes to a `Vec<u8>`.
pub struct Encoder {
    buf: Vec<u8>,
}

impl Default for Encoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Encoder {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: Vec::with_capacity(capacity),
        }
    }

    #[inline]
    pub fn position(&self) -> usize {
        self.buf.len()
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    pub fn patch(&mut self, pos: usize, data: &[u8]) {
        self.buf[pos..pos + data.len()].copy_from_slice(data);
    }

    fn write_head(&mut self, major: u8, value: u64) {
        let m = major << 5;
        match value {
            0..=23 => self.buf.push(m | value as u8),
            24..=0xFF => {
                self.buf.push(m | 24);
                self.buf.push(value as u8);
            }
            0x100..=0xFFFF => {
                self.buf.push(m | 25);
                self.buf.extend_from_slice(&(value as u16).to_be_bytes());
            }
            0x1_0000..=0xFFFF_FFFF => {
                self.buf.push(m | 26);
                self.buf.extend_from_slice(&(value as u32).to_be_bytes());
            }
            _ => {
                self.buf.push(m | 27);
                self.buf.extend_from_slice(&value.to_be_bytes());
            }
        }
    }

    pub fn write_uint(&mut self, v: u64) {
        self.write_head(MAJOR_UINT, v);
    }

    pub fn write_bstr(&mut self, data: &[u8]) {
        self.write_head(MAJOR_BSTR, data.len() as u64);
        self.buf.extend_from_slice(data);
    }

    pub fn write_tstr(&mut self, s: &str) {
        self.write_head(MAJOR_TSTR, s.len() as u64);
        self.buf.extend_from_slice(s.as_bytes());
    }

    pub fn write_array(&mut self, len: usize) {
        self.write_head(MAJOR_ARRAY, len as u64);
    }

    pub fn write_indefinite_array(&mut self) {
        self.buf.push(0x9F);
    }

    pub fn write_break(&mut self) {
        self.buf.push(BREAK);
    }

    pub fn write_bstr_header(&mut self, len: u64) {
        self.write_head(MAJOR_BSTR, len);
    }
}
