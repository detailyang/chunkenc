use crate::bitstream::{Bit, BitStream, Reader};
use crate::error::Error;
use crate::helper::{encode_i64, read_i64};
use bytes::{BufMut, BytesMut};
use unsigned_varint::io;

const CHUNK_COMPACT_CAPACITY_THRESHOLD: usize = 32;
const MAX_VARINT_LEN64: usize = 10;

type Result<T> = std::result::Result<T, Error>;

pub enum Encoding {
    XOR,
    None,
}

pub trait Appender {
    fn append(&mut self, t: i64, v: f64);
}

pub trait Iterator {
    fn next(&mut self) -> Result<bool>;

    fn seek(&mut self, t: i64) -> Result<bool>;

    fn at(&self) -> (i64, f64);
}

pub trait Chunk<'a> {
    fn bytes(&self) -> &[u8];

    fn encoding(&self) -> Encoding;

    fn num_samples(&self) -> usize;

    fn compact(&mut self);

    fn appender(&'a mut self) -> Result<Box<dyn Appender + 'a>>;

    fn iterator(&self) -> Box<dyn Iterator>;
}

pub struct NopIterator {}

impl Iterator for NopIterator {
    fn next(&mut self) -> Result<bool> {
        Ok(false)
    }

    fn seek(&mut self, _: i64) -> Result<bool> {
        Ok(false)
    }

    fn at(&self) -> (i64, f64) {
        (i64::MIN, 0_f64)
    }
}

pub struct XORChunk {
    b: BitStream,
}

impl XORChunk {
    pub fn new() -> Self {
        let mut data = BytesMut::with_capacity(128);
        data.put_u8(0_u8);
        data.put_u8(0);

        Self {
            b: BitStream { data, count: 0 },
        }
    }

    pub fn to_vec(self) -> Vec<u8> {
        self.b.to_vec()
    }

    fn _iterator(&self) -> XORIterator {
        XORIterator {
            br: Reader::new(bytes::BytesMut::from(&self.b.as_ref()[2..])),
            num_total: u16::from_be_bytes(self.b.as_ref()[0..2].try_into().unwrap()),
            t: i64::MIN,
            num_read: 0,
            v: 0.0,
            leading: 0,
            trailing: 0,
            t_delta: 0,
        }
    }
}

impl<'a> Chunk<'a> for XORChunk {
    fn bytes(&self) -> &[u8] {
        self.b.as_ref()
    }

    fn encoding(&self) -> Encoding {
        Encoding::XOR
    }

    fn num_samples(&self) -> usize {
        u16::from_be_bytes(self.bytes()[0..2].try_into().unwrap()) as usize
    }

    fn compact(&mut self) {
        let l = self.b.data.len();
        if self.b.data.capacity() > l + CHUNK_COMPACT_CAPACITY_THRESHOLD {
            let mut buf = vec![0_u8; l];
            buf.copy_from_slice(self.b.data.as_ref());
            self.b.data.copy_from_slice(&buf);
        }
    }

    fn appender(&'a mut self) -> Result<Box<dyn Appender + 'a>> {
        let mut it = self._iterator();

        while it.next()? {}

        let leading = if u16::from_be_bytes(self.b.as_ref().try_into().unwrap()) == 0 {
            0xff
        } else {
            it.leading
        };

        let a = XORAppender {
            b: &mut self.b,
            t: it.t,
            v: it.v,
            t_delta: it.t_delta,
            leading,
            trailing: it.trailing,
        };

        Ok(Box::new(a))
    }

    fn iterator(&self) -> Box<dyn Iterator> {
        Box::new(self._iterator())
    }
}

pub struct XORAppender<'a> {
    b: &'a mut BitStream,
    t: i64,
    v: f64,
    t_delta: u64,
    leading: u8,
    trailing: u8,
}

