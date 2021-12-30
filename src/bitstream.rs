use bytes::{BufMut, BytesMut};
use std::borrow::Borrow;

#[derive(Debug, PartialEq)]
pub enum Bit {
    Zero,
    One,
}

impl From<bool> for Bit {
    fn from(b: bool) -> Self {
        if b {
            Self::One
        } else {
            Self::Zero
        }
    }
}

impl From<Bit> for bool {
    fn from(b: Bit) -> Self {
        match b {
            Bit::One => true,
            Bit::Zero => false,
        }
    }
}

#[derive(Clone)]
pub struct BitStream {
    pub(crate) data: bytes::BytesMut,
    pub(crate) count: u8, // how many bits are valid in current byte
}

impl AsRef<[u8]> for BitStream {
    fn as_ref(&self) -> &[u8] {
        self.data.borrow()
    }
}

impl BitStream {
    pub fn new() -> Self {
        Self {
            data: BytesMut::new(),
            count: 0,
        }
    }

    pub fn to_vec(self) -> Vec<u8> {
        self.data.to_vec()
    }

    pub fn write_bit(&mut self, bit: impl Into<Bit>) {
        let bit = bit.into();

        if self.count == 0 {
            self.data.put_u8(0);
            self.count = 8;
        }

        let i = self.data.len() - 1;
        if bool::from(bit) {
            self.data[i] |= (1 << (self.count - 1)) as u8;
        }

        self.count -= 1;
    }

    pub fn write_byte(&mut self, b: u8) {
        if self.count == 0 {
            self.data.put_u8(0);
            self.count = 8
        }

        let i = self.data.len() - 1;
        self.data[i] |= b >> (8 - self.count);

        self.data.put_u8(0);
        self.data[i + 1] = ((b as u64) << self.count) as u8;
    }

    pub fn write_bits(&mut self, u: u64, mut nbits: isize) {
        let mut u = u << (64 - nbits);
        while nbits >= 8 {
            self.write_byte((u >> 56) as u8);
            u <<= 8;
            nbits -= 8;
        }

        while nbits > 0 {
            self.write_bit(Bit::from((u >> 63) == 1));
            u <<= 1;
            nbits -= 1;
        }
    }
}

#[derive(Debug, Default)]
pub struct Reader {
    stream: bytes::BytesMut,
    stream_offset: usize,
    buffer: u64,
    valid: u8,
}

impl Reader {
    pub fn new(stream: bytes::BytesMut) -> Self {
        Self {
            stream,
            ..Default::default()
        }
    }

    pub fn read_byte(&mut self) -> Option<u8> {
        self.read_bits(8).map(|u| u as u8)
    }

    pub fn try_read_bit(&mut self) -> Option<Bit> {
        let mut bit = self.read_bit_fast();
        if bit.is_none() {
            bit = self.read_bit();
        }
        bit
    }

    pub fn try_read_bits(&mut self, nbits: u8) -> Option<u64> {
        let mut bits = self.read_bits_fast(nbits);
        if bits.is_none() {
            bits = self.read_bits(nbits);
        }
        bits
    }

    pub fn read_bits(&mut self, nbits: u8) -> Option<u64> {
        if self.valid == 0 && !self.load_next_buffer(nbits) {
            return None;
        }

        if nbits <= self.valid {
            return self.read_bits_fast(nbits);
        }

        let bitmask: u64 = (1 << self.valid) - 1;
        let nbits = nbits - self.valid;
        let v = (self.buffer & bitmask) << nbits;
        self.valid = 0;

        if !self.load_next_buffer(nbits) {
            return None;
        }

        let bitmask = (1 << nbits) - 1;
        let v = v | ((self.buffer >> (self.valid - nbits)) & bitmask);
        self.valid -= nbits;

        Some(v)
    }

    pub fn read_bits_fast(&mut self, nbits: u8) -> Option<u64> {
        if nbits > self.valid {
            return None;
        }

        let bitmask: u64 = (1 << nbits) - 1;
        self.valid -= nbits;

        Some((self.buffer >> self.valid) & bitmask)
    }

    pub fn read_bit(&mut self) -> Option<Bit> {
        if self.valid == 0 && !self.load_next_buffer(1) {
            return None;
        }

        self.read_bit_fast()
    }

    pub fn read_bit_fast(&mut self) -> Option<Bit> {
        if self.valid == 0 {
            return None;
        }

        self.valid -= 1;
        let bitmask = 1_u64 << self.valid;

        Some((self.buffer & bitmask != 0).into())
    }

    pub fn load_next_buffer(&mut self, nbits: u8) -> bool {
        if self.stream_offset >= self.stream.as_ref().len() {
            return false;
        }

        if self.stream_offset + 8 < self.stream.as_ref().len() {
            self.buffer = u64::from_be_bytes(
                self.stream[self.stream_offset..self.stream_offset + 8]
                    .try_into()
                    .unwrap(),
            );
            self.stream_offset += 8;
            self.valid = 64;
            return true;
        }

        let mut nbytes = (nbits / 8 + 1) as usize;
        if self.stream_offset + nbytes > self.stream.as_ref().len() {
            nbytes = self.stream.as_ref().len() - self.stream_offset;
        }

        let mut buffer = 0_u64;
        for i in 0..nbytes {
            buffer |=
                ((self.stream.as_ref()[self.stream_offset + i]) as u64) << (8 * (nbytes - i - 1));
        }

        self.buffer = buffer;
        self.stream_offset += nbytes;
        self.valid = (nbytes * 8) as u8;

        true
    }
}

impl std::io::Read for Reader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.read_byte() {
            Some(val) => {
                buf[0] = val;
                Ok(1_usize)
            }
            None => Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn test_reader() {
        let mut bs = BitStream::new();
        for i in [true, false] {
            bs.write_bit(i);
        }

        for nbits in 1..64 {
            bs.write_bits(nbits, nbits as isize);
        }

        for v in (1..10000).step_by(123) {
            bs.write_bits(v, 29);
        }

        let mut r = Reader::new(bytes::BytesMut::from(bs.as_ref()));

        for i in [true, false] {
            let value = r.read_bit().unwrap();
            assert_eq!(Bit::from(i), value, "testing read_bit_fast");
        }

        for nbits in 1..64_u8 {
            let value = r.read_bits(nbits).unwrap();
            assert_eq!(nbits as u64, value, "testing read_bit_fast");
        }

        for i in (1..10000).step_by(123) {
            let value = r.read_bits(29).unwrap();
            assert_eq!(i as u64, value, "testing read_bit_fast");
        }
    }
}
