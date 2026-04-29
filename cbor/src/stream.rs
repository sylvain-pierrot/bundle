//! Streaming CBOR codec over [`CborRead`]/[`CborWrite`].

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::consts::*;
use crate::{CborRead, CborWrite, Error};

// ---------------------------------------------------------------------------
// StreamDecoder
// ---------------------------------------------------------------------------

/// CBOR decoder over any [`CborRead`] source.
///
/// Returns owned data (unlike the buffer-based [`crate::Decoder`] which
/// borrows from `&[u8]`). Use [`read_bstr_header`](Self::read_bstr_header) +
/// [`inner`](Self::inner) to stream large byte strings without buffering.
pub struct StreamDecoder<R> {
    reader: R,
    peeked: Option<u8>,
    pos: u64,
}

impl<R: CborRead> StreamDecoder<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            peeked: None,
            pos: 0,
        }
    }

    #[inline]
    pub fn position(&self) -> u64 {
        self.pos
    }

    /// Get a mutable reference to the inner reader.
    pub fn inner(&mut self) -> &mut R {
        &mut self.reader
    }

    /// Consume this decoder, returning the inner reader.
    pub fn into_inner(self) -> R {
        self.reader
    }

    fn next_byte(&mut self) -> Result<u8, Error> {
        if let Some(b) = self.peeked.take() {
            self.pos += 1;
            return Ok(b);
        }
        let mut buf = [0u8; 1];
        self.reader.cbor_read_exact(&mut buf)?;
        self.pos += 1;
        Ok(buf[0])
    }

    fn peek_byte(&mut self) -> Result<u8, Error> {
        if let Some(b) = self.peeked {
            return Ok(b);
        }
        let mut buf = [0u8; 1];
        self.reader.cbor_read_exact(&mut buf)?;
        self.peeked = Some(buf[0]);
        Ok(buf[0])
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), Error> {
        self.reader.cbor_read_exact(buf)?;
        self.pos += buf.len() as u64;
        Ok(())
    }

    fn read_argument(&mut self, additional: u8) -> Result<u64, Error> {
        match additional {
            0..=23 => Ok(additional as u64),
            24 => Ok(self.next_byte()? as u64),
            25 => {
                let mut buf = [0u8; 2];
                self.read_exact(&mut buf)?;
                Ok(u16::from_be_bytes(buf) as u64)
            }
            26 => {
                let mut buf = [0u8; 4];
                self.read_exact(&mut buf)?;
                Ok(u32::from_be_bytes(buf) as u64)
            }
            27 => {
                let mut buf = [0u8; 8];
                self.read_exact(&mut buf)?;
                Ok(u64::from_be_bytes(buf))
            }
            _ => Err(Error::InvalidCbor),
        }
    }

    /// Peek at the major type of the next item.
    pub fn peek_major(&mut self) -> Result<u8, Error> {
        Ok(self.peek_byte()? >> 5)
    }

    /// Check if the next byte is a CBOR break code.
    pub fn is_break(&mut self) -> Result<bool, Error> {
        Ok(self.peek_byte()? == BREAK)
    }

    /// Read and consume a break code.
    pub fn read_break(&mut self) -> Result<(), Error> {
        let b = self.next_byte()?;
        if b != BREAK {
            return Err(Error::UnexpectedCborType {
                expected: "break",
                actual: b,
            });
        }
        Ok(())
    }

    /// Read an unsigned integer.
    pub fn read_uint(&mut self) -> Result<u64, Error> {
        let b = self.next_byte()?;
        let major = b >> 5;
        let additional = b & 0x1F;
        if major != MAJOR_UINT {
            return Err(Error::UnexpectedCborType {
                expected: "uint",
                actual: b,
            });
        }
        self.read_argument(additional)
    }

    /// Read a byte string into an owned `Vec<u8>`.
    pub fn read_bstr(&mut self) -> Result<Vec<u8>, Error> {
        let len = self.read_bstr_header()?;
        let mut buf = vec![0u8; len as usize];
        self.read_exact(&mut buf)?;
        Ok(buf)
    }

    /// Read only the byte string header, returning the data length.
    pub fn read_bstr_header(&mut self) -> Result<u64, Error> {
        let b = self.next_byte()?;
        let major = b >> 5;
        let additional = b & 0x1F;
        if major != MAJOR_BSTR {
            return Err(Error::UnexpectedCborType {
                expected: "bstr",
                actual: b,
            });
        }
        self.read_argument(additional)
    }

    /// Read a text string into an owned `String`.
    pub fn read_tstr(&mut self) -> Result<String, Error> {
        let b = self.next_byte()?;
        let major = b >> 5;
        let additional = b & 0x1F;
        if major != MAJOR_TSTR {
            return Err(Error::UnexpectedCborType {
                expected: "tstr",
                actual: b,
            });
        }
        let len = self.read_argument(additional)? as usize;
        let mut buf = vec![0u8; len];
        self.read_exact(&mut buf)?;
        String::from_utf8(buf).map_err(|_| Error::InvalidUtf8)
    }

    /// Read a definite-length array header, returning the number of items.
    pub fn read_array_len(&mut self) -> Result<usize, Error> {
        let b = self.next_byte()?;
        let major = b >> 5;
        let additional = b & 0x1F;
        if major != MAJOR_ARRAY {
            return Err(Error::UnexpectedCborType {
                expected: "array",
                actual: b,
            });
        }
        if additional == 31 {
            return Err(Error::UnexpectedCborType {
                expected: "definite array",
                actual: b,
            });
        }
        Ok(self.read_argument(additional)? as usize)
    }

    /// Read an indefinite-length array start marker (0x9F).
    pub fn read_indefinite_array_start(&mut self) -> Result<(), Error> {
        let b = self.next_byte()?;
        if b != 0x9F {
            return Err(Error::UnexpectedCborType {
                expected: "indefinite array",
                actual: b,
            });
        }
        Ok(())
    }

    /// Read either a uint or a tstr (owned).
    pub fn read_uint_or_tstr(&mut self) -> Result<UintOrString, Error> {
        match self.peek_major()? {
            MAJOR_UINT => Ok(UintOrString::Uint(self.read_uint()?)),
            MAJOR_TSTR => Ok(UintOrString::Tstr(self.read_tstr()?)),
            _ => Err(Error::UnexpectedCborType {
                expected: "uint or tstr",
                actual: self.peek_byte()?,
            }),
        }
    }

    /// Skip `len` bytes in the stream, updating the position.
    pub fn skip(&mut self, len: u64) -> Result<(), Error> {
        let mut remaining = len;
        let mut buf = [0u8; 256];
        while remaining > 0 {
            let to_read = (remaining as usize).min(buf.len());
            self.reader.cbor_read_exact(&mut buf[..to_read])?;
            remaining -= to_read as u64;
        }
        self.pos += len;
        Ok(())
    }

    /// Inform the decoder that `n` bytes were read directly from
    /// [`inner`](Self::inner), so the position stays accurate.
    pub fn advance(&mut self, n: u64) {
        self.pos += n;
    }
}

