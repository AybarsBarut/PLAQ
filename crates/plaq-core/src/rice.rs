//! Bounded MSB-first Rice/Golomb bit coding.

use crate::CoreError;

pub const MAX_RICE_K: u8 = 31;
pub const MAX_UNARY_QUOTIENT: u64 = 1_000_000;

pub fn zigzag_encode(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

pub fn zigzag_decode(value: u64) -> i64 {
    ((value >> 1) as i64) ^ -((value & 1) as i64)
}

#[derive(Debug, Default)]
pub struct RiceEncoder {
    bytes: Vec<u8>,
    current: u8,
    used: u8,
}

impl RiceEncoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn write_values(&mut self, values: &[i64], k: u8) -> Result<(), CoreError> {
        if k > MAX_RICE_K {
            return Err(CoreError::InvalidRiceParameter(k));
        }
        for &value in values {
            let unsigned = zigzag_encode(value);
            let quotient = unsigned >> k;
            if quotient > MAX_UNARY_QUOTIENT {
                return Err(CoreError::RiceQuotientTooLarge);
            }
            for _ in 0..quotient {
                self.write_bit(false);
            }
            self.write_bit(true);
            if k > 0 {
                let remainder = unsigned & ((1_u64 << k) - 1);
                self.write_bits(remainder, k);
            }
        }
        Ok(())
    }

    fn write_bits(&mut self, value: u64, count: u8) {
        for shift in (0..count).rev() {
            self.write_bit(((value >> shift) & 1) != 0);
        }
    }

    fn write_bit(&mut self, bit: bool) {
        self.current <<= 1;
        if bit {
            self.current |= 1;
        }
        self.used += 1;
        if self.used == 8 {
            self.bytes.push(self.current);
            self.current = 0;
            self.used = 0;
        }
    }

    pub fn finish(mut self) -> Vec<u8> {
        if self.used > 0 {
            self.current <<= 8 - self.used;
            self.bytes.push(self.current);
        }
        self.bytes
    }
}

#[derive(Debug)]
pub struct RiceDecoder<'a> {
    bytes: &'a [u8],
    bit_position: usize,
}

impl<'a> RiceDecoder<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            bit_position: 0,
        }
    }

    pub fn read_values(&mut self, count: usize, k: u8) -> Result<Vec<i64>, CoreError> {
        if k > MAX_RICE_K {
            return Err(CoreError::InvalidRiceParameter(k));
        }
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            let mut quotient = 0_u64;
            while !self.read_bit()? {
                quotient += 1;
                if quotient > MAX_UNARY_QUOTIENT {
                    return Err(CoreError::RiceQuotientTooLarge);
                }
            }
            let remainder = self.read_bits(k)?;
            let unsigned = quotient
                .checked_shl(u32::from(k))
                .and_then(|base| base.checked_add(remainder))
                .ok_or(CoreError::SizeOverflow)?;
            values.push(zigzag_decode(unsigned));
        }
        Ok(values)
    }

    fn read_bits(&mut self, count: u8) -> Result<u64, CoreError> {
        let mut value = 0_u64;
        for _ in 0..count {
            value = (value << 1) | u64::from(self.read_bit()?);
        }
        Ok(value)
    }

    fn read_bit(&mut self) -> Result<bool, CoreError> {
        if self.bit_position >= self.bytes.len().saturating_mul(8) {
            return Err(CoreError::UnexpectedEndOfPayload);
        }
        let byte = self.bytes[self.bit_position / 8];
        let shift = 7 - (self.bit_position % 8);
        self.bit_position += 1;
        Ok(((byte >> shift) & 1) != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_mapping_round_trips() {
        for value in [-1_000_000, -2, -1, 0, 1, 2, 1_000_000] {
            assert_eq!(zigzag_decode(zigzag_encode(value)), value);
        }
    }

    #[test]
    fn rice_round_trips_multiple_sequences() {
        let first = [-8, -1, 0, 1, 8, 127];
        let second = [0, 0, 42, -42];
        let mut encoder = RiceEncoder::new();
        encoder.write_values(&first, 2).unwrap();
        encoder.write_values(&second, 4).unwrap();
        let payload = encoder.finish();
        let mut decoder = RiceDecoder::new(&payload);
        assert_eq!(decoder.read_values(first.len(), 2).unwrap(), first);
        assert_eq!(decoder.read_values(second.len(), 4).unwrap(), second);
    }
}
