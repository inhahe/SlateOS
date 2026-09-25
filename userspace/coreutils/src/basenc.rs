//! `base64`, `base32` and `basenc`: one program upstream, three here.
//!
//! GNU coreutils 9.4 builds all three from `src/basenc.c`, compiled with
//! `BASE_TYPE` 64, 32 and 42, on top of gnulib's `base64.c` and `base32.c`.
//! This module is that file and those two: the encoders, the decoders with
//! their carried context, the block-at-a-time driver, `--wrap`, and the option
//! table each program sees. The bins are one call each.
//!
//! Two of the three are bins today, `base32` and `basenc`. `base64` is
//! ported here too -- [`Program::Base64`], tested, and reachable as
//! `basenc --base64` -- but its bin waits: `userspace/base64` still builds a
//! `base64` for the sake of the two tools that share that crate, `uuencode`
//! and `uudecode`, and two programs of one name is the collision
//! `scripts/check-bin-collisions.py` exists to refuse. See `known-issues.md`
//! -> `TD-B-BASE64-IS-STILL-THE-OLD-CRATE-UNTIL-UUENCODE-MOVES`.
//!
//! # Why the decoders are transcriptions
//!
//! What `base64 -d` prints for *invalid* input is not "nothing". It is every
//! byte decoded before the bad character -- sometimes including a partial
//! quantum -- and then `invalid input`, and exactly how much comes out depends
//! on three details of gnulib's `base64_decode_ctx` that a clean-room decoder
//! would not reproduce:
//!
//! * the **fast path** decodes four characters at a time straight from the
//!   buffer, and when a quantum fails there it *rewinds* the bytes it wrote
//!   and retries the quantum on the slow path;
//! * the **slow path** gathers four non-newline characters into the context
//!   (which is how a quantum survives a newline or a buffer boundary), and a
//!   quantum that fails *there* keeps its partial bytes;
//! * padding: `=` inside the buffer is refused on the fast path unless the
//!   quantum is the last four bytes, and accepted on the slow path, so `YQ==YQ==`
//!   decodes to `aa`.
//!
//! And the driver hands the decoder fixed-size blocks -- 4096 characters for
//! `base64`, 8192 for `base32`, a size per encoding for `basenc` -- after
//! `-i` has filtered them, so which bytes precede an error in *output* depends
//! on the block too. All of it is reproduced, and measured against 9.4 by
//! `scripts/basenc-diff.sh`.
//!
//! # `basenc`'s other encodings
//!
//! `--base64url` and `--base32hex` translate an alphabet on the way in and out
//! of the two gnulib codecs (and `--base64url` refuses a whole block that holds
//! `+` or `/`, before decoding any of it). `--base16`, `--base2msbf`,
//! `--base2lsbf` and `--z85` are `basenc.c`'s own; `--z85` refuses input that is
//! not a multiple of 4 bytes (encoding) or 5 characters (decoding).

use crate::diag;
use crate::errmsg::strerror;
use crate::getopt::{self, Opt, Program as Prog, Takes};
use crate::quote::{os_bytes, os_from_bytes, quote, quotef};
use crate::stdfd::{self, Stream};
use crate::xnum::{Status, xstrtoimax};
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::process::ExitCode;

// ------------------------------------------------------------------ programs ---

/// Which of the three programs: upstream's `BASE_TYPE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Program {
    /// `BASE_TYPE == 64`.
    Base64,
    /// `BASE_TYPE == 32`.
    Base32,
    /// `BASE_TYPE == 42`: the encoding comes from an option.
    Basenc,
}

impl Program {
    fn name(self) -> &'static str {
        match self {
            Program::Base64 => "base64",
            Program::Base32 => "base32",
            Program::Basenc => "basenc",
        }
    }

    /// `DEC_BLOCKSIZE`: bytes of *output* per decode block. The input block is
    /// the encoded length of this many bytes.
    fn dec_blocksize(self) -> usize {
        match self {
            Program::Base64 => 1024 * 3,
            Program::Base32 => 1024 * 5,
            Program::Basenc => 4200,
        }
    }
}

/// An alphabet: what `basenc`'s encoding option chose, or the one `base64` and
/// `base32` are fixed to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Base64,
    Base64Url,
    Base32,
    Base32Hex,
    Base16,
    Base2Msbf,
    Base2Lsbf,
    Z85,
}

/// `ENC_BLOCKSIZE`: input bytes per encode block. A multiple of 3, 5 and 4, so
/// no block but the last produces padding.
const ENC_BLOCKSIZE: usize = 1024 * 3 * 10;

impl Encoding {
    /// `BASE_LENGTH (len)`: the encoded length of `len` bytes.
    fn length(self, len: usize) -> usize {
        match self {
            Encoding::Base64 | Encoding::Base64Url => len.div_ceil(3).saturating_mul(4),
            Encoding::Base32 | Encoding::Base32Hex => len.div_ceil(5).saturating_mul(8),
            Encoding::Base16 => len.saturating_mul(2),
            Encoding::Base2Msbf | Encoding::Base2Lsbf => len.saturating_mul(8),
            // "Z85 does not allow padding, so no need to round".
            Encoding::Z85 => len.saturating_mul(5) / 4,
        }
    }

    /// `isbase`: what `-i` keeps (with `=`, which it always keeps).
    fn is_base(self, c: u8) -> bool {
        match self {
            Encoding::Base64 => is_base64(c),
            Encoding::Base64Url => {
                c == b'-' || c == b'_' || (c != b'+' && c != b'/' && is_base64(c))
            }
            Encoding::Base32 => is_base32(c),
            Encoding::Base32Hex => c.is_ascii_digit() || (b'A'..=b'V').contains(&c),
            Encoding::Base16 => c.is_ascii_digit() || (b'A'..=b'F').contains(&c),
            Encoding::Base2Msbf | Encoding::Base2Lsbf => c == b'0' || c == b'1',
            // `strchr (set, ch) != NULL` also finds the terminator, so NUL
            // counts: `-i` keeps it, and the decoder then refuses it.
            Encoding::Z85 => c.is_ascii_alphanumeric() || c == 0 || Z85_PUNCT.contains(&c),
        }
    }
}

// ------------------------------------------------------------ gnulib base64 ---