/// Owned result of reading a uint-or-tstr ambiguity.
pub enum UintOrString {
    Uint(u64),
    Tstr(String),
}

// ---------------------------------------------------------------------------
// StreamEncoder
// ---------------------------------------------------------------------------

/// CBOR encoder over any [`CborWrite`] sink.
pub struct StreamEncoder<W> {
    writer: W,
    pos: u64,
}

impl<W: CborWrite> StreamEncoder<W> {
    pub fn new(writer: W) -> Self {
        Self { writer, pos: 0 }
    }

    #[inline]
    pub fn position(&self) -> u64 {
        self.pos
    }

    /// Get a mutable reference to the inner writer.
    pub fn inner(&mut self) -> &mut W {
        &mut self.writer
    }

    /// Consume this encoder, returning the inner writer.
    pub fn into_inner(self) -> W {
        self.writer
    }

    /// Flush the inner writer.
    pub fn flush(&mut self) -> Result<(), Error> {
        self.writer.cbor_flush()
    }

    fn write_all(&mut self, data: &[u8]) -> Result<(), Error> {
        self.writer.cbor_write_all(data)?;
        self.pos += data.len() as u64;
        Ok(())
    }

    fn write_head(&mut self, major: u8, value: u64) -> Result<(), Error> {
        let m = major << 5;
        match value {
            0..=23 => self.write_all(&[m | value as u8]),
            24..=0xFF => self.write_all(&[m | 24, value as u8]),
            0x100..=0xFFFF => {
                self.write_all(&[m | 25])?;
                self.write_all(&(value as u16).to_be_bytes())
            }
            0x1_0000..=0xFFFF_FFFF => {
                self.write_all(&[m | 26])?;
                self.write_all(&(value as u32).to_be_bytes())
            }
            _ => {
                self.write_all(&[m | 27])?;
                self.write_all(&value.to_be_bytes())
            }
        }
    }

