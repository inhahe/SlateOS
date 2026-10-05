//! BCJ2 as 7-Zip decodes it in a 7z archive (method `0303011B`): `C/Bcj2.c`
//! and the decoder of `CPP/7zip/Compress/Bcj2Coder.cpp`, from the LZMA SDK
//! 26.00 (Igor Pavlov, public domain), ported.
//!
//! BCJ2 makes x86 code compress better by taking the 32-bit targets of
//! `CALL` (`E8`) and `JMP` (`E9`, and `0F 8x` conditional jumps) out of the
//! instruction stream: the code without them is stream MAIN, the call and
//! jump targets -- as absolute addresses, big-endian -- streams CALL and
//! JUMP, and a range-coded bit for every candidate opcode, saying whether
//! its operand was taken out, stream RC. Decoding interleaves the four
//! back into the code.
//!
//! The four inputs are given whole. `Bcj2Coder.cpp` reads them in buffers
//! and `Bcj2Dec_Decode` stops whenever one runs dry, to be called again
//! with more; given all of each at once, it stops only where an input has
//! no more -- the same decisions -- and the reading rules that matter at
//! the ends are kept: a CALL or JUMP stream is read four bytes at a time,
//! and one to three bytes left over at its end are a damaged stream.

// Positions are within the inputs and the output, checked where they index;
// the range coder's arithmetic is on `u32`, wrapping where `Bcj2.c` relies
// on it (`wrapping_*`).
#![allow(clippy::arithmetic_side_effects)]

use alloc::vec::Vec;

use crate::lzma_coder::Coded;

/// `BCJ2_STREAM_*`
const MAIN: usize = 0;
const CALL: usize = 1;
const JUMP: usize = 2;
const RC: usize = 3;
/// `BCJ2_DEC_STATE_ORIG_0`: up to four bytes of a target still to write.
const STATE_ORIG_0: u32 = 4;
/// `BCJ2_DEC_STATE_ORIG`: the output is full.
const STATE_ORIG: u32 = 8;
/// `BCJ2_DEC_STATE_ERROR`
const STATE_ERROR: u32 = 9;

/// `kTopValue`
const TOP: u32 = 1 << 24;
/// `kNumBitModelTotalBits`
const NUM_BIT_MODEL_TOTAL_BITS: u32 = 11;
/// `kBitModelTotal`
const BIT_MODEL_TOTAL: u32 = 1 << NUM_BIT_MODEL_TOTAL_BITS;
/// `kNumMoveBits`
const NUM_MOVE_BITS: u32 = 5;

/// `BCJ2_IS_32BIT_STREAM`
const fn is_32bit_stream(s: usize) -> bool {
    s == CALL || s == JUMP
}

/// A probability after a bit; every probability is below 2^11.
#[allow(clippy::cast_possible_truncation)]
const fn prob16(v: u32) -> u16 {
    v as u16
}

/// The low byte.
#[allow(clippy::cast_possible_truncation)]
const fn low8(v: u32) -> u8 {
    v as u8
}

/// `CBcj2Dec`, with the input buffers as positions in the whole inputs.
struct Bcj2Dec<'a> {
    inputs: [&'a [u8]; 4],
    /// `bufs[]`: where each input is read next.
    bufs: [usize; 4],
    /// `lims[]`: how far each input has been made available.
    lims: [usize; 4],
    dest_lim: usize,
    state: u32,
    ip: u32,
    temp: u32,
    range: u32,
    code: u32,
    probs: [u16; 2 + 256],
}

