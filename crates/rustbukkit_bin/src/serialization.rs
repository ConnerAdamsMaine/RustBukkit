#![allow(dead_code)]

use std::ops::{BitOrAssign as _, ShrAssign as _};

use smallvec::SmallVec; // as Vec;

/// Represents `127` in variable-length integer encoding.
const B_SEGMENT: u32 = 0b0111_1111;

/// Represents `128` in variable-length integer encoding.
const B_CONTINUE: u8 = 0b1000_0000;

/// Represents `1` in variable-length integer encoding.
const B_ONE: u8 = 0b0000_1000;
const B_TWO: u8 = 0b0000_0010;
const B_EIGHT: u8 = 0b0000_1000;

#[must_use]
pub fn varint(value: i32) -> SmallVec<[u8; 5]> {
    // Vec<u8> {
    // let mut output: SmallVec<[u8; 5]> = SmallVec::new();
    let mut output: SmallVec<[u8; 5]> = SmallVec::new_const();
    let mut uv = value as u32;

    loop {
        let mut b = (uv & B_SEGMENT) as u8;
        uv.shr_assign(7);

        if uv != 0 {
            b.bitor_assign(B_CONTINUE);
        }
        output.push(b);

        if uv == 0 {
            break;
        }
    }

    output
}

#[must_use]
#[rustfmt::skip]
pub const fn boolean(input: bool) -> [u8; 1] {
    if input { [0x01] } else { [0x00] }
}

#[must_use]
#[inline(always)]
pub const fn float(input: f32) -> [u8; 4] {
    input.to_be_bytes()
}

#[must_use]
#[inline(always)]
pub const fn double(input: f64) -> [u8; 8] {
    input.to_be_bytes()
}

#[must_use]
#[inline(always)]
pub const fn short(input: i16) -> [u8; 2] {
    input.to_be_bytes()
}

#[must_use]
#[inline(always)]
pub const fn unsigned_short(input: u16) -> [u8; 2] {
    input.to_be_bytes()
}

#[must_use]
#[inline(always)]
pub const fn int(input: i32) -> [u8; 4] {
    input.to_be_bytes()
}

#[must_use]
#[inline(always)]
pub const fn long(input: i64) -> [u8; 8] {
    input.to_be_bytes()
}

#[must_use]
#[inline(always)]
pub const fn unsigned_long(input: u64) -> [u8; 8] {
    input.to_be_bytes()
}

#[must_use]
#[inline(always)]
pub fn try_string<S: AsRef<str>>(input: S) -> Option<SmallVec<[u8; 8]>> {
    // Option<[u8; 8]> {
    let mut output: SmallVec<[u8; 8]> = SmallVec::from_slice(&varint(input.as_ref().len() as i32));
    // Vec<u8> = varint(input.as_ref().len() as i32);
    // output.append(&mut input.as_ref().as_bytes().to_vec());
    output.append(&mut SmallVec::<[u8; 8]>::from_slice(input.as_ref().as_bytes()));
    output.into()
}

#[must_use]
#[inline(always)]
pub fn bitset(input: &[u64]) -> [u8; 8] {
    let mut output: [u8; 8] = [0; 8];
    input.iter().enumerate().for_each(|(i, val)| {
        let bytes = val.to_be_bytes();
        output[i * 8..(i + 1) * 8].copy_from_slice(&bytes);
    });
    output
}

#[must_use]
#[inline(always)]
pub const fn uuid(input: &u128) -> [u8; 16] {
    input.to_be_bytes()
}

#[must_use]
#[inline(always)]
pub fn prefixed_array(mut data: SmallVec<[u8; 8]>, len: i32) -> SmallVec<[u8; 5]> {
    // Vec<u8>, len: i32) -> Vec<u8> {
    // let mut output: Vec<u8> = varint(len);
    let mut output: SmallVec<[u8; 5]> = varint(len);
    output.append(&mut data);
    output
}
