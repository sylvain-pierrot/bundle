//! Buffer-based CBOR decoder over `&[u8]`.

use crate::Error;
use crate::consts::*;

/// CBOR decoder over a byte slice (zero-copy reads).
pub struct Decoder<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Decoder<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    #[inline]
    pub fn position(&self) -> usize {
        self.pos
    }

    fn next_byte(&mut self) -> Result<u8, Error> {
        if self.pos >= self.data.len() {
            return Err(Error::UnexpectedEof);
        }
        let b = self.data[self.pos];
        self.pos += 1;
        Ok(b)
    }

    fn peek_byte(&self) -> Result<u8, Error> {
        if self.pos >= self.data.len() {
            return Err(Error::UnexpectedEof);
        }
        Ok(self.data[self.pos])
    }

    fn read_bytes(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if self.pos + n > self.data.len() {
            return Err(Error::UnexpectedEof);
        }
        let slice = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    fn read_argument(&mut self, additional: u8) -> Result<u64, Error> {
        match additional {
            0..=23 => Ok(additional as u64),
            24 => Ok(self.next_byte()? as u64),
            25 => {
                let b = self.read_bytes(2)?;
                Ok(u16::from_be_bytes([b[0], b[1]]) as u64)
            }
            26 => {
                let b = self.read_bytes(4)?;
                Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as u64)
            }
            27 => {
                let b = self.read_bytes(8)?;
                Ok(u64::from_be_bytes([
                    b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
                ]))
            }
            _ => Err(Error::InvalidCbor),
        }
    }

    pub fn peek_major(&self) -> Result<u8, Error> {
        Ok(self.peek_byte()? >> 5)
    }

    pub fn is_break(&self) -> Result<bool, Error> {
        Ok(self.peek_byte()? == BREAK)
    }

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

    pub fn read_bstr(&mut self) -> Result<&'a [u8], Error> {
        let b = self.next_byte()?;
        let major = b >> 5;
        let additional = b & 0x1F;
        if major != MAJOR_BSTR {
            return Err(Error::UnexpectedCborType {
                expected: "bstr",
                actual: b,
            });
        }
        let len = self.read_argument(additional)? as usize;
        self.read_bytes(len)
    }

    pub fn read_bstr_with_offset(&mut self) -> Result<(&'a [u8], usize), Error> {
        let b = self.next_byte()?;
        let major = b >> 5;
        let additional = b & 0x1F;
        if major != MAJOR_BSTR {
            return Err(Error::UnexpectedCborType {
                expected: "bstr",
                actual: b,
            });
        }
        let len = self.read_argument(additional)? as usize;
        let offset = self.pos;
        let data = self.read_bytes(len)?;
        Ok((data, offset))
    }

    pub fn read_tstr(&mut self) -> Result<&'a str, Error> {
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
        let bytes = self.read_bytes(len)?;
        core::str::from_utf8(bytes).map_err(|_| Error::InvalidUtf8)
    }

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

    pub fn read_uint_or_tstr(&mut self) -> Result<UintOrTstr<'a>, Error> {
        match self.peek_major()? {
            MAJOR_UINT => Ok(UintOrTstr::Uint(self.read_uint()?)),
            MAJOR_TSTR => Ok(UintOrTstr::Tstr(self.read_tstr()?)),
            _ => Err(Error::UnexpectedCborType {
                expected: "uint or tstr",
                actual: self.peek_byte()?,
            }),
        }
    }
}

/// Result of reading a uint-or-tstr ambiguity (used for EID SSP).
pub enum UintOrTstr<'a> {
    Uint(u64),
    Tstr(&'a str),
}