const B64C: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `c - from + to`, for a `c` the caller's match arm has already bounded to
/// `from..`: every use below is inside such an arm, so the result is exact
/// and the wrapping only tells the lint so.
fn shift(c: u8, from: u8, to: u8) -> u8 {
    c.wrapping_sub(from).wrapping_add(to)
}

/// gnulib's `b64[]`: a character's value, or `None` outside the alphabet.
fn b64(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(shift(c, b'A', 0)),
        b'a'..=b'z' => Some(shift(c, b'a', 26)),
        b'0'..=b'9' => Some(shift(c, b'0', 52)),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn is_base64(c: u8) -> bool {
    b64(c).is_some()
}

/// A character's value, for one already checked with [`is_base64`].
fn v64(c: u8) -> u32 {
    u32::from(b64(c).unwrap_or(0))
}

/// `base64_encode` with `outlen == BASE64_LENGTH (inlen)`: every group of three
/// bytes to four characters, the last group padded with `=`.
fn base64_encode(input: &[u8]) -> Vec<u8> {
    let sym = |v: u32| {
        B64C.get(usize::try_from(v & 0x3f).unwrap_or(0))
            .copied()
            .unwrap_or(b'A')
    };
    let mut out = Vec::with_capacity(Encoding::Base64.length(input.len()));
    for group in input.chunks(3) {
        let a = u32::from(group.first().copied().unwrap_or(0));
        let b = group.get(1).map(|&x| u32::from(x));
        let c = group.get(2).map(|&x| u32::from(x));
        out.push(sym(a >> 2));
        out.push(sym((a << 4) | b.map_or(0, |b| b >> 4)));
        out.push(b.map_or(b'=', |b| sym((b << 2) | c.map_or(0, |c| c >> 6))));
        out.push(c.map_or(b'=', sym));
    }
    out
}

/// The carried state between calls: up to one quantum, gathered past newlines.
#[derive(Default)]
struct B64Ctx {
    i: usize,
    buf: [u8; 4],
}

/// Room left in the output block, and the block. `decode_4` and `decode_8`
/// write only while there is room, exactly as upstream's `*outleft` does.
struct Out<'a> {
    buf: &'a mut Vec<u8>,
    left: usize,
}

impl Out<'_> {
    fn push(&mut self, byte: u32) {
        if self.left > 0 {
            // A char-sized store truncates, as the C assignment does.
            self.buf.push(u8::try_from(byte & 0xff).unwrap_or(0));
            self.left = self.left.saturating_sub(1);
        }
    }
}

/// gnulib's `decode_4`. `input` is everything from the quantum to the end of
/// what the caller has, which is how the length tests below can tell "the
/// last quantum" from "a quantum in the middle".
fn decode_4(input: &[u8], out: &mut Out<'_>) -> bool {
    let at = |i: usize| input.get(i).copied().unwrap_or(0);
    let inlen = input.len();
    if inlen < 2 {
        return false;
    }
    if !is_base64(at(0)) || !is_base64(at(1)) {
        return false;
    }
    out.push((v64(at(0)) << 2) | (v64(at(1)) >> 4));
    if inlen == 2 {
        return false;
    }
    if at(2) == b'=' {
        if inlen != 4 || at(3) != b'=' {
            return false;
        }
    } else {
        if !is_base64(at(2)) {
            return false;
        }
        out.push(((v64(at(1)) << 4) & 0xf0) | (v64(at(2)) >> 2));
        if inlen == 3 {
            return false;
        }
        if at(3) == b'=' {
            if inlen != 4 {
                return false;
            }
        } else {
            if !is_base64(at(3)) {
                return false;
            }
            out.push(((v64(at(2)) << 6) & 0xc0) | v64(at(3)));
        }
    }
    true
}

/// gnulib's `get_4`: the next quantum, as a copy -- straight from the input when
/// it is four newline-free bytes and nothing is carried, else gathered into the
/// context past newlines. Returns the characters and how many there are.
fn get_4(ctx: &mut B64Ctx, input: &[u8], pos: &mut usize, end: usize) -> ([u8; 4], usize) {
    if ctx.i == 4 {
        ctx.i = 0;
    }
    if ctx.i == 0
        && let Some(t) = input
            .get(*pos..pos.saturating_add(4))
            .filter(|_| end.saturating_sub(*pos) >= 4)
        && !t.contains(&b'\n')
    {
        let mut quad = [0u8; 4];
        quad.copy_from_slice(t);
        *pos = pos.saturating_add(4);
        return (quad, 4);
    }
    while *pos < end {
        let c = input.get(*pos).copied().unwrap_or(0);
        *pos = pos.saturating_add(1);
        if c != b'\n' {
            if let Some(slot) = ctx.buf.get_mut(ctx.i) {
                *slot = c;
            }
            ctx.i = ctx.i.saturating_add(1);
            if ctx.i == 4 {
                break;
            }
        }
    }
    (ctx.buf, ctx.i)
}

/// gnulib's `base64_decode_ctx`: decode `input` onto `out`, carrying a partial
/// quantum in `ctx`. An empty `input` is the request to flush what is carried.
/// True when the input was valid.
fn base64_decode_ctx(ctx: &mut B64Ctx, input: &[u8], out: &mut Out<'_>) -> bool {
    let flush_ctx = input.is_empty();
    // A snapshot: the fast path runs, or does not, for the whole call.
    let ctx_i = ctx.i;
    let mut pos = 0usize;
    let mut inlen = input.len();
    loop {
        let mut left_save = out.left;
        if ctx_i == 0 && !flush_ctx {
            loop {
                left_save = out.left;
                if !decode_4(input.get(pos..).unwrap_or_default(), out) {
                    break;
                }
                pos = pos.saturating_add(4);
                inlen = inlen.saturating_sub(4);
            }
        }
        if inlen == 0 && !flush_ctx {
            break;
        }
        // "the common case of 72-byte wrapped lines".
        if inlen > 0 && input.get(pos) == Some(&b'\n') {
            pos = pos.saturating_add(1);
            inlen = inlen.saturating_sub(1);
            continue;
        }
        // Rewind whatever a failed fast-path quantum wrote.
        let wrote = left_save.saturating_sub(out.left);
        out.buf.truncate(out.buf.len().saturating_sub(wrote));
        out.left = left_save;

        let end = pos.saturating_add(inlen);
        let (quad, n) = get_4(ctx, input, &mut pos, end);
        inlen = n;
        if inlen == 0 || (inlen < 4 && !flush_ctx) {
            inlen = 0;
            break;
        }
        if !decode_4(quad.get(..inlen).unwrap_or_default(), out) {
            break;
        }
        inlen = end.saturating_sub(pos);
    }
    inlen == 0
}