    pub fn write_uint(&mut self, v: u64) -> Result<(), Error> {
        self.write_head(MAJOR_UINT, v)
    }

    pub fn write_bstr(&mut self, data: &[u8]) -> Result<(), Error> {
        self.write_head(MAJOR_BSTR, data.len() as u64)?;
        self.write_all(data)
    }

    /// Write only the byte string header (length prefix).
    pub fn write_bstr_header(&mut self, len: u64) -> Result<(), Error> {
        self.write_head(MAJOR_BSTR, len)
    }

    pub fn write_tstr(&mut self, s: &str) -> Result<(), Error> {
        self.write_head(MAJOR_TSTR, s.len() as u64)?;
        self.write_all(s.as_bytes())
    }

    pub fn write_array(&mut self, len: usize) -> Result<(), Error> {
        self.write_head(MAJOR_ARRAY, len as u64)
    }

    pub fn write_indefinite_array(&mut self) -> Result<(), Error> {
        self.write_all(&[0x9F])
    }

    pub fn write_break(&mut self) -> Result<(), Error> {
        self.write_all(&[BREAK])
    }

    /// Write raw bytes directly to the sink.
    pub fn write_raw(&mut self, data: &[u8]) -> Result<(), Error> {
        self.write_all(data)
    }

    /// Inform the encoder that `n` bytes were written directly to
    /// [`inner`](Self::inner), so the position stays accurate.
    pub fn advance(&mut self, n: u64) {
        self.pos += n;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Decoder, Encoder};

    #[test]
    fn stream_roundtrip_uint() {
        for v in [
            0u64,
            1,
            23,
            24,
            255,
            256,
            65535,
            65536,
            0xFFFF_FFFF,
            u64::MAX,
        ] {
            let mut buf = Vec::new();
            let mut enc = StreamEncoder::new(&mut buf);
            enc.write_uint(v).unwrap();
            drop(enc);

            let mut dec = StreamDecoder::new(buf.as_slice());
            assert_eq!(dec.read_uint().unwrap(), v, "failed for {v}");
        }
    }

    #[test]
    fn stream_roundtrip_bstr() {
        let data = b"hello world";
        let mut buf = Vec::new();
        let mut enc = StreamEncoder::new(&mut buf);
        enc.write_bstr(data).unwrap();
        drop(enc);

        let mut dec = StreamDecoder::new(buf.as_slice());
        assert_eq!(dec.read_bstr().unwrap(), data);
    }

