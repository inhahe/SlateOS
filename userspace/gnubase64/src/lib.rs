//! gnulib's `lib/base64.c`: the base64 encoder, and the decoder that carries
//! a partial group from one call to the next.
//!
//! Two GNU packages in this tree build on it: coreutils (`base64`, `basenc`,
//! and `cksum`'s base64 digests -- `coreutils::basenc`) and sharutils
//! (`uuencode -m`, `uudecode`'s `begin-base64` bodies and `-encoded` names).
//! They ship gnulib copies eight years apart, 2015's and 2023's, and the two
//! decoders are the same algorithm line for line -- only the integer types
//! changed -- so this is one transcription for both.
//!
//! # Why a transcription
//!
//! What a decoder emits for *invalid* input, and which input it calls invalid,
//! are not properties of base64; they are properties of this code, and both
//! packages print them:
//!
//! * the **fast path** decodes four characters at a time straight from the
//!   input; when a group fails there it *rewinds* what it wrote and retries
//!   the group on the slow path;
//! * the **slow path** gathers four non-newline characters into the context --
//!   which is how a group survives a newline or a call boundary -- and a group
//!   that fails *there* keeps its partial bytes;
//! * padding is judged against the length *remaining*, so `Zg==` is fine at
//!   the end and refused by the fast path in mid-line -- where the slow path,
//!   seeing exactly four, then accepts it;
//! * with fewer than four characters left and no flush requested, the slow
//!   path parks them in the context and reports success: a bad byte after the
//!   last whole group fails the *next* call, not this one.
//!
//! Without a context (gnulib's `base64_decode`, which uudecode uses for
//! `-encoded` names) newlines are garbage and nothing is carried.

/// `b64c`: the alphabet.
pub const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `c - from + to`, for a `c` the caller's match arm has bounded to `from..`,
/// so the result is exact and the wrapping only tells the lint so.
fn shift(c: u8, from: u8, to: u8) -> u8 {
    c.wrapping_sub(from).wrapping_add(to)
}

/// `b64[]`: a character's six-bit value, or `None` outside the alphabet
/// (`=` included).
#[must_use]
pub fn value(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(shift(c, b'A', 0)),
        b'a'..=b'z' => Some(shift(c, b'a', 26)),
        b'0'..=b'9' => Some(shift(c, b'0', 52)),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// `isbase64`: `c` is in the alphabet (padding excluded).
#[must_use]
pub fn is_base64(c: u8) -> bool {
    value(c).is_some()
}

/// A character's value, for one already checked with [`is_base64`].
fn v64(c: u8) -> u32 {
    u32::from(value(c).unwrap_or(0))
}

/// `base64_encode` with room for the whole result: each group of three bytes
/// as four characters, a short last group padded with `=`. (Given less room,
/// gnulib truncates; no caller here does that.)
#[must_use]
pub fn encode(input: &[u8]) -> Vec<u8> {
    let sym = |v: u32| {
        ALPHABET
            .get(usize::try_from(v & 0x3f).unwrap_or(0))
            .copied()
            .unwrap_or(b'A')
    };
    let mut out = Vec::with_capacity(input.len().div_ceil(3).saturating_mul(4));
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

/// `struct base64_decode_context`: up to one group, gathered past newlines.
#[derive(Clone, Debug, Default)]
pub struct Ctx {
    i: usize,
    buf: [u8; 4],
}

impl Ctx {
    /// `ctx->i`: how many characters of a group are carried.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.i
    }
}

/// The output block and the room left in it (`out`, `*outlen`): bytes past
/// the room are dropped, exactly as gnulib's `*outleft` drops them.
pub struct Out<'a> {
    /// Where the decoded bytes go.
    pub buf: &'a mut Vec<u8>,
    /// How many more may be written.
    pub left: usize,
}

impl Out<'_> {
    /// One byte, if there is room; a char-sized store truncates, as the C
    /// assignment does.
    pub fn push(&mut self, byte: u32) {
        if self.left > 0 {
            self.buf.push(byte.to_le_bytes()[0]);
            self.left = self.left.saturating_sub(1);
        }
    }

    /// Take back the last `n` bytes written.
    fn rewind(&mut self, n: usize) {
        let keep = self.buf.len().saturating_sub(n);
        self.buf.truncate(keep);
        self.left = self.left.saturating_add(n);
    }
}

/// `decode_4`. `input` is everything from the group to the end of what the
/// caller has, which is how the length tests tell "the last group" from "a
/// group in the middle". A failure keeps what it wrote.
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

/// `get_4`: the next group, as a copy -- straight from the input when it is
/// four newline-free bytes and nothing is carried, else gathered into the
/// context past newlines. Returns the characters and how many there are.
fn get_4(ctx: &mut Ctx, input: &[u8], pos: &mut usize, end: usize) -> ([u8; 4], usize) {
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

/// `base64_decode_ctx`: decode `input` onto `out`. With a context, newlines
/// are skipped, a partial group is carried, and an empty `input` asks for
/// what is carried to be flushed; without one (`base64_decode`), newlines are
/// invalid and nothing is carried. True when the input was valid.
pub fn decode_ctx(mut ctx: Option<&mut Ctx>, input: &[u8], out: &mut Out<'_>) -> bool {
    let ignore_newlines = ctx.is_some();
    let flush_ctx = ignore_newlines && input.is_empty();
    // A snapshot: the fast path runs, or does not, for the whole call.
    let ctx_i = ctx.as_deref().map_or(0, |c| c.i);
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
        if inlen > 0 && input.get(pos) == Some(&b'\n') && ignore_newlines {
            pos = pos.saturating_add(1);
            inlen = inlen.saturating_sub(1);
            continue;
        }
        // Rewind whatever a failed fast-path group wrote.
        out.rewind(left_save.saturating_sub(out.left));
        let end = pos.saturating_add(inlen);
        let ok = if let Some(c) = ctx.as_deref_mut() {
            let (quad, n) = get_4(c, input, &mut pos, end);
            inlen = n;
            if inlen == 0 || (inlen < 4 && !flush_ctx) {
                inlen = 0;
                break;
            }
            decode_4(quad.get(..inlen).unwrap_or_default(), out)
        } else {
            // The same group the fast path just refused: refused again,
            // keeping what it wrote this time.
            decode_4(input.get(pos..).unwrap_or_default(), out)
        };
        if !ok {
            break;
        }
        inlen = end.saturating_sub(pos);
    }
    inlen == 0
}

#[cfg(test)]
mod tests;