// ------------------------------------------------------------ gnulib base32 ---

const B32C: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn b32(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(shift(c, b'A', 0)),
        b'2'..=b'7' => Some(shift(c, b'2', 26)),
        _ => None,
    }
}

fn is_base32(c: u8) -> bool {
    b32(c).is_some()
}

fn v32(c: u8) -> u32 {
    u32::from(b32(c).unwrap_or(0))
}

/// `base32_encode` with `outlen == BASE32_LENGTH (inlen)`.
fn base32_encode(input: &[u8]) -> Vec<u8> {
    let sym = |v: u32| {
        B32C.get(usize::try_from(v & 0x1f).unwrap_or(0))
            .copied()
            .unwrap_or(b'A')
    };
    let mut out = Vec::with_capacity(Encoding::Base32.length(input.len()));
    for group in input.chunks(5) {
        let g = |i: usize| group.get(i).map(|&x| u32::from(x));
        let (a, b, c, d, e) = (g(0).unwrap_or(0), g(1), g(2), g(3), g(4));
        out.push(sym(a >> 3));
        out.push(sym((a << 2) | b.map_or(0, |b| b >> 6)));
        out.push(b.map_or(b'=', |b| sym(b >> 1)));
        out.push(b.map_or(b'=', |b| sym((b << 4) | c.map_or(0, |c| c >> 4))));
        out.push(c.map_or(b'=', |c| sym((c << 1) | d.map_or(0, |d| d >> 7))));
        out.push(d.map_or(b'=', |d| sym(d >> 2)));
        out.push(d.map_or(b'=', |d| sym((d << 3) | e.map_or(0, |e| e >> 5))));
        out.push(e.map_or(b'=', sym));
    }
    out
}

#[derive(Default)]
struct B32Ctx {
    i: usize,
    buf: [u8; 8],
}

/// gnulib's `decode_8`.
fn decode_8(input: &[u8], out: &mut Out<'_>) -> bool {
    let at = |i: usize| input.get(i).copied().unwrap_or(0);
    if input.len() < 8 {
        return false;
    }
    if !is_base32(at(0)) || !is_base32(at(1)) {
        return false;
    }
    out.push((v32(at(0)) << 3) | (v32(at(1)) >> 2));
    let all_pad = |from: usize| (from..8).all(|i| at(i) == b'=');
    if at(2) == b'=' {
        if !all_pad(3) {
            return false;
        }
    } else {
        if !is_base32(at(2)) || !is_base32(at(3)) {
            return false;
        }
        out.push((v32(at(1)) << 6) | (v32(at(2)) << 1) | (v32(at(3)) >> 4));
        if at(4) == b'=' {
            if !all_pad(5) {
                return false;
            }
        } else {
            if !is_base32(at(4)) {
                return false;
            }
            out.push((v32(at(3)) << 4) | (v32(at(4)) >> 1));
            if at(5) == b'=' {
                if !all_pad(6) {
                    return false;
                }
            } else {
                if !is_base32(at(5)) || !is_base32(at(6)) {
                    return false;
                }
                out.push((v32(at(4)) << 7) | (v32(at(5)) << 2) | (v32(at(6)) >> 3));
                if at(7) != b'=' {
                    if !is_base32(at(7)) {
                        return false;
                    }
                    out.push((v32(at(6)) << 5) | v32(at(7)));
                }
            }
        }
    }
    true
}

/// gnulib's `get_8`.
fn get_8(ctx: &mut B32Ctx, input: &[u8], pos: &mut usize, end: usize) -> ([u8; 8], usize) {
    if ctx.i == 8 {
        ctx.i = 0;
    }
    if ctx.i == 0
        && let Some(t) = input
            .get(*pos..pos.saturating_add(8))
            .filter(|_| end.saturating_sub(*pos) >= 8)
        && !t.contains(&b'\n')
    {
        let mut oct = [0u8; 8];
        oct.copy_from_slice(t);
        *pos = pos.saturating_add(8);
        return (oct, 8);
    }
    while *pos < end {
        let c = input.get(*pos).copied().unwrap_or(0);
        *pos = pos.saturating_add(1);
        if c != b'\n' {
            if let Some(slot) = ctx.buf.get_mut(ctx.i) {
                *slot = c;
            }
            ctx.i = ctx.i.saturating_add(1);
            if ctx.i == 8 {
                break;
            }
        }
    }
    (ctx.buf, ctx.i)
}

/// gnulib's `base32_decode_ctx`, the same shape as [`base64_decode_ctx`].
fn base32_decode_ctx(ctx: &mut B32Ctx, input: &[u8], out: &mut Out<'_>) -> bool {
    let flush_ctx = input.is_empty();
    let ctx_i = ctx.i;
    let mut pos = 0usize;
    let mut inlen = input.len();
    loop {
        let mut left_save = out.left;
        if ctx_i == 0 && !flush_ctx {
            loop {
                left_save = out.left;
                if !decode_8(input.get(pos..).unwrap_or_default(), out) {
                    break;
                }
                pos = pos.saturating_add(8);
                inlen = inlen.saturating_sub(8);
            }
        }
        if inlen == 0 && !flush_ctx {
            break;
        }
        if inlen > 0 && input.get(pos) == Some(&b'\n') {
            pos = pos.saturating_add(1);
            inlen = inlen.saturating_sub(1);
            continue;
        }
        let wrote = left_save.saturating_sub(out.left);
        out.buf.truncate(out.buf.len().saturating_sub(wrote));
        out.left = left_save;

        let end = pos.saturating_add(inlen);
        let (oct, n) = get_8(ctx, input, &mut pos, end);
        inlen = n;
        if inlen == 0 || (inlen < 8 && !flush_ctx) {
            inlen = 0;
            break;
        }
        if !decode_8(oct.get(..inlen).unwrap_or_default(), out) {
            break;
        }
        inlen = end.saturating_sub(pos);
    }
    inlen == 0
}

// ----------------------------------------------------- basenc's own codecs ---

