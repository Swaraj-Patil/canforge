//! Where a signal's bits live in a frame.
//!
//! Bits are numbered the DBC way: absolute bit `b` is bit `b % 8` of byte
//! `b / 8`, where bit 0 is the least significant.
//!
//! Intel (little endian, `@1`): the start bit is the signal's LSB, and raw
//! bit `i` sits at absolute bit `start + i`.
//!
//! Motorola (big endian, `@0`): the start bit is the signal's MSB. Walking
//! from MSB to LSB, the position moves down within a byte and, after bit 0,
//! continues at bit 7 of the next byte.

use crate::model::{Signal, ValueType};

/// `(byte, bit)` for each raw bit of the signal, indexed by raw bit (0 = LSB).
///
/// Callers must check `Signal::valid_length` first.
pub fn bit_positions(start: u64, length: u64, little_endian: bool) -> Vec<(u64, u32)> {
    let len = length as usize;
    let mut pos: Vec<(u64, u32)> = vec![(0, 0); len];
    if little_endian {
        for (i, slot) in pos.iter_mut().enumerate() {
            let a = start + i as u64;
            *slot = (a / 8, (a % 8) as u32);
        }
    } else {
        let mut b = start;
        for k in 0..len {
            pos[len - 1 - k] = (b / 8, (b % 8) as u32);
            if b % 8 == 0 {
                b += 15;
            } else {
                b -= 1;
            }
        }
    }
    pos
}

pub fn signal_bits(s: &Signal) -> Vec<(u64, u32)> {
    bit_positions(s.start, s.length, s.little_endian)
}

/// One contiguous run of a signal's bits inside a single byte.
///
/// Byte bit `p` holds raw bit `raw_bit_lo + (p - byte_bit_lo)`, for
/// `p` in `byte_bit_lo .. byte_bit_lo + count`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub byte: u64,
    pub byte_bit_lo: u32,
    pub raw_bit_lo: u32,
    pub count: u32,
}

/// The signal's bits grouped into one run per byte, sorted by byte. This is
/// what lets generated code move each byte with a single shift and mask.
pub fn segments(s: &Signal) -> Vec<Segment> {
    let pos = signal_bits(s);
    let mut bytes: Vec<u64> = Vec::new();
    for &(byte, _) in pos.iter() {
        if !bytes.contains(&byte) {
            bytes.push(byte);
        }
    }
    bytes.sort_unstable();
    let mut out: Vec<Segment> = Vec::new();
    for &byte in bytes.iter() {
        let mut lo: u32 = 8;
        let mut raw_lo: u32 = 0;
        let mut count: u32 = 0;
        for (raw_bit, &(b, bit)) in pos.iter().enumerate() {
            if b == byte {
                count += 1;
                if bit < lo {
                    lo = bit;
                    raw_lo = raw_bit as u32;
                }
            }
        }
        out.push(Segment {
            byte,
            byte_bit_lo: lo,
            raw_bit_lo: raw_lo,
            count,
        });
    }
    out
}

/// The raw integer range of a field of `length` bits (1..=64).
pub fn raw_range(length: u64, signed: bool) -> (i128, i128) {
    if signed {
        (-(1i128 << (length - 1)), (1i128 << (length - 1)) - 1)
    } else {
        (0, (1i128 << length) - 1)
    }
}

/// True when every bit of the signal lies inside the first `len` bytes.
pub fn fits(s: &Signal, len: usize) -> bool {
    signal_bits(s).iter().all(|&(byte, _)| byte < len as u64)
}

/// The signal's raw bits as an unsigned integer. Requires `fits`.
pub fn extract_raw(data: &[u8], s: &Signal) -> u64 {
    let mut v: u64 = 0;
    for (raw_bit, &(byte, bit)) in signal_bits(s).iter().enumerate() {
        if ((data[byte as usize] >> bit) & 1) == 1 {
            v |= 1u64 << raw_bit;
        }
    }
    v
}

/// Write a raw value (two's complement if negative) into the frame. Requires `fits`.
pub fn insert_raw(data: &mut [u8], s: &Signal, raw: i128) {
    let mask: i128 = (1i128 << s.length) - 1;
    let u = (raw & mask) as u64;
    for (raw_bit, &(byte, bit)) in signal_bits(s).iter().enumerate() {
        let idx = byte as usize;
        if ((u >> raw_bit) & 1) == 1 {
            data[idx] |= 1u8 << bit;
        } else {
            data[idx] &= !(1u8 << bit);
        }
    }
}