    #[test]
    fn stream_roundtrip_tstr() {
        let s = "dtn://node1/svc";
        let mut buf = Vec::new();
        let mut enc = StreamEncoder::new(&mut buf);
        enc.write_tstr(s).unwrap();
        drop(enc);

        let mut dec = StreamDecoder::new(buf.as_slice());
        assert_eq!(dec.read_tstr().unwrap(), s);
    }

    #[test]
    fn stream_indefinite_array() {
        let mut buf = Vec::new();
        let mut enc = StreamEncoder::new(&mut buf);
        enc.write_indefinite_array().unwrap();
        enc.write_uint(42).unwrap();
        enc.write_break().unwrap();
        drop(enc);

        let mut dec = StreamDecoder::new(buf.as_slice());
        dec.read_indefinite_array_start().unwrap();
        assert!(!dec.is_break().unwrap());
        assert_eq!(dec.read_uint().unwrap(), 42);
        assert!(dec.is_break().unwrap());
        dec.read_break().unwrap();
    }

    #[test]
    fn stream_bstr_header_then_manual_read() {
        let payload = b"streaming payload data";
        let mut buf = Vec::new();
        let mut enc = StreamEncoder::new(&mut buf);
        enc.write_bstr(payload).unwrap();
        drop(enc);

        let mut dec = StreamDecoder::new(buf.as_slice());
        let len = dec.read_bstr_header().unwrap();
        assert_eq!(len, payload.len() as u64);

        let mut data = vec![0u8; len as usize];
        dec.inner().cbor_read_exact(&mut data).unwrap();
        dec.advance(len);
        assert_eq!(data, payload);
    }

    #[test]
    fn stream_skip() {
        let mut buf = Vec::new();
        let mut enc = StreamEncoder::new(&mut buf);
        enc.write_bstr(b"skip me").unwrap();
        enc.write_uint(99).unwrap();
        drop(enc);

        let mut dec = StreamDecoder::new(buf.as_slice());
        let len = dec.read_bstr_header().unwrap();
        dec.skip(len).unwrap();
        assert_eq!(dec.read_uint().unwrap(), 99);
    }

    #[test]
    fn cross_format_buffer_encode_stream_decode() {
        let mut enc = Encoder::new();
        enc.write_array(3);
        enc.write_uint(1);
        enc.write_tstr("hello");
        enc.write_bstr(b"\x00\x01\x02");
        let bytes = enc.into_bytes();

        let mut dec = StreamDecoder::new(bytes.as_slice());
        assert_eq!(dec.read_array_len().unwrap(), 3);
        assert_eq!(dec.read_uint().unwrap(), 1);
        assert_eq!(dec.read_tstr().unwrap(), "hello");
        assert_eq!(dec.read_bstr().unwrap(), b"\x00\x01\x02");
    }

    #[test]
    fn cross_format_stream_encode_buffer_decode() {
        let mut buf = Vec::new();
        let mut enc = StreamEncoder::new(&mut buf);
        enc.write_array(2).unwrap();
        enc.write_uint(42).unwrap();
        enc.write_tstr("test").unwrap();
        drop(enc);

        let mut dec = Decoder::new(&buf);
        assert_eq!(dec.read_array_len().unwrap(), 2);
        assert_eq!(dec.read_uint().unwrap(), 42);
        assert_eq!(dec.read_tstr().unwrap(), "test");
    }

    #[test]
    fn stream_position_tracking() {
        let mut buf = Vec::new();
        let mut enc = StreamEncoder::new(&mut buf);
        assert_eq!(enc.position(), 0);
        enc.write_uint(1).unwrap();
        assert_eq!(enc.position(), 1);
        enc.write_bstr(b"hello").unwrap();
        let end_pos = enc.position();
        drop(enc);

        let mut dec = StreamDecoder::new(buf.as_slice());
        assert_eq!(dec.position(), 0);
        dec.read_uint().unwrap();
        assert_eq!(dec.position(), 1);
        dec.read_bstr().unwrap();
        assert_eq!(dec.position(), end_pos);
    }
}