impl<'a> XORAppender<'a> {
    fn write_v_delta(&mut self, v: f64) {
        let v_delta = v.to_bits() ^ self.v.to_bits();

        if v_delta == 0 {
            self.b.write_bit(Bit::Zero);
            return;
        }
        self.b.write_bit(Bit::One);

        let mut leading = v_delta.leading_zeros() as u8;
        let trailing = v_delta.trailing_zeros() as u8;

        if leading >= 32 {
            leading = 31;
        }

        if self.leading != 0xff && leading >= self.leading && trailing >= self.trailing {
            self.b.write_bit(Bit::Zero);
            self.b.write_bits(
                v_delta >> self.trailing,
                (64 - self.leading - self.trailing) as isize,
            );
        } else {
            self.leading = leading;
            self.trailing = trailing;

            self.b.write_bit(Bit::One);
            self.b.write_bits(leading as u64, 5);
            let sigbits = 64 - leading - trailing;

            self.b.write_bits(sigbits as u64, 6);
            self.b.write_bits(v_delta >> trailing, sigbits as isize);
        }
    }
}

impl<'a> Appender for XORAppender<'a> {
    fn append(&mut self, t: i64, v: f64) {
        let mut t_delta = 0_u64;
        let n = u16::from_be_bytes(self.b.as_ref()[0..2].as_ref().try_into().unwrap());
        // TODO(detailyang): check the u16 overflow

        if n == 0 {
            let mut buf = [0_u8; MAX_VARINT_LEN64];
            let l = encode_i64(t, &mut buf);
            for i in l {
                self.b.write_byte(*i);
            }
            self.b.write_bits(v.to_bits(), 64);
        } else if n == 1 {
            t_delta = (t - self.t) as u64;

            let mut buf = [0_u8; MAX_VARINT_LEN64];
            let l = unsigned_varint::encode::u64(t_delta, &mut buf);

            for i in l {
                self.b.write_byte(*i);
            }

            self.write_v_delta(v)
        } else {
            t_delta = (t - self.t) as u64;
            let dod = t_delta as i64 - self.t_delta as i64;

            match 1 {
                _ if dod == 0 => {
                    self.b.write_bit(Bit::Zero);
                }
                _ if bit_range(dod, 14) => {
                    self.b.write_bits(0x02, 2);
                    self.b.write_bits(dod as u64, 14);
                }
                _ if bit_range(dod, 17) => {
                    self.b.write_bits(0x06, 3);
                    self.b.write_bits(dod as u64, 17);
                }
                _ if bit_range(dod, 20) => {
                    self.b.write_bits(0x06, 4);
                    self.b.write_bits(dod as u64, 20);
                }
                _ => {
                    self.b.write_bits(0x0f, 4);
                    self.b.write_bits(dod as u64, 64);
                }
            }

            self.write_v_delta(v);
        }

        self.t = t;
        self.v = v;
        self.t_delta = t_delta as u64;
        self.b.data[0..2].copy_from_slice((n + 1).to_be_bytes().as_slice());
    }
}

fn bit_range(x: i64, nbits: u8) -> bool {
    return -((1 << (nbits - 1)) - 1) <= x && x <= 1 << (nbits - 1);
}

#[derive(Debug)]
pub struct XORIterator {
    br: Reader,
    num_total: u16,
    num_read: u16,
    t: i64,
    v: f64,
    leading: u8,
    trailing: u8,
    t_delta: u64,
}

impl XORIterator {
    pub fn read_value(&mut self) -> Result<bool> {
        let b = self.br.try_read_bit().ok_or(crate::error::Error::EOF)?;
        if b == Bit::Zero {
        } else {
            let b = self.br.try_read_bit().ok_or(crate::error::Error::EOF)?;
            if b == Bit::Zero {
            } else {
                let bits = self.br.try_read_bits(5).ok_or(crate::error::Error::EOF)?;
                self.leading = bits as u8;

                let bits = self.br.try_read_bits(6).ok_or(crate::error::Error::EOF)?;
                let mbits = {
                    if bits == 0 {
                        64_u8
                    } else {
                        bits as u8
                    }
                };

                self.trailing = 64 - self.leading - mbits;
            }

            let mbits = 64 - self.leading - self.trailing;
            let bits = self
                .br
                .try_read_bits(mbits)
                .ok_or(crate::error::Error::EOF)?;

            let mut vbits = self.v.to_bits();
            vbits ^= bits << self.trailing;
            self.v = f64::from_bits(vbits);
        }

        self.num_read += 1;
        Ok(true)
    }
}