/// The raw value, sign-extended for signed integer signals. Requires `fits`.
pub fn raw_value(data: &[u8], s: &Signal) -> i128 {
    let v = extract_raw(data, s) as i128;
    if s.is_signed_integer() && (v & (1i128 << (s.length - 1))) != 0 {
        return v - (1i128 << s.length);
    }
    v
}

/// raw * factor + offset, after reinterpreting float signals' bits.
pub fn physical_value(s: &Signal, raw: i128) -> f64 {
    match s.value_type {
        ValueType::Float32 => {
            let f = f32::from_bits((raw as u64 & 0xFFFF_FFFF) as u32) as f64;
            f * s.factor + s.offset
        }
        ValueType::Float64 => {
            let f = f64::from_bits(raw as u64);
            f * s.factor + s.offset
        }
        ValueType::Integer => (raw as f64) * s.factor + s.offset,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small deterministic generator, so tests need no dependencies.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn between(&mut self, lo: i128, hi: i128) -> i128 {
            let span = (hi - lo) as u128 + 1;
            let r = ((self.next() as u128) << 64 | self.next() as u128) % span;
            lo + r as i128
        }
    }

    fn signal(start: u64, length: u64, little_endian: bool, signed: bool) -> Signal {
        let mut s = Signal::new("S".to_string(), 1);
        s.start = start;
        s.length = length;
        s.little_endian = little_endian;
        s.signed = signed;
        s
    }

    fn pack_by_segments(s: &Signal, raw: i128, dlc: usize) -> Vec<u8> {
        let mut data = vec![0u8; dlc];
        let u = (raw & ((1i128 << s.length) - 1)) as u64;
        for seg in segments(s) {
            let mask: u64 = (1u64 << seg.count) - 1;
            data[seg.byte as usize] |= (((u >> seg.raw_bit_lo) & mask) << seg.byte_bit_lo) as u8;
        }
        data
    }

    fn pack_one(s: &Signal, raw: i128, dlc: usize) -> Vec<u8> {
        let mut d = vec![0u8; dlc];
        insert_raw(&mut d, s, raw);
        d
    }

    #[test]
    fn matches_hand_derived_byte_patterns() {
        assert_eq!(pack_one(&signal(0, 16, true, false), 0x1234, 8)[..2], [0x34, 0x12]);
        assert_eq!(pack_one(&signal(7, 16, false, false), 0x1234, 8)[..2], [0x12, 0x34]);
        assert_eq!(pack_one(&signal(7, 12, false, false), 0xABC, 8)[..2], [0xAB, 0xC0]);
        assert_eq!(pack_one(&signal(11, 12, false, false), 0xABC, 8)[1..3], [0x0A, 0xBC]);
        assert_eq!(pack_one(&signal(48, 14, true, true), -1, 8)[6..8], [0xFF, 0x3F]);
        assert_eq!(pack_one(&signal(55, 2, false, false), 2, 8)[6], 0x80);
        assert_eq!(
            pack_one(&signal(191, 48, false, false), 0x0102_0304_0506, 64)[23..29],
            [1, 2, 3, 4, 5, 6]
        );
        let s = signal(48, 14, true, true);
        assert_eq!(raw_value(&pack_one(&s, -1, 8), &s), -1);
    }

    #[test]
    fn every_layout_round_trips_both_ways() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let dlc = 8usize;
        let mut layouts = 0;
        for little in [true, false] {
            for start in 0..(dlc as u64 * 8) {
                for length in 1..=64u64 {
                    let base = signal(start, length, little, false);
                    if !fits(&base, dlc) {
                        continue;
                    }
                    layouts += 1;
                    for signed in [false, true] {
                        let s = signal(start, length, little, signed);
                        let (lo, hi) = raw_range(length, signed);
                        let samples = [lo, hi, rng.between(lo, hi), rng.between(lo, hi)];
                        for raw in samples.iter() {
                            let bytes = pack_one(&s, *raw, dlc);
                            assert_eq!(raw_value(&bytes, &s), *raw, "{}|{}@{}", start, length, little);
                            assert_eq!(pack_by_segments(&s, *raw, dlc), bytes, "segments {}|{}", start, length);
                        }
                    }
                }
            }
        }
        // An 8-byte frame holds 2080 Intel and 2080 Motorola layouts.
        assert_eq!(layouts, 4160);
    }
}