impl<'a> Bcj2Dec<'a> {
    /// `Bcj2Dec_Init`
    fn new(inputs: [&'a [u8]; 4], dest_lim: usize) -> Self {
        Self {
            inputs,
            bufs: [0; 4],
            lims: [0; 4],
            dest_lim,
            state: RC as u32,
            ip: 0,
            temp: 0,
            range: 0,
            code: 0,
            probs: [prob16(BIT_MODEL_TOTAL >> 1); 2 + 256],
        }
    }

    fn byte(&mut self, s: usize) -> Option<u32> {
        let b = self.inputs.get(s)?.get(*self.bufs.get(s)?).copied()?;
        if let Some(p) = self.bufs.get_mut(s) {
            *p += 1;
        }
        Some(u32::from(b))
    }

    fn at_lim(&self, s: usize) -> bool {
        self.bufs.get(s) == self.lims.get(s)
    }

    /// The next big-endian 32-bit value of stream `s`, which has four
    /// bytes available.
    fn be32(&mut self, s: usize) -> u32 {
        let mut v = 0u32;
        for _ in 0..4 {
            v = (v << 8) | self.byte(s).unwrap_or(0);
        }
        v
    }

    /// `Bcj2Dec_Decode`: decodes onto `out` until an input runs out or the
    /// output is full. `Err` is `SZ_ERROR_DATA`: the range coder's first
    /// bytes are wrong.
    #[allow(clippy::too_many_lines)]
    fn decode(&mut self, out: &mut Vec<u8>) -> Result<(), ()> {
        let mut v = self.temp;
        if self.range <= 5 {
            let mut code = self.code;
            self.state = STATE_ERROR;
            while self.range != 5 {
                if self.range == 1 && code != 0 {
                    return Err(());
                }
                if self.at_lim(RC) {
                    self.state = RC as u32;
                    return Ok(());
                }
                code = (code << 8) | self.byte(RC).unwrap_or(0);
                self.code = code;
                self.range += 1;
            }
            if code == 0xFFFF_FFFF {
                return Err(());
            }
            self.range = 0xFFFF_FFFF;
        }
        {
            let mut state = self.state;
            if is_32bit_stream(state as usize) {
                let s = state as usize;
                if self.at_lim(s) {
                    return Ok(());
                }
                let ip = self.ip.wrapping_add(4);
                v = self.be32(s).wrapping_sub(ip);
                self.ip = ip;
                state = STATE_ORIG_0;
            }
            if state.wrapping_sub(STATE_ORIG_0) < 4 {
                loop {
                    if out.len() == self.dest_lim {
                        self.state = state;
                        self.temp = v;
                        return Ok(());
                    }
                    out.push(low8(v));
                    state += 1;
                    if state == STATE_ORIG_0 + 4 {
                        break;
                    }
                    v >>= 8;
                }
            }
        }

        loop {
            if self.range < TOP {
                if self.at_lim(RC) {
                    self.state = RC as u32;
                    self.temp = v;
                    return Ok(());
                }
                self.range <<= 8;
                self.code = (self.code << 8) | self.byte(RC).unwrap_or(0);
            }
            // Copy MAIN until a candidate opcode -- E8, E9, or 0F 8x.
            loop {
                if self.at_lim(MAIN) || out.len() == self.dest_lim {
                    self.state = if self.at_lim(MAIN) {
                        MAIN as u32
                    } else {
                        STATE_ORIG
                    };
                    self.temp = v;
                    return Ok(());
                }
                let b = self.byte(MAIN).unwrap_or(0);
                out.push(low8(b));
                self.ip = self.ip.wrapping_add(1);
                v = (v << 24) | b;
                if ((b + (0x100 - 0xE8)) & 0xFE) == 0
                    || (v.wrapping_sub((0x0F << 24) + 0x80) & (((1u32 << (4 + 24)) - 1) << 4)) == 0
                {
                    break;
                }
            }
            // The bit: was this opcode's operand taken out?
            let c = (v.wrapping_add(0x17) >> 6) & 1;
            let index = ((0u32.wrapping_sub(c) & ((v >> 24) & 0xFF)) + c + ((v >> 5) & 1)) as usize;
            let Some(&ttt) = self.probs.get(index) else {
                return Err(());
            };
            let ttt = u32::from(ttt);
            let bound = (self.range >> NUM_BIT_MODEL_TOTAL_BITS) * ttt;
            if self.code < bound {
                self.range = bound;
                if let Some(p) = self.probs.get_mut(index) {
                    *p = prob16(ttt + ((BIT_MODEL_TOTAL - ttt) >> NUM_MOVE_BITS));
                }
                continue;
            }
            self.range -= bound;
            self.code -= bound;
            if let Some(p) = self.probs.get_mut(index) {
                *p = prob16(ttt - (ttt >> NUM_MOVE_BITS));
            }

            // It was: the target, from CALL (after E8) or JUMP.
            let cj = (((v.wrapping_add(0x57)) >> 6) & 1) as usize + CALL;
            if self.at_lim(cj) {
                self.state = cj as u32;
                break;
            }
            let ip = self.ip.wrapping_add(4);
            v = self.be32(cj).wrapping_sub(ip);
            self.ip = ip;
            let rem = self.dest_lim.saturating_sub(out.len());
            if rem < 4 {
                for _ in 0..rem {
                    out.push(low8(v));
                    v >>= 8;
                }
                self.temp = v;
                self.state = STATE_ORIG_0 + u32::try_from(rem).unwrap_or(0);
                break;
            }
            out.extend_from_slice(&v.to_le_bytes());
            v >>= 24;
        }

        if self.range < TOP && !self.at_lim(RC) {
            self.range <<= 8;
            self.code = (self.code << 8) | self.byte(RC).unwrap_or(0);
        }
        Ok(())
    }
}

/// `NCompress::NBcj2::CDecoder::Code` in finish mode, for the four inputs
/// in 7z's order -- MAIN, CALL, JUMP, RC -- and an output of `out_size`.
pub(crate) fn decode(inputs: [&[u8]; 4], out_size: usize) -> Coded {
    let mut dec = Bcj2Dec::new(inputs, out_size);
    let mut out = Vec::new();
    // `ReadInStream`: each input is read once, whole; what a CALL or JUMP
    // stream holds past its last whole four bytes is kept back.
    let mut read = [false; 4];
    let mut crit_ok = true;

    loop {
        if dec.decode(&mut out).is_err() {
            crit_ok = false;
            break;
        }
        let state = dec.state as usize;
        if state >= 4 {
            // `BCJ2_DEC_STATE_ORIG*`: the output is full -- the rest of the
            // size has been given.
            break;
        }
        // The decoder wants more of input `state`.
        let len = inputs.get(state).map_or(0, |i| i.len());
        if read.get(state).copied().unwrap_or(true) {
            // Read before: nothing more.
            break;
        }
        if let Some(r) = read.get_mut(state) {
            *r = true;
        }
        let mut avail = len;
        if is_32bit_stream(state) {
            // Four bytes at a time. The one to three past the last four
            // are never read, so a stream that has them is not used to its
            // last byte and `ok` below refuses it -- `Bcj2Coder.cpp`'s
            // extra-bytes error, with no flag of its own: whichever way
            // decoding ends, it cannot end `ok` with them unread.
            avail -= len & 3;
        }
        if avail == 0 {
            // An empty input: decoding stops here.
            break;
        }
        if let Some(l) = dec.lims.get_mut(state) {
            *l = avail;
        }
    }

    let ok = crit_ok
        // `Bcj2Dec_IsMaybeFinished_code`
        && dec.code == 0
        && (dec.state == MAIN as u32 || dec.state == STATE_ORIG)
        && out.len() == out_size
        // Every input used, to its last byte.
        && (0..4).all(|i| {
            dec.bufs.get(i).copied().unwrap_or(0) == inputs.get(i).map_or(0, |s| s.len())
        });
    Coded { out, ok }
}
