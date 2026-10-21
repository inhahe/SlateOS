//! Units written syntax element by syntax element, for what no conformance
//! stream does: the tests' side of the bit reader.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    reason = "test code, on values the tests choose"
)]

use crate::nal::{Flags, Nal, Splitter};
use ffbits::Bits;

/// A unit written syntax element by syntax element, each recorded as
/// `trace_headers` would print it.
#[derive(Default)]
pub(crate) struct Syntax {
    bytes: Vec<u8>,
    len: usize,
    pub(crate) trace: Vec<(String, i64)>,
}

impl Syntax {
    pub(crate) fn bits(&mut self, v: u64, n: u32) {
        for k in (0..n).rev() {
            if self.len.is_multiple_of(8) {
                self.bytes.push(0);
            }
            if (v >> k) & 1 != 0 {
                *self.bytes.last_mut().unwrap() |= 0x80 >> (self.len % 8);
            }
            self.len += 1;
        }
    }

    pub(crate) fn u(&mut self, name: &str, n: u32, v: i64) -> &mut Self {
        self.bits(v as u64, n);
        self.trace.push((name.to_owned(), v));
        self
    }

    pub(crate) fn ue(&mut self, name: &str, v: i64) -> &mut Self {
        let x = v as u64 + 1;
        let n = 64 - x.leading_zeros();
        self.bits(0, n - 1);
        self.bits(x, n);
        self.trace.push((name.to_owned(), v));
        self
    }

    pub(crate) fn se(&mut self, name: &str, v: i64) -> &mut Self {
        let code = if v > 0 { 2 * v - 1 } else { -2 * v };
        self.ue(name, code);
        self.trace.last_mut().unwrap().1 = v;
        self
    }

    /// A scaling list: absent (`None`), the default (`Some(&[])`), or these
    /// values in zigzag order, each written as a delta from the last.
    pub(crate) fn list(&mut self, flag: &str, list: Option<&[u8]>) -> &mut Self {
        self.u(flag, 1, i64::from(list.is_some()));
        match list {
            None => {}
            Some([]) => {
                self.se("delta_scale[0]", -8);
            }
            Some(values) => {
                let mut last = 8i64;
                for (j, &v) in values.iter().enumerate() {
                    let delta = (i64::from(v) - last + 128).rem_euclid(256) - 128;
                    self.se(&format!("delta_scale[{j}]"), delta);
                    last = i64::from(v);
                }
            }
        }
        self
    }

    /// How many bits are written: where the next element would start,
    /// counted from the unit's first bit as `trace_headers` counts.
    pub(crate) fn position(&self) -> usize {
        self.len + 8
    }

    /// The stop bit and alignment, the header in front, and the escapes a
    /// stream carries.
    pub(crate) fn finish(&mut self, header: u8) -> Vec<u8> {
        self.bits(1, 1);
        while !self.len.is_multiple_of(8) {
            self.bits(0, 1);
        }
        let mut out = vec![header];
        let mut zeros = 0;
        for &b in &self.bytes {
            if zeros >= 2 && b <= 3 {
                out.push(3);
                zeros = 0;
            }
            out.push(b);
            zeros = if b == 0 { zeros + 1 } else { 0 };
        }
        out
    }
}

/// `unit` as the decoder splits it from a packet of its own: its reader,
/// past the header, and the unit.
pub(crate) fn read_unit<R>(unit: &[u8], f: impl FnOnce(&mut Bits<'_>, &Nal) -> R) -> R {
    let mut packet = vec![0, 0, 0, 1];
    packet.extend_from_slice(unit);
    let mut s = Splitter::default();
    s.split(&packet, 4, Flags::default()).unwrap();
    let nal = s.nals()[0];
    let mut gb = s.reader(&packet, &nal).unwrap();
    f(&mut gb, &nal)
}