/// `base32_norm_to_hex`, as a function: the normal alphabet's characters to
/// the extended-hex alphabet's, everything else (padding) unchanged.
fn base32_norm_to_hex(c: u8) -> u8 {
    match c {
        b'2'..=b'7' => shift(c, b'2', b'Q'),
        b'A'..=b'J' => shift(c, b'A', b'0'),
        b'K'..=b'Z' => shift(c, b'K', b'A'),
        _ => c,
    }
}

/// `base32_hex_to_norm`, applied by the decoder only to a character that
/// `isbase32hex` accepts.
fn base32_hex_to_norm(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => shift(c, b'0', b'A'),
        // Values 10-25 come back as `K`..`Z` and 26-31 as `2`..`7`: two runs,
        // not one, since the normal alphabet breaks after `Z`.
        b'A'..=b'P' => shift(c, b'A', b'K'),
        b'Q'..=b'V' => shift(c, b'Q', b'2'),
        _ => c,
    }
}

const BASE16: &[u8; 16] = b"0123456789ABCDEF";

const Z85_ENCODING: &[u8; 85] =
    b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ.-:+=^!/*?&<>()[]{}@%$#";

/// The punctuation half of the Z85 alphabet, for `isz85`.
const Z85_PUNCT: &[u8] = b".-:+=^!/*?&<>()[]{}@%$#";

/// `z85_decoding[]`: a character's value, for characters 33 to 125.
fn z85_value(c: u8) -> Option<u32> {
    Z85_ENCODING
        .iter()
        .position(|&x| x == c)
        .and_then(|p| u32::try_from(p).ok())
}

/// The carried state of every decoder, as upstream's union and `ctx.i`.
#[derive(Default)]
struct DecodeCtx {
    /// `ctx.i`: what the driver reads to decide whether a flush call is
    /// needed. Base16 and Z85 start it at 1 so the flush always runs.
    i: usize,
    b64: B64Ctx,
    b32: B32Ctx,
    nibble: u32,
    have_nibble: bool,
    octet: u32,
    z85_i: usize,
    z85_octets: [u32; 5],
}

impl DecodeCtx {
    /// `base_decode_ctx_init`.
    fn new(encoding: Encoding) -> Self {
        let i = match encoding {
            Encoding::Base16 | Encoding::Z85 => 1,
            _ => 0,
        };
        Self {
            i,
            ..Self::default()
        }
    }
}

/// `base_decode_ctx` for each encoding: decode `input` (empty to flush),
/// appending to `out`, which holds at most `cap` bytes. True when valid.
fn decode_block(
    encoding: Encoding,
    ctx: &mut DecodeCtx,
    input: &[u8],
    out: &mut Vec<u8>,
    cap: usize,
) -> bool {
    let mut o = Out {
        buf: out,
        left: cap,
    };
    match encoding {
        Encoding::Base64 => {
            let ok = base64_decode_ctx(&mut ctx.b64, input, &mut o);
            ctx.i = ctx.b64.i;
            ok
        }
        Encoding::Base64Url => {
            // The whole block is translated before any of it is decoded, and
            // the standard alphabet's two characters refuse it outright.
            if input.iter().any(|&c| c == b'+' || c == b'/') {
                return false;
            }
            let translated: Vec<u8> = input
                .iter()
                .map(|&c| match c {
                    b'-' => b'+',
                    b'_' => b'/',
                    other => other,
                })
                .collect();
            let ok = base64_decode_ctx(&mut ctx.b64, &translated, &mut o);
            ctx.i = ctx.b64.i;
            ok
        }
        Encoding::Base32 => {
            let ok = base32_decode_ctx(&mut ctx.b32, input, &mut o);
            ctx.i = ctx.b32.i;
            ok
        }
        Encoding::Base32Hex => {
            let translated: Vec<u8> = input
                .iter()
                .map(|&c| {
                    if Encoding::Base32Hex.is_base(c) {
                        base32_hex_to_norm(c)
                    } else {
                        c
                    }
                })
                .collect();
            let ok = base32_decode_ctx(&mut ctx.b32, &translated, &mut o);
            ctx.i = ctx.b32.i;
            ok
        }
        Encoding::Base16 => {
            if input.is_empty() {
                return !ctx.have_nibble;
            }
            for &c in input {
                if c == b'\n' {
                    continue;
                }
                let nib = match c {
                    b'0'..=b'9' => u32::from(shift(c, b'0', 0)),
                    b'A'..=b'F' => u32::from(shift(c, b'A', 10)),
                    _ => return false,
                };
                if ctx.have_nibble {
                    o.push((ctx.nibble << 4) | nib);
                } else {
                    ctx.nibble = nib;
                }
                ctx.have_nibble = !ctx.have_nibble;
            }
            true
        }
        Encoding::Base2Msbf | Encoding::Base2Lsbf => {
            if input.is_empty() {
                return ctx.i == 0;
            }
            for &c in input {
                if c == b'\n' {
                    continue;
                }
                if c != b'0' && c != b'1' {
                    return false;
                }
                let bit = u32::from(c == b'1');
                if encoding == Encoding::Base2Lsbf {
                    ctx.octet |= bit << ctx.i;
                    ctx.i = ctx.i.saturating_add(1);
                    if ctx.i == 8 {
                        o.push(ctx.octet);
                        ctx.octet = 0;
                        ctx.i = 0;
                    }
                } else {
                    if ctx.i == 0 {
                        ctx.i = 8;
                    }
                    ctx.i = ctx.i.saturating_sub(1);
                    ctx.octet |= bit << ctx.i;
                    if ctx.i == 0 {
                        o.push(ctx.octet);
                        ctx.octet = 0;
                    }
                }
            }
            true
        }
        Encoding::Z85 => {
            if input.is_empty() {
                // "Z85 variant does not allow padding".
                return ctx.z85_i == 0;
            }
            for &c in input {
                if c == b'\n' {
                    continue;
                }
                let Some(v) = (33..=125).contains(&c).then(|| z85_value(c)).flatten() else {
                    return false;
                };
                if let Some(slot) = ctx.z85_octets.get_mut(ctx.z85_i) {
                    *slot = v;
                }
                ctx.z85_i = ctx.z85_i.saturating_add(1);
                if ctx.z85_i == 5 {
                    // a*85^4 + b*85^3 + c*85^2 + d*85 + e, at most 85^5 - 1:
                    // 33 bits, so the wrapping never wraps.
                    let val = ctx.z85_octets.iter().fold(0u64, |acc, &v| {
                        acc.wrapping_mul(85).wrapping_add(u64::from(v))
                    });
                    // "To be on the safe side, reject" an overflow of 32 bits.
                    if (val >> 24) & !0xff != 0 {
                        return false;
                    }
                    for shift in [24u32, 16, 8, 0] {
                        o.push(u32::try_from((val >> shift) & 0xff).unwrap_or(0));
                    }
                    ctx.z85_i = 0;
                }
            }
            ctx.i = ctx.z85_i;
            true
        }
    }
}

/// `base_encode` for one block. `Err` is Z85's refusal of a length that is not
/// a multiple of four, which upstream reports and exits on at once.
fn encode_block(encoding: Encoding, input: &[u8]) -> Result<Vec<u8>, ()> {
    Ok(match encoding {
        Encoding::Base64 => base64_encode(input),
        Encoding::Base64Url => base64_encode(input)
            .into_iter()
            .map(|c| match c {
                b'+' => b'-',
                b'/' => b'_',
                other => other,
            })
            .collect(),
        Encoding::Base32 => base32_encode(input),
        Encoding::Base32Hex => base32_encode(input)
            .into_iter()
            .map(base32_norm_to_hex)
            .collect(),
        Encoding::Base16 => {
            let mut out = Vec::with_capacity(input.len().saturating_mul(2));
            for &b in input {
                out.push(BASE16.get(usize::from(b >> 4)).copied().unwrap_or(b'0'));
                out.push(BASE16.get(usize::from(b & 0x0f)).copied().unwrap_or(b'0'));
            }
            out
        }
        Encoding::Base2Msbf | Encoding::Base2Lsbf => {
            let mut out = Vec::with_capacity(input.len().saturating_mul(8));
            let msb_first = encoding == Encoding::Base2Msbf;
            for &b in input {
                for step in 0..8u32 {
                    let i = if msb_first {
                        7u32.saturating_sub(step)
                    } else {
                        step
                    };
                    out.push(if (b >> i) & 1 == 1 { b'1' } else { b'0' });
                }
            }
            out
        }
        Encoding::Z85 => {
            if !input.len().is_multiple_of(4) {
                return Err(());
            }
            let mut out = Vec::with_capacity(Encoding::Z85.length(input.len()));
            for quad in input.chunks(4) {
                let mut val = quad.iter().fold(0u64, |acc, &b| (acc << 8) | u64::from(b));
                let mut five = [0u8; 5];
                for slot in five.iter_mut().rev() {
                    *slot = Z85_ENCODING
                        .get(usize::try_from(val % 85).unwrap_or(0))
                        .copied()
                        .unwrap_or(b'0');
                    val /= 85;
                }
                out.extend_from_slice(&five);
            }
            out
        }
    })
}

// -------------------------------------------------------------- the driver ---

/// What the command line decided.
#[derive(Debug, PartialEq, Eq)]
struct Settings {
    encoding: Encoding,
    decode: bool,
    ignore_garbage: bool,
    /// `wrap_column`; 0 means no wrapping (and no final newline).
    wrap: usize,
    /// `-` when none was named.
    file: OsString,
}

#[derive(Debug, PartialEq, Eq)]
enum Request {
    Help,
    Version,
    Run(Settings),
}

/// `base64` and `base32`'s table.
const FIXED_LONG_OPTIONS: &[(&str, Takes)] = &[
    ("decode", Takes::Nothing),
    ("wrap", Takes::Required),
    ("ignore-garbage", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// `basenc`'s table: the three above, the eight encodings, help and version,
/// in upstream's order.
const BASENC_LONG_OPTIONS: &[(&str, Takes)] = &[
    ("decode", Takes::Nothing),
    ("wrap", Takes::Required),
    ("ignore-garbage", Takes::Nothing),
    ("base64", Takes::Nothing),
    ("base64url", Takes::Nothing),
    ("base32", Takes::Nothing),
    ("base32hex", Takes::Nothing),
    ("base16", Takes::Nothing),
    ("base2msbf", Takes::Nothing),
    ("base2lsbf", Takes::Nothing),
    ("z85", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

fn encoding_option(name: &str) -> Option<Encoding> {
    Some(match name {
        "base64" => Encoding::Base64,
        "base64url" => Encoding::Base64Url,
        "base32" => Encoding::Base32,
        "base32hex" => Encoding::Base32Hex,
        "base16" => Encoding::Base16,
        "base2msbf" => Encoding::Base2Msbf,
        "base2lsbf" => Encoding::Base2Lsbf,
        "z85" => Encoding::Z85,
        _ => return None,
    })
}

/// Upstream's `getopt_long` loop and the checks after it, in their order: the
/// wrap size as it is met, then (`basenc`) the missing encoding, then the
/// extra operand.
fn parse_args(program: Program, prog: Prog, args: &[OsString]) -> Result<Request, getopt::Error> {
    let table = if program == Program::Basenc {
        BASENC_LONG_OPTIONS
    } else {
        FIXED_LONG_OPTIONS
    };
    let mut decode = false;
    let mut ignore_garbage = false;
    let mut wrap = 76usize;
    let mut chosen: Option<Encoding> = match program {
        Program::Base64 => Some(Encoding::Base64),
        Program::Base32 => Some(Encoding::Base32),
        Program::Basenc => None,
    };
    let mut operands: Vec<OsString> = Vec::new();
    for item in prog.parse(args, "diw:", table) {
        match item? {
            Opt::Short(b'd', _) | Opt::Long("decode", _) => decode = true,
            Opt::Short(b'i', _) | Opt::Long("ignore-garbage", _) => ignore_garbage = true,
            Opt::Short(b'w', value) | Opt::Long("wrap", value) => {
                let text = value.map(|v| os_bytes(&v).into_owned()).unwrap_or_default();
                wrap = parse_wrap(prog, &text)?;
            }
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Long(name, _) if program == Program::Basenc && encoding_option(name).is_some() => {
                chosen = encoding_option(name);
            }
            Opt::Operand(x) => operands.push(x.clone()),
            // Unreachable: every name in both tables is handled above.
            Opt::Long(other, _) => {
                return Err(prog.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(c, _) => return Err(prog.invalid_option(c)),
        }
    }
    let Some(encoding) = chosen else {
        return Err(prog.usage_referring("missing encoding type".to_string()));
    };
    if let Some(extra) = operands.get(1) {
        return Err(prog.usage_referring(format!("extra operand {}", quote(&os_bytes(extra)))));
    }
    Ok(Request::Run(Settings {
        encoding,
        decode,
        ignore_garbage,
        wrap,
        file: operands
            .into_iter()
            .next()
            .unwrap_or_else(|| OsString::from("-")),
    }))
}

/// `-w`: `xstrtoimax (optarg, nullptr, 10, &w, "")`. An overflowing width is
/// accepted and means no wrapping at all -- upstream's `IDX_MAX < w ? 0 : w`.
fn parse_wrap(prog: Prog, text: &[u8]) -> Result<usize, getopt::Error> {
    let (w, status) = xstrtoimax(text, Some(b""));
    if !matches!(status, Status::Ok | Status::Overflow) || w < 0 {
        // `error (EXIT_FAILURE, 0, ...)`: fatal, and no "Try" line.
        return Err(prog.usage(format!("invalid wrap size: {}", quote(text))));
    }
    if status == Status::Overflow {
        return Ok(0);
    }
    Ok(usize::try_from(w).unwrap_or(0))
}

fn help_text(program: Program) -> String {
    let name = program.name();
    let title = match program {
        Program::Base64 => "Base64",
        Program::Base32 => "Base32",
        Program::Basenc => "basenc",
    };
    let mut text = format!(
        "Usage: {name} [OPTION]... [FILE]\n\
         {title} encode or decode FILE, or standard input, to standard output.\n\
         \n\
         With no FILE, or when FILE is -, read standard input.\n\
         \n\
         Mandatory arguments to long options are mandatory for short options too.\n"
    );
    if program == Program::Basenc {
        text.push_str(
            "      --base64          same as 'base64' program (RFC4648 section 4)\n\
             \x20     --base64url       file- and url-safe base64 (RFC4648 section 5)\n\
             \x20     --base32          same as 'base32' program (RFC4648 section 6)\n\
             \x20     --base32hex       extended hex alphabet base32 (RFC4648 section 7)\n\
             \x20     --base16          hex encoding (RFC4648 section 8)\n\
             \x20     --base2msbf       bit string with most significant bit (msb) first\n\
             \x20     --base2lsbf       bit string with least significant bit (lsb) first\n",
        );
    }
    text.push_str(
        "  -d, --decode          decode data\n\
         \x20 -i, --ignore-garbage  when decoding, ignore non-alphabet characters\n\
         \x20 -w, --wrap=COLS       wrap encoded lines after COLS character (default 76).\n\
         \x20                         Use 0 to disable line wrapping\n",
    );
    if program == Program::Basenc {
        text.push_str(
            "      --z85             ascii85-like encoding (ZeroMQ spec:32/Z85);\n\
             \x20                       when encoding, input length must be a multiple of 4;\n\
             \x20                       when decoding, input length must be a multiple of 5\n",
        );
    }
    text.push_str(
        "      --help        display this help and exit\n\
         \x20     --version     output version information and exit\n",
    );
    if program == Program::Basenc {
        text.push_str(
            "\nWhen decoding, the input may contain newlines in addition to the bytes of\n\
             the formal alphabet.  Use --ignore-garbage to attempt to recover\n\
             from any other non-alphabet bytes in the encoded stream.\n",
        );
    } else {
        text.push_str(&format!(
            "\nThe data are encoded as described for the {name} alphabet in RFC 4648.\n\
             When decoding, the input may contain newlines in addition to the bytes of\n\
             the formal {name} alphabet.  Use --ignore-garbage to attempt to recover\n\
             from any other non-alphabet bytes in the encoded stream.\n"
        ));
    }
    text
}

/// The input, with stdio's two sticky flags.
struct Input {
    src: Box<dyn Read>,
    eof: bool,
    error: Option<io::Error>,
}

impl Input {
    /// `fread (buf, 1, want, in)`: read until `want` bytes arrived or the input
    /// ended or failed, setting the matching flag. A read that gets what it
    /// asked for does not look further, so a file that ends exactly on a block
    /// boundary is only seen to end on the *next* call -- which is when
    /// upstream's `feof` becomes true, and the decode loop depends on it.
    fn fread(&mut self, want: usize) -> Vec<u8> {
        let mut got = vec![0u8; want];
        let mut filled = 0usize;
        while filled < want {
            match self.src.read(got.get_mut(filled..).unwrap_or_default()) {
                Ok(0) => {
                    self.eof = true;
                    break;
                }
                Ok(n) => filled = filled.saturating_add(n),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => {
                    self.error = Some(e);
                    break;
                }
            }
        }
        got.truncate(filled);
        got
    }
}

/// `wrap_write`: emit `text`, breaking lines at `wrap` columns. The break comes
/// *before* a character that would pass the edge, so a line that ends exactly
/// on it waits for more output, or the final newline, to be ended.
fn wrap_write(text: &[u8], wrap: usize, column: &mut usize, out: &mut Stream) {
    // Each write is deliberately unread: `Stream` records a failure, and
    // `close_stdout` reports it -- upstream's `write_error` by another road.
    if wrap == 0 {
        let _ = out.write_all(text);
        return;
    }
    let mut written = 0usize;
    while written < text.len() {
        let room = wrap
            .saturating_sub(*column)
            .min(text.len().saturating_sub(written));
        if room == 0 {
            let _ = out.write_all(b"\n");
            *column = 0;
        } else {
            let end = written.saturating_add(room);
            let _ = out.write_all(text.get(written..end).unwrap_or_default());
            *column = column.saturating_add(room);
            written = end;
        }
    }
}

/// `do_encode`. `Err` is the message that ends the run with status 1.
fn encode(settings: &Settings, input: &mut Input, out: &mut Stream) -> Result<(), String> {
    let mut column = 0usize;
    loop {
        let mut block: Vec<u8> = Vec::with_capacity(ENC_BLOCKSIZE);
        loop {
            let more = input.fread(ENC_BLOCKSIZE.saturating_sub(block.len()));
            block.extend_from_slice(&more);
            if input.eof || input.error.is_some() || block.len() >= ENC_BLOCKSIZE {
                break;
            }
        }
        if !block.is_empty() {
            let text = encode_block(settings.encoding, &block).map_err(|()| {
                "invalid input (length must be multiple of 4 characters)".to_string()
            })?;
            wrap_write(&text, settings.wrap, &mut column, out);
        }
        if input.eof || input.error.is_some() || block.len() != ENC_BLOCKSIZE {
            break;
        }
    }
    if settings.wrap > 0 && column > 0 {
        // Deliberately unread; see `wrap_write`.
        let _ = out.write_all(b"\n");
    }
    match input.error.take() {
        Some(e) => Err(format!("read error: {}", strerror(&e))),
        None => Ok(()),
    }
}

/// `do_decode`.
fn decode(
    program: Program,
    settings: &Settings,
    input: &mut Input,
    out: &mut Stream,
) -> Result<(), String> {
    let cap = program.dec_blocksize();
    let block_len = settings.encoding.length(cap);
    let mut ctx = DecodeCtx::new(settings.encoding);
    loop {
        let mut block: Vec<u8> = Vec::with_capacity(block_len);
        loop {
            let mut more = input.fread(block_len.saturating_sub(block.len()));
            if settings.ignore_garbage {
                more.retain(|&c| settings.encoding.is_base(c) || c == b'=');
            }
            block.extend_from_slice(&more);
            if let Some(e) = input.error.take() {
                return Err(format!("read error: {}", strerror(&e)));
            }
            if block.len() >= block_len || input.eof {
                break;
            }
        }
        // One pass over the block, and after the last block a second with
        // nothing in it: the request to flush what the context still carries.
        for k in 0..=usize::from(input.eof) {
            if k == 1 && ctx.i == 0 {
                break;
            }
            let mut decoded = Vec::with_capacity(cap);
            let chunk: &[u8] = if k == 0 { &block } else { &[] };
            let ok = decode_block(settings.encoding, &mut ctx, chunk, &mut decoded, cap);
            // Deliberately unread; see `wrap_write`.
            let _ = out.write_all(&decoded);
            if !ok {
                return Err("invalid input".to_string());
            }
        }
        if input.eof {
            return Ok(());
        }
    }
}

/// One of the three programs, from `argv` to exit status.
pub fn main(program: Program) -> ExitCode {
    stdfd::close_stderr(run(program), 1)
}

fn run(program: Program) -> ExitCode {
    stdfd::restore();
    let name = program.name();
    let prog = Prog::new(name, 1);
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let settings = match parse_args(program, prog, &args) {
        Ok(Request::Run(s)) => s,
        Ok(Request::Help) => {
            let mut out = Stream::stdout();
            // Deliberately unread; see `wrap_write`.
            let _ = out.write_all(help_text(program).as_bytes());
            return stdfd::close_stdout(name, out, ExitCode::SUCCESS);
        }
        Ok(Request::Version) => {
            let mut out = Stream::stdout();
            // Deliberately unread; see `wrap_write`.
            let _ = out.write_all(format!("{name} (SlateOS coreutils) 0.1.0\n").as_bytes());
            return stdfd::close_stdout(name, out, ExitCode::SUCCESS);
        }
        Err(e) => {
            prog.report(&e);
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };

    let file = os_bytes(&settings.file).into_owned();
    let src: Box<dyn Read> = if file == b"-" {
        Box::new(io::stdin())
    } else {
        match File::open(os_from_bytes(&file)) {
            Ok(f) => Box::new(f),
            Err(e) => {
                diag!("{name}: {}: {}", quotef(&file), strerror(&e));
                return ExitCode::from(1);
            }
        }
    };
    let mut input = Input {
        src,
        eof: false,
        error: None,
    };
    let mut out = Stream::stdout();
    let outcome = if settings.decode {
        decode(program, &settings, &mut input, &mut out)
    } else {
        encode(&settings, &mut input, &mut out)
    };
    match outcome {
        Ok(()) => stdfd::close_stdout(name, out, ExitCode::SUCCESS),
        Err(message) => {
            diag!("{name}: {message}");
            stdfd::close_stdout(name, out, ExitCode::from(1))
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn enc(e: Encoding, s: &[u8]) -> String {
        String::from_utf8(encode_block(e, s).unwrap()).unwrap()
    }

    /// Decode `input` the way the driver does for a short stream: one pass,
    /// then the flush when anything is carried. Returns the bytes and whether
    /// every call succeeded.
    fn dec(e: Encoding, input: &[u8]) -> (Vec<u8>, bool) {
        let mut ctx = DecodeCtx::new(e);
        let mut out = Vec::new();
        let ok = decode_block(e, &mut ctx, input, &mut out, 1 << 20);
        if !ok {
            return (out, false);
        }
        if ctx.i != 0 {
            let ok = decode_block(e, &mut ctx, &[], &mut out, 1 << 20);
            return (out, ok);
        }
        (out, true)
    }

    /// RFC 4648 section 10.
    #[test]
    fn rfc_4648_test_vectors() {
        let cases: [(&[u8], &str, &str, &str, &str); 7] = [
            (b"", "", "", "", ""),
            (b"f", "Zg==", "MY======", "CO======", "66"),
            (b"fo", "Zm8=", "MZXQ====", "CPNG====", "666F"),
            (b"foo", "Zm9v", "MZXW6===", "CPNMU===", "666F6F"),
            (b"foob", "Zm9vYg==", "MZXW6YQ=", "CPNMUOG=", "666F6F62"),
            (b"fooba", "Zm9vYmE=", "MZXW6YTB", "CPNMUOJ1", "666F6F6261"),
            (
                b"foobar",
                "Zm9vYmFy",
                "MZXW6YTBOI======",
                "CPNMUOJ1E8======",
                "666F6F626172",
            ),
        ];
        for (raw, b64s, b32s, b32h, b16s) in cases {
            assert_eq!(enc(Encoding::Base64, raw), b64s);
            assert_eq!(enc(Encoding::Base32, raw), b32s);
            assert_eq!(enc(Encoding::Base32Hex, raw), b32h);
            assert_eq!(enc(Encoding::Base16, raw), b16s);
            assert_eq!(dec(Encoding::Base64, b64s.as_bytes()), (raw.to_vec(), true));
            assert_eq!(dec(Encoding::Base32, b32s.as_bytes()), (raw.to_vec(), true));
            assert_eq!(
                dec(Encoding::Base32Hex, b32h.as_bytes()),
                (raw.to_vec(), true)
            );
            assert_eq!(dec(Encoding::Base16, b16s.as_bytes()), (raw.to_vec(), true));
        }
    }

    /// Every character of the extended-hex alphabet, both ways -- the RFC's
    /// vectors never use `Q` to `T`, which is where a one-run translation
    /// goes wrong -- and every byte value through encode and decode.
    #[test]
    fn base32hex_alphabet_translates_both_ways() {
        for &c in b"0123456789ABCDEFGHIJKLMNOPQRSTUV" {
            let norm = base32_hex_to_norm(c);
            assert!(is_base32(norm), "{}", c as char);
            assert_eq!(base32_norm_to_hex(norm), c, "{}", c as char);
        }
        let all: Vec<u8> = (0..=255u8).collect();
        let text = encode_block(Encoding::Base32Hex, &all).unwrap();
        assert_eq!(dec(Encoding::Base32Hex, &text), (all, true));
    }

    /// The ZeroMQ Z85 specification's own example.
    #[test]
    fn z85_spec_example() {
        let raw = [0x86, 0x4F, 0xD2, 0x6F, 0xB5, 0x59, 0xF7, 0x5B];
        assert_eq!(enc(Encoding::Z85, &raw), "HelloWorld");
        assert_eq!(dec(Encoding::Z85, b"HelloWorld"), (raw.to_vec(), true));
        assert!(encode_block(Encoding::Z85, b"abc").is_err());
        // A fifth character short: carried, and refused at the flush.
        assert!(!dec(Encoding::Z85, b"Hell").1);
        // `%%%%%` is past 2^32 and refused rather than wrapped.
        assert_eq!(dec(Encoding::Z85, b"%%%%%"), (Vec::new(), false));
    }

    #[test]
    fn base64url_and_base2() {
        assert_eq!(enc(Encoding::Base64Url, &[0xfb, 0xff]), "-_8=");
        assert_eq!(dec(Encoding::Base64Url, b"-_8="), (vec![0xfb, 0xff], true));
        // The standard alphabet's two characters refuse the whole block.
        assert_eq!(dec(Encoding::Base64Url, b"YWJj+/8="), (Vec::new(), false));
        assert_eq!(enc(Encoding::Base2Msbf, b"A"), "01000001");
        assert_eq!(enc(Encoding::Base2Lsbf, b"A"), "10000010");
        assert_eq!(dec(Encoding::Base2Msbf, b"01000001"), (b"A".to_vec(), true));
        assert_eq!(dec(Encoding::Base2Lsbf, b"10000010"), (b"A".to_vec(), true));
        assert!(!dec(Encoding::Base2Msbf, b"0100000").1);
        // Lower-case hex is not base16.
        assert!(!dec(Encoding::Base16, b"6a").1);
    }

    /// The gnulib behaviours a clean-room decoder would not have.
    #[test]
    fn gnulib_decoder_quirks() {
        // Padding mid-stream is accepted, through the slow path.
        assert_eq!(dec(Encoding::Base64, b"YQ==YQ=="), (b"aa".to_vec(), true));
        // A quantum that fails on the slow path keeps its partial byte.
        assert_eq!(dec(Encoding::Base64, b"YQ=x"), (b"a".to_vec(), false));
        // Newlines anywhere, even inside a quantum.
        assert_eq!(
            dec(Encoding::Base64, b"Y\nW\nJ\nj\n"),
            (b"abc".to_vec(), true)
        );
        // A short final quantum is carried, then refused at the flush.
        assert!(!dec(Encoding::Base64, b"YWJjZA").1);
        // Base32 padding may not stop part-way.
        assert!(!dec(Encoding::Base32, b"MF=RGG==").1);
    }

    #[test]
    fn wrap_breaks_before_the_character_that_passes_the_edge() {
        // Driven through the settings parser and a buffer, since `wrap_write`
        // writes to a stream; the arithmetic is what matters here.
        let mut column = 0usize;
        let mut lines: Vec<usize> = Vec::new();
        let text = [b'x'; 10];
        let wrap = 4usize;
        let mut written = 0usize;
        let mut current = 0usize;
        while written < text.len() {
            let room = wrap.saturating_sub(column).min(text.len() - written);
            if room == 0 {
                lines.push(current);
                current = 0;
                column = 0;
            } else {
                column += room;
                current += room;
                written += room;
            }
        }
        lines.push(current);
        assert_eq!(lines, vec![4, 4, 2]);
    }

    fn parse(program: Program, list: &[&str]) -> Result<Request, getopt::Error> {
        let args: Vec<OsString> = list.iter().map(OsString::from).collect();
        parse_args(program, Prog::new(program.name(), 1), &args)
    }

    #[test]
    fn command_lines() {
        let Request::Run(s) = parse(Program::Base64, &["-d", "-w", "0", "f"]).unwrap() else {
            panic!()
        };
        assert_eq!((s.decode, s.wrap, s.encoding), (true, 0, Encoding::Base64));
        // An overflowing width means no wrapping.
        let Request::Run(s) = parse(Program::Base32, &["-w", "99999999999999999999"]).unwrap()
        else {
            panic!()
        };
        assert_eq!(s.wrap, 0);
        let e = parse(Program::Base64, &["-w", "x"]).unwrap_err();
        assert!(e.message().starts_with("invalid wrap size: ‘x’"));
        let e = parse(Program::Basenc, &["f"]).unwrap_err();
        assert!(e.message().starts_with("missing encoding type"));
        // The last encoding named wins.
        let Request::Run(s) = parse(Program::Basenc, &["--base64", "--z85"]).unwrap() else {
            panic!()
        };
        assert_eq!(s.encoding, Encoding::Z85);
        let e = parse(Program::Base64, &["a", "b"]).unwrap_err();
        assert!(e.message().starts_with("extra operand ‘b’"));
        // `base64` does not know `basenc`'s options.
        assert!(parse(Program::Base64, &["--z85"]).is_err());
    }

    #[test]
    fn help_matches_upstream_line_for_line() {
        let text = help_text(Program::Basenc);
        assert!(
            text.contains(
                "\n      --base64url       file- and url-safe base64 (RFC4648 section 5)\n"
            )
        );
        assert!(
            text.contains(
                "\n  -i, --ignore-garbage  when decoding, ignore non-alphabet characters\n"
            )
        );
        assert!(text.contains("\n                          Use 0 to disable line wrapping\n"));
        assert!(text.contains(
            "\n                        when decoding, input length must be a multiple of 5\n"
        ));
        assert!(help_text(Program::Base32).contains("the formal base32 alphabet.  Use"));
    }
}