impl Iterator for XORIterator {
    fn next(&mut self) -> Result<bool> {
        if self.num_read == self.num_total {
            return Ok(false);
        }

        if self.num_read == 0 {
            let t = read_i64(&mut self.br)?;
            let v = self.br.read_bits(64).ok_or(crate::error::Error::EOF)?;
            self.t = t;
            self.v = f64::from_bits(v);
            self.num_read += 1;
            return Ok(true);
        }

        if self.num_read == 1 {
            let t_delta = io::read_u64(&mut self.br).map_err(crate::error::Error::ReadError)?;
            self.t_delta = t_delta;
            self.t += self.t_delta as i64;

            return self.read_value();
        }

        // delta-of-delta
        let mut d = 0_u8;
        for _ in 0..4 {
            d <<= 1;

            let bit = self.br.try_read_bit().ok_or(crate::error::Error::EOF)?;
            if bit == Bit::Zero {
                break;
            }

            d |= 1;
        }

        let mut sz = 0_u8;
        let mut dod = 0_i64;
        match d {
            0x02 => {
                sz = 14;
            }
            0x06 => {
                sz = 17;
            }
            0x0e => {
                sz = 20;
            }
            0x0f => {
                let bits = self.br.read_bits(64).ok_or(crate::error::Error::EOF)?;
                dod = bits as i64;
            }
            _ => {}
        }

        if sz != 0 {
            let mut bits = self.br.try_read_bits(sz).ok_or(crate::error::Error::EOF)?;
            if bits > (1 << (sz - 1)) {
                bits = (bits as i64 - (1 << sz)) as u64;
            }
            dod = bits as i64;
        }

        self.t_delta = (self.t_delta as i64 + dod) as u64;
        self.t += self.t_delta as i64;

        self.read_value()
    }

    fn seek(&mut self, t: i64) -> Result<bool> {
        while t > self.t || self.num_read == 0 {
            self.next()?;
        }

        Ok(true)
    }

    fn at(&self) -> (i64, f64) {
        (self.t, self.v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::prelude::*;

    #[derive(Debug, PartialEq)]
    struct Pair {
        ts: i64,
        v: f64,
    }

    // #[test]
    // pub fn test_generate_tests_for_go() {
    //     let mut xor = XORChunk::new();
    //     let mut a = xor.appender().unwrap();
    //
    //     for i in 1..16 {
    //         a.append(i, i as f64);
    //     }
    //     std::mem::drop(a);
    //
    //     let b = xor.bytes();
    //     println!("{:#x?}", b);
    // }

    #[test]
    pub fn test_chunk() {
        do_test_chunk(0);
        do_test_chunk(1);
        do_test_chunk(2);
        do_test_chunk(4);
        do_test_chunk(8);
        do_test_chunk(16);
        do_test_chunk(64);
        do_test_chunk(256);
        do_test_chunk(1024);
        do_test_chunk(10240);
        do_test_chunk(65535);
        // do_test_chunk(65536); // it will overflow
    }

    #[allow(unused_must_use)]
    pub fn do_test_chunk(n: usize) {
        let mut xor = XORChunk::new();
        let mut a = xor.appender().unwrap();

        let mut exp = Vec::new();
        let mut ts = 1234123324_i64;
        let mut v = 1243535.123_f64;

        for i in 0..n {
            let num = rand::thread_rng().gen_range(0..10000) + 1;
            ts += num;
            if i % 2 == 0 {
                v += rand::thread_rng().gen_range(0..1000000) as f64
            } else {
                v -= rand::thread_rng().gen_range(0..1000000) as f64
            }

            a.append(ts, v);
            exp.push(Pair { ts, v });
        }

        std::mem::drop(a);

        let mut exp1 = Vec::new();
        let mut it = xor.iterator();
        while it.next().unwrap_or_else(|_| false) {
            let (ts, v) = it.at();
            exp1.push(Pair { ts, v });
        }

        assert_eq!(exp, exp1);
    }
}
