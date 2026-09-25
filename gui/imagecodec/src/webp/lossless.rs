//! WebP lossless (`VP8L`): RFC 9649 §3.
//!
//! A picture stored exactly, alpha included, as an ARGB image that has been
//! through up to four reversible transforms and is then entropy-coded pixel by
//! pixel. Each pixel is one of three things: a literal (green, red, blue and
//! alpha, each with its own prefix code), a copy of earlier pixels (LZ77, with a
//! distance that for short hops names a nearby pixel in two dimensions), or an
//! entry of a small colour cache hashed from recent colours. The prefix codes
//! themselves may change from block to block, chosen by a smaller "entropy
//! image", and the transforms' own data -- a predictor per block, a colour
//! decorrelation per block, a palette -- are images too, coded the same way.
//!
//! Decoding reverses all of that: read the transforms' data, decode the pixels,
//! then undo the transforms last-read first. Everything is integer arithmetic
//! specified to the bit, so the result is the encoder's input exactly, and the
//! tests hold it to libwebp's decode with no tolerance at all.
//!
//! # Hostile input
//!
//! Every count in the stream is bounded before anything is sized from it: the
//! image by [`Limits`], a colour cache to 2^11 entries, prefix codes to the
//! alphabets the format defines, copies to the pixels already decoded. A
//! prefix code must describe a complete tree, as libwebp requires. And the
//! entropy image may name up to 65536 groups of five codes, whose tables are
//! the one allocation a file could inflate cheaply: each group is parsed and
//! checked, but only the groups some block actually uses are kept, and their
//! tables are counted against the byte budget.

use alloc::vec;
use alloc::vec::Vec;

use crate::{ImageError, ImageResult, Limits};

/// The first byte of every VP8L stream.
pub(super) const SIGNATURE: u8 = 0x2F;

/// Width and height from a stream's five-byte header.
pub(super) fn dimensions(data: &[u8]) -> ImageResult<(u32, u32)> {
    let header = data.get(..5).ok_or(ImageError::Truncated)?;
    if header.first() != Some(&SIGNATURE) {
        return Err(ImageError::Malformed("a VP8L stream without its signature"));
    }
    let mut bits = Bits::new(header.get(1..).unwrap_or(&[]), 0);
    let width = bits.read(14).saturating_add(1);
    let height = bits.read(14).saturating_add(1);
    let _alpha_is_used = bits.read(1);
    if bits.read(3) != 0 {
        return Err(ImageError::Malformed(
            "a VP8L stream of a version other than 0",
        ));
    }
    Ok((width, height))
}

/// Decode a whole `VP8L` chunk into `0xAARRGGBB` pixels, row by row.
pub(super) fn decode(data: &[u8], limits: Limits) -> ImageResult<(u32, u32, Vec<u32>)> {
    let (width, height) = dimensions(data)?;
    let pixels = decode_stream(
        data.get(5..).unwrap_or(&[]),
        width,
        height,
        limits,
        Stream::Image,
    )?;
    Ok((width, height, pixels))
}

/// What an image stream is, which decides -- as it does in libwebp -- exactly
/// when running out of data is an error.
///
/// libwebp's reader flags the end of a stream once more bits have been read
/// than it holds, but never within the first 64 bits it loads, which for a
/// stream shorter than eight bytes is past its end: those read as zeros and
/// are not an error. A `VP8L` picture's reader starts at its five-byte header,
/// so its stream gets the rest of those 64 bits; an `ALPH` stream's reader
/// starts at the stream.
///
/// And libwebp decodes an alpha plane that is only colour-indexed, with no
/// colour cache and only one red, blue and alpha value, a byte per pixel, by a
/// routine that judges it by whether every pixel was decoded: the last symbol
/// may read past the end. Everywhere else a stream that reads past its end is
/// truncated.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Stream {
    /// A `VP8L` picture's stream, after its header.
    Image,
    /// An `ALPH` chunk's plane, its value in the green channel.
    Alpha,
}

/// Decode an image stream with no header -- what follows a VP8L header, and
/// what a lossless `ALPH` chunk holds -- of the given size.
pub(super) fn decode_stream(
    data: &[u8],
    width: u32,
    height: u32,
    limits: Limits,
    stream: Stream,
) -> ImageResult<Vec<u32>> {
    let (width, height) = (width as usize, height as usize);
    let pixels = width.saturating_mul(height);
    if pixels as u64 > limits.max_pixels {
        return Err(ImageError::TooLarge {
            pixels: pixels as u64,
            limit: limits.max_pixels,
        });
    }
    let mut budget = Budget::new(limits);
    budget.spend(pixels.saturating_mul(4))?;
    let mut bits = Bits::new(
        data,
        match stream {
            Stream::Image => 3,
            Stream::Alpha => 8,
        },
    );

    let (transforms, coded_width) = read_transforms(&mut bits, width, height, &mut budget)?;
    let byte_per_pixel = stream == Stream::Alpha
        && matches!(transforms.as_slice(), [Transform::ColorIndexing { .. }]);
    let end = if byte_per_pixel {
        End::ForgiveLastIfBytePerPixel
    } else {
        End::Strict
    };

    let (mut image, forgiven) =
        decode_image(&mut bits, coded_width, height, Some(end), &mut budget)?;
    if bits.exhausted() && !forgiven {
        return Err(ImageError::Truncated);
    }
    for transform in transforms.iter().rev() {
        image = transform.undo(image, height);
    }
    if image.len() != pixels {
        return Err(ImageError::Malformed("a VP8L image of the wrong size"));
    }
    Ok(image)
}

/// Read the transforms ahead of the pixels (RFC 9649 §3.5), in the order they
/// were sent, and the width the pixels are then coded at: colour indexing
/// packs several pixels into one and narrows it.
fn read_transforms(
    bits: &mut Bits<'_>,
    width: usize,
    height: usize,
    budget: &mut Budget,
) -> ImageResult<(Vec<Transform>, usize)> {
    // Undone in reverse.
    let mut transforms: Vec<Transform> = Vec::new();
    let mut seen = [false; 4];
    // The width of the image the pixels are coded at: colour indexing packs
    // several pixels into one and narrows it.
    let mut coded_width = width;
    while bits.read(1) == 1 {
        let kind = bits.read(2) as usize;
        if seen.get(kind).copied().unwrap_or(true) {
            return Err(ImageError::Malformed("a VP8L transform used twice"));
        }
        if let Some(slot) = seen.get_mut(kind) {
            *slot = true;
        }
        let transform = match kind {
            0 | 1 => {
                let size_bits = bits.read(3).saturating_add(2);
                let blocks_w = div_round_up(coded_width, size_bits);
                let blocks_h = div_round_up(height, size_bits);
                let (data, _) = decode_image(bits, blocks_w, blocks_h, None, budget)?;
                let block = Blocks {
                    size_bits,
                    blocks_w,
                    data,
                };
                if kind == 0 {
                    Transform::Predictor(block, coded_width)
                } else {
                    Transform::Color(block, coded_width)
                }
            }
            2 => Transform::SubtractGreen,
            _ => {
                let size = (bits.read(8) as usize).saturating_add(1);
                let (mut table, _) = decode_image(bits, size, 1, None, budget)?;
                // Delta-coded: each entry adds to the one before it, channel
                // by channel -- a running sum from black-transparent zero.
                let mut previous = 0u32;
                for entry in &mut table {
                    *entry = add_pixels(*entry, previous);
                    previous = *entry;
                }
                // Indices past the table are transparent black, and an index
                // is a byte: pad the table to every index there is.
                table.resize(256, 0);
                let width_bits = match size {
                    1..=2 => 3,
                    3..=4 => 2,
                    5..=16 => 1,
                    _ => 0,
                };
                let before = coded_width;
                coded_width = div_round_up(coded_width, width_bits);
                Transform::ColorIndexing {
                    table,
                    width_bits,
                    width: before,
                }
            }
        };
        transforms.push(transform);
    }

    Ok((transforms, coded_width))
}

/// `ceil(size / 2^bits)`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "bits is at most 9, a transform's block size, so neither the shift nor the sum can overflow"
)]
const fn div_round_up(size: usize, bits: u32) -> usize {
    let unit = 1usize << bits;
    size.saturating_add(unit - 1) >> bits
}

// ---------------------------------------------------------------------------
// Bits
// ---------------------------------------------------------------------------

/// The stream, least significant bit of each byte first.
///
/// Reading past the end yields zero bits and remembers that it did, so the
/// decode can finish its current step cheaply and report the file truncated
/// once, rather than every read checking.
struct Bits<'a> {
    data: &'a [u8],
    /// The data's length for judging an overrun: at least `floor` bytes, the
    /// window libwebp's reader never flags (see [`Stream`]).
    judged_len: usize,
    /// The next byte to load.
    at: usize,
    /// Loaded bits, the next one lowest.
    acc: u64,
    /// How many of them.
    count: u32,
    /// Bits consumed that were not there.
    overrun: bool,
}

impl<'a> Bits<'a> {
    /// A reader over `data` that flags an overrun only past `floor` bytes if
    /// the data is shorter than that.
    fn new(data: &'a [u8], floor: usize) -> Self {
        Self {
            data,
            judged_len: data.len().max(floor),
            at: 0,
            acc: 0,
            count: 0,
            overrun: false,
        }
    }

    /// At least 32 bits loaded, from the data or, past its end, zeros.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "count stays within 0..=64 and each byte lands below bit 64"
    )]
    fn fill(&mut self) {
        while self.count <= 56 {
            let byte = self.data.get(self.at).copied();
            self.at = self.at.saturating_add(1);
            self.acc |= u64::from(byte.unwrap_or(0)) << self.count;
            self.count += 8;
        }
    }

    /// The next `n` bits (at most 32) without consuming them.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "n is below 32 where the mask is shifted"
    )]
    fn peek(&mut self, n: u32) -> u32 {
        if self.count < n {
            self.fill();
        }
        let mask = if n >= 32 { u32::MAX } else { (1u32 << n) - 1 };
        (self.acc as u32) & mask
    }

    /// Consume `n` bits already peeked.
    fn skip(&mut self, n: u32) {
        self.acc = self.acc.checked_shr(n).unwrap_or(0);
        self.count = self.count.saturating_sub(n);
        // Bytes loaded past the end are zeros standing in for data; using any
        // of them means the stream was too short.
        let loaded_past = self.at.saturating_sub(self.judged_len).saturating_mul(8);
        if (self.count as usize) < loaded_past {
            self.overrun = true;
        }
    }

    /// The next `n` bits (at most 32), consumed.
    fn read(&mut self, n: u32) -> u32 {
        let value = self.peek(n);
        self.skip(n);
        value
    }

    /// Whether any bit read so far lay past the end of the data.
    const fn exhausted(&self) -> bool {
        self.overrun
    }
}

// ---------------------------------------------------------------------------
// Prefix codes
// ---------------------------------------------------------------------------

/// Bits a code's first table is indexed by.
const ROOT_BITS: u32 = 8;

/// The longest prefix code the format allows.
const MAX_LENGTH: usize = 15;

/// One entry of a code's lookup table: a symbol and its length, or -- where
/// `length` exceeds [`ROOT_BITS`] in the first table -- a link to a
/// second-level table `length - ROOT_BITS` bits wide at offset `value`.
#[derive(Clone, Copy, Debug, Default)]
struct Entry {
    length: u8,
    value: u16,
}

/// A prefix code, as tables read least significant bit first.
struct Code {
    table: Vec<Entry>,
}

impl Code {
    /// The canonical code for `lengths` (one per symbol, 0 for unused), or
    /// `None` if it does not describe a complete tree -- except the one
    /// symbol case, which reads no bits at all.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "lengths are at most 15 and alphabets at most 2328 symbols, so every count, code and table index is far inside its type"
    )]
    fn build(lengths: &[u8]) -> Option<Self> {
        let mut count = [0u16; MAX_LENGTH + 1];
        let mut used = 0usize;
        let mut last = 0usize;
        for (symbol, &length) in lengths.iter().enumerate() {
            let length = usize::from(length);
            if length > MAX_LENGTH {
                return None;
            }
            if length > 0 {
                if let Some(c) = count.get_mut(length) {
                    *c += 1;
                }
                used += 1;
                last = symbol;
            }
        }
        match used {
            0 => return None,
            1 => {
                let entry = Entry {
                    length: 0,
                    value: u16::try_from(last).ok()?,
                };
                return Some(Self {
                    table: vec![entry; 1 << ROOT_BITS],
                });
            }
            _ => {}
        }
        // Complete: the lengths' Kraft sum is exactly one.
        let mut open = 1i64;
        for &c in count.iter().skip(1) {
            open = open * 2 - i64::from(c);
            if open < 0 {
                return None;
            }
        }
        if open != 0 {
            return None;
        }
        // The first code of each length, canonically.
        let mut next = [0u32; MAX_LENGTH + 2];
        let mut code = 0u32;
        for length in 1..=MAX_LENGTH {
            code = (code + u32::from(count.get(length - 1).copied().unwrap_or(0))) << 1;
            if let Some(slot) = next.get_mut(length) {
                *slot = code;
            }
        }
        // Each root slot's widest second-level table.
        let root_size = 1usize << ROOT_BITS;
        let mut sub_bits = vec![0u32; root_size];
        let mut codes = vec![(0u32, 0u32); lengths.len()];
        for (symbol, &length) in lengths.iter().enumerate() {
            let length = u32::from(length);
            if length == 0 {
                continue;
            }
            let slot = next.get_mut(length as usize)?;
            let reversed = reverse(*slot, length);
            *slot += 1;
            if let Some(c) = codes.get_mut(symbol) {
                *c = (reversed, length);
            }
            if length > ROOT_BITS {
                let root = (reversed as usize) & (root_size - 1);
                if let Some(bits) = sub_bits.get_mut(root) {
                    *bits = (*bits).max(length - ROOT_BITS);
                }
            }
        }
        let mut table = vec![Entry::default(); root_size];
        // Second-level tables after the first, linked from it.
        for (root, &bits) in sub_bits.iter().enumerate() {
            if bits > 0 {
                let offset = u16::try_from(table.len()).ok()?;
                if let Some(entry) = table.get_mut(root) {
                    *entry = Entry {
                        length: u8::try_from(ROOT_BITS + bits).ok()?,
                        value: offset,
                    };
                }
                table.resize(table.len() + (1usize << bits), Entry::default());
            }
        }
        for (symbol, &(reversed, length)) in codes.iter().enumerate() {
            if length == 0 {
                continue;
            }
            let value = u16::try_from(symbol).ok()?;
            if length <= ROOT_BITS {
                let step = 1usize << length;
                let mut at = reversed as usize;
                while at < root_size {
                    if let Some(entry) = table.get_mut(at) {
                        *entry = Entry {
                            length: u8::try_from(length).ok()?,
                            value,
                        };
                    }
                    at += step;
                }
            } else {
                let root = (reversed as usize) & (root_size - 1);
                let link = *table.get(root)?;
                let bits = u32::from(link.length) - ROOT_BITS;
                let base = usize::from(link.value);
                let rest = (reversed >> ROOT_BITS) as usize;
                let own = length - ROOT_BITS;
                let step = 1usize << own;
                let mut at = rest;
                while at < (1usize << bits) {
                    if let Some(entry) = table.get_mut(base + at) {
                        *entry = Entry {
                            length: u8::try_from(own).ok()?,
                            value,
                        };
                    }
                    at += step;
                }
            }
        }
        Some(Self { table })
    }

    /// The next symbol.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "table lengths are at most 15 bits and indices are masked to their tables"
    )]
    fn read(&self, bits: &mut Bits<'_>) -> u16 {
        let peeked = bits.peek(MAX_LENGTH as u32);
        let root = self
            .table
            .get((peeked as usize) & ((1 << ROOT_BITS) - 1))
            .copied()
            .unwrap_or_default();
        if u32::from(root.length) <= ROOT_BITS {
            bits.skip(u32::from(root.length));
            return root.value;
        }
        let sub_bits = u32::from(root.length) - ROOT_BITS;
        let index =
            usize::from(root.value) + ((peeked >> ROOT_BITS) as usize & ((1 << sub_bits) - 1));
        let entry = self.table.get(index).copied().unwrap_or_default();
        bits.skip(ROOT_BITS + u32::from(entry.length));
        entry.value
    }

    /// Whether the code has a single symbol, which reads no bits.
    fn single(&self) -> bool {
        self.table.first().is_some_and(|entry| entry.length == 0)
    }

    /// Bytes the tables hold, for the budget.
    fn size(&self) -> usize {
        self.table
            .len()
            .saturating_mul(core::mem::size_of::<Entry>())
    }
}

/// `code`'s low `length` bits in reverse order: a canonical code is sent most
/// significant bit first into a stream read least significant bit first.
fn reverse(code: u32, length: u32) -> u32 {
    code.reverse_bits()
        .checked_shr(32u32.saturating_sub(length))
        .unwrap_or(0)
}

/// The order code-length code lengths are sent in.
const CODE_LENGTH_ORDER: [usize; 19] = [
    17, 18, 0, 1, 2, 3, 4, 5, 16, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];

/// Read one prefix code's lengths for an alphabet of `size` symbols and
/// build it (RFC 9649 §3.7.2.1).
#[allow(
    clippy::arithmetic_side_effects,
    reason = "counts are bounded by the alphabet, at most 2328, and runs by 138"
)]
fn read_code(bits: &mut Bits<'_>, size: usize) -> ImageResult<Code> {
    let mut lengths = vec![0u8; size];
    if bits.read(1) == 1 {
        // Simple: one or two symbols, of length one.
        let two = bits.read(1) == 1;
        let first_width = if bits.read(1) == 1 { 8 } else { 1 };
        let first = bits.read(first_width) as usize;
        *lengths
            .get_mut(first)
            .ok_or(ImageError::Malformed("a VP8L symbol past its alphabet"))? = 1;
        if two {
            let second = bits.read(8) as usize;
            *lengths
                .get_mut(second)
                .ok_or(ImageError::Malformed("a VP8L symbol past its alphabet"))? = 1;
        }
    } else {
        let count = bits.read(4) as usize + 4;
        let mut code_lengths = [0u8; 19];
        for &symbol in CODE_LENGTH_ORDER.iter().take(count) {
            if let Some(slot) = code_lengths.get_mut(symbol) {
                *slot = bits.read(3) as u8;
            }
        }
        let length_code = Code::build(&code_lengths).ok_or(ImageError::Malformed(
            "a VP8L code-length code that is not a tree",
        ))?;
        let mut tokens = if bits.read(1) == 1 {
            let width = 2 + 2 * bits.read(3);
            let tokens = 2 + bits.read(width) as usize;
            if tokens > size {
                return Err(ImageError::Malformed("more VP8L code lengths than symbols"));
            }
            tokens
        } else {
            size
        };
        let mut symbol = 0usize;
        let mut previous = 8u8;
        while symbol < size && tokens > 0 {
            tokens -= 1;
            let token = length_code.read(bits);
            if token < 16 {
                let length = token as u8;
                if let Some(slot) = lengths.get_mut(symbol) {
                    *slot = length;
                }
                symbol += 1;
                if length != 0 {
                    previous = length;
                }
            } else {
                let (extra, offset, value) = match token {
                    16 => (2, 3, previous),
                    17 => (3, 3, 0),
                    _ => (7, 11, 0),
                };
                let repeat = bits.read(extra) as usize + offset;
                if symbol + repeat > size {
                    return Err(ImageError::Malformed("a VP8L length run past its alphabet"));
                }
                for slot in lengths.iter_mut().skip(symbol).take(repeat) {
                    *slot = value;
                }
                symbol += repeat;
            }
            if bits.exhausted() {
                return Err(ImageError::Truncated);
            }
        }
    }
    Code::build(&lengths).ok_or(ImageError::Malformed(
        "a VP8L prefix code that is not a tree",
    ))
}

/// The five codes one block of pixels is decoded with.
struct Group {
    green: Code,
    red: Code,
    blue: Code,
    alpha: Code,
    distance: Code,
}

/// Alphabet sizes of a group's red, blue, alpha and distance codes.
const LITERALS: usize = 256;
const LENGTH_CODES: usize = 24;
const DISTANCE_CODES: usize = 40;

fn read_group(bits: &mut Bits<'_>, cache_size: usize) -> ImageResult<Group> {
    Ok(Group {
        green: read_code(bits, (LITERALS + LENGTH_CODES).saturating_add(cache_size))?,
        red: read_code(bits, LITERALS)?,
        blue: read_code(bits, LITERALS)?,
        alpha: read_code(bits, LITERALS)?,
        distance: read_code(bits, DISTANCE_CODES)?,
    })
}

impl Group {
    /// Whether its red, blue and alpha codes each have a single symbol, and so
    /// read no bits.
    fn one_value_but_green(&self) -> bool {
        self.red.single() && self.blue.single() && self.alpha.single()
    }

    fn size(&self) -> usize {
        [
            &self.green,
            &self.red,
            &self.blue,
            &self.alpha,
            &self.distance,
        ]
        .iter()
        .map(|c| c.size())
        .sum()
    }
}

/// The byte budget, spent as the decode allocates.
struct Budget {
    left: usize,
}

impl Budget {
    const fn new(limits: Limits) -> Self {
        Self {
            left: limits.max_decompressed_bytes,
        }
    }

    fn spend(&mut self, bytes: usize) -> ImageResult<()> {
        self.left = self.left.checked_sub(bytes).ok_or(ImageError::TooLarge {
            pixels: bytes as u64,
            limit: self.left as u64,
        })?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Entropy-coded images
// ---------------------------------------------------------------------------

/// A value coded as a prefix and extra bits (RFC 9649 §3.6.2.2).
#[allow(
    clippy::arithmetic_side_effects,
    reason = "a prefix is below 40, so the extra bits are at most 18 and the value below 2^20"
)]
fn prefix_value(bits: &mut Bits<'_>, prefix: u32) -> usize {
    if prefix < 4 {
        return prefix as usize + 1;
    }
    let extra = (prefix - 2) >> 1;
    let offset = (2 + (prefix & 1)) << extra;
    (offset + bits.read(extra)) as usize + 1
}

/// The two-dimensional neighbourhood the first 120 distance codes name, as
/// `(columns left, rows up)` (RFC 9649 Figure 20).
const DISTANCE_MAP: [(i8, u8); 120] = [
    (0, 1),
    (1, 0),
    (1, 1),
    (-1, 1),
    (0, 2),
    (2, 0),
    (1, 2),
    (-1, 2),
    (2, 1),
    (-2, 1),
    (2, 2),
    (-2, 2),
    (0, 3),
    (3, 0),
    (1, 3),
    (-1, 3),
    (3, 1),
    (-3, 1),
    (2, 3),
    (-2, 3),
    (3, 2),
    (-3, 2),
    (0, 4),
    (4, 0),
    (1, 4),
    (-1, 4),
    (4, 1),
    (-4, 1),
    (3, 3),
    (-3, 3),
    (2, 4),
    (-2, 4),
    (4, 2),
    (-4, 2),
    (0, 5),
    (3, 4),
    (-3, 4),
    (4, 3),
    (-4, 3),
    (5, 0),
    (1, 5),
    (-1, 5),
    (5, 1),
    (-5, 1),
    (2, 5),
    (-2, 5),
    (5, 2),
    (-5, 2),
    (4, 4),
    (-4, 4),
    (3, 5),
    (-3, 5),
    (5, 3),
    (-5, 3),
    (0, 6),
    (6, 0),
    (1, 6),
    (-1, 6),
    (6, 1),
    (-6, 1),
    (2, 6),
    (-2, 6),
    (6, 2),
    (-6, 2),
    (4, 5),
    (-4, 5),
    (5, 4),
    (-5, 4),
    (3, 6),
    (-3, 6),
    (6, 3),
    (-6, 3),
    (0, 7),
    (7, 0),
    (1, 7),
    (-1, 7),
    (5, 5),
    (-5, 5),
    (7, 1),
    (-7, 1),
    (4, 6),
    (-4, 6),
    (6, 4),
    (-6, 4),
    (2, 7),
    (-2, 7),
    (7, 2),
    (-7, 2),
    (3, 7),
    (-3, 7),
    (7, 3),
    (-7, 3),
    (5, 6),
    (-5, 6),
    (6, 5),
    (-6, 5),
    (8, 0),
    (4, 7),
    (-4, 7),
    (7, 4),
    (-7, 4),
    (8, 1),
    (8, 2),
    (6, 6),
    (-6, 6),
    (8, 3),
    (5, 7),
    (-5, 7),
    (7, 5),
    (-7, 5),
    (8, 4),
    (6, 7),
    (-6, 7),
    (7, 6),
    (-7, 6),
    (8, 5),
    (7, 7),
    (-7, 7),
    (8, 6),
    (8, 7),
];

/// The scan-line distance a distance code names in an image `width` wide.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "a code is below 2^20 and a width below 2^14"
)]
fn distance(code: usize, width: usize) -> usize {
    if code > DISTANCE_MAP.len() {
        return code - DISTANCE_MAP.len();
    }
    let (left, up) = DISTANCE_MAP
        .get(code.wrapping_sub(1))
        .copied()
        .unwrap_or((0, 1));
    let dist = i64::from(left) + i64::from(up) * width as i64;
    usize::try_from(dist.max(1)).unwrap_or(1)
}

/// A colour cache's slot for `argb`: a multiplicative hash's top bits.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "bits is 1 to 11, checked where it is read"
)]
const fn cache_slot(argb: u32, bits: u32) -> usize {
    (argb.wrapping_mul(0x1E35_A7BD) >> (32 - bits)) as usize
}

/// How the main image's decode treats reading past the end of the stream.
#[derive(Clone, Copy, PartialEq, Eq)]
enum End {
    /// Any bit read past the end is an error.
    Strict,
    /// If the image turns out to be one libwebp decodes a byte per pixel (see
    /// [`Stream`]), bits read past the end by the symbol that completes it are
    /// forgiven.
    ForgiveLastIfBytePerPixel,
}

/// Decode one entropy-coded image of `width` x `height` pixels: the colour
/// cache info, for the main image the meta prefix codes, the prefix codes, and
/// the pixels (RFC 9649 §3.6, §3.7). `main` is the main image's end rule, and
/// `None` for the images inside transforms and meta codes. Returns the pixels,
/// and whether an overrun in the last symbol was forgiven.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "positions are bounded by the image, which Limits bounds, and each step is checked against what is left"
)]
fn decode_image(
    bits: &mut Bits<'_>,
    width: usize,
    height: usize,
    main: Option<End>,
    budget: &mut Budget,
) -> ImageResult<(Vec<u32>, bool)> {
    let total = width.saturating_mul(height);
    let cache_bits = if bits.read(1) == 1 {
        let cache_bits = bits.read(4);
        if !(1..=11).contains(&cache_bits) {
            return Err(ImageError::Malformed(
                "a VP8L colour cache of an impossible size",
            ));
        }
        Some(cache_bits)
    } else {
        None
    };
    let cache_size = cache_bits.map_or(0, |b| 1usize << b);

    // Which group each block uses: one group everywhere, or an entropy image.
    let mut entropy: Option<(u32, usize, Vec<u32>)> = None;
    let mut groups_named = 1usize;
    if main.is_some() && bits.read(1) == 1 {
        let prefix_bits = bits.read(3) + 2;
        let blocks_w = div_round_up(width, prefix_bits);
        let blocks_h = div_round_up(height, prefix_bits);
        budget.spend(blocks_w.saturating_mul(blocks_h).saturating_mul(4))?;
        let (mut image, _) = decode_image(bits, blocks_w, blocks_h, None, budget)?;
        for pixel in &mut image {
            *pixel = (*pixel >> 8) & 0xFFFF;
            groups_named = groups_named.max(*pixel as usize + 1);
        }
        entropy = Some((prefix_bits, blocks_w, image));
    }

    // Every group named is in the stream and must be read to reach the next;
    // only the ones a block uses are kept.
    let mut used = vec![false; groups_named];
    match &entropy {
        Some((_, _, image)) => {
            for &group in image {
                if let Some(slot) = used.get_mut(group as usize) {
                    *slot = true;
                }
            }
        }
        None => used.fill(true),
    }
    let mut index = vec![usize::MAX; groups_named];
    let mut groups: Vec<Group> = Vec::new();
    // Whether every group -- and every group a block uses -- has one red, one
    // blue and one alpha value: half of libwebp's byte-per-pixel test.
    let (mut single_all, mut single_used) = (true, true);
    for (n, &wanted) in used.iter().enumerate() {
        let group = read_group(bits, cache_size)?;
        single_all &= group.one_value_but_green();
        if wanted {
            single_used &= group.one_value_but_green();
            budget.spend(group.size())?;
            if let Some(slot) = index.get_mut(n) {
                *slot = groups.len();
            }
            groups.push(group);
        }
        if bits.exhausted() {
            return Err(ImageError::Truncated);
        }
    }

    // libwebp keeps every group it reads unless there are more than 1000 of
    // them or more than pixels, when it keeps only the used ones -- and its
    // byte-per-pixel test looks at the groups it kept.
    let kept_only_used = entropy.is_some() && (groups_named > 1000 || groups_named > total);
    let byte_per_pixel = main == Some(End::ForgiveLastIfBytePerPixel)
        && cache_bits.is_none()
        && if kept_only_used {
            single_used
        } else {
            single_all
        };

    budget.spend(total.saturating_mul(4))?;
    budget.spend(cache_size.saturating_mul(4))?;
    let mut out = vec![0u32; total];
    let mut cache = vec![0u32; cache_size];
    let group_at = |x: usize, y: usize| -> Option<&Group> {
        match &entropy {
            None => groups.first(),
            Some((prefix_bits, blocks_w, image)) => {
                let block = (y >> prefix_bits)
                    .saturating_mul(*blocks_w)
                    .saturating_add(x >> prefix_bits);
                let named = *image.get(block)? as usize;
                groups.get(*index.get(named)?)
            }
        }
    };
    let insert = |cache: &mut Vec<u32>, argb: u32| {
        if let Some(b) = cache_bits
            && let Some(slot) = cache.get_mut(cache_slot(argb, b))
        {
            *slot = argb;
        }
    };

    let mut at = 0usize;
    let (mut x, mut y) = (0usize, 0usize);
    while at < total {
        let group = group_at(x, y).ok_or(ImageError::Malformed("a VP8L block naming no code"))?;
        let symbol = usize::from(group.green.read(bits));
        if symbol < LITERALS {
            let red = u32::from(group.red.read(bits));
            let blue = u32::from(group.blue.read(bits));
            let alpha = u32::from(group.alpha.read(bits));
            let argb = (alpha << 24) | (red << 16) | ((symbol as u32) << 8) | blue;
            if let Some(slot) = out.get_mut(at) {
                *slot = argb;
            }
            insert(&mut cache, argb);
            at += 1;
            x += 1;
        } else if symbol < LITERALS + LENGTH_CODES {
            let length = prefix_value(bits, (symbol - LITERALS) as u32);
            let distance_prefix = u32::from(group.distance.read(bits));
            let code = prefix_value(bits, distance_prefix);
            let dist = distance(code, width);
            if dist > at || length > total - at {
                return Err(ImageError::Malformed("a VP8L copy from outside the image"));
            }
            for _ in 0..length {
                let argb = out.get(at - dist).copied().unwrap_or(0);
                if let Some(slot) = out.get_mut(at) {
                    *slot = argb;
                }
                insert(&mut cache, argb);
                at += 1;
            }
            x += length;
        } else {
            let key = symbol - LITERALS - LENGTH_CODES;
            let argb = *cache.get(key).ok_or(ImageError::Malformed(
                "a VP8L colour cache entry past the cache",
            ))?;
            if let Some(slot) = out.get_mut(at) {
                *slot = argb;
            }
            insert(&mut cache, argb);
            at += 1;
            x += 1;
        }
        if width > 0 {
            y += x / width;
            x %= width;
        }
        if bits.exhausted() && !(byte_per_pixel && at >= total) {
            return Err(ImageError::Truncated);
        }
    }
    Ok((out, byte_per_pixel))
}

// ---------------------------------------------------------------------------
// Transforms
// ---------------------------------------------------------------------------

/// A per-block transform's data: one pixel per `2^size_bits` square.
struct Blocks {
    size_bits: u32,
    blocks_w: usize,
    data: Vec<u32>,
}

impl Blocks {
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "a pixel's block is inside the block grid of the image it belongs to"
    )]
    fn at(&self, x: usize, y: usize) -> u32 {
        let block = (y >> self.size_bits) * self.blocks_w + (x >> self.size_bits);
        self.data.get(block).copied().unwrap_or(0)
    }
}

/// A transform as read, each with the width of the image it applies to.
enum Transform {
    Predictor(Blocks, usize),
    Color(Blocks, usize),
    SubtractGreen,
    ColorIndexing {
        table: Vec<u32>,
        width_bits: u32,
        width: usize,
    },
}

impl Transform {
    /// Undo this transform on `image`, which is `height` rows.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "channel arithmetic is masked to bytes, and indices stay inside the image"
    )]
    fn undo(&self, mut image: Vec<u32>, height: usize) -> Vec<u32> {
        match self {
            Self::Predictor(blocks, width) => {
                unpredict(&mut image, *width, height, blocks);
                image
            }
            Self::Color(blocks, width) => {
                for (i, pixel) in image.iter_mut().enumerate() {
                    let element = blocks.at(i % width, i / width);
                    *pixel = uncolor(*pixel, element);
                }
                image
            }
            Self::SubtractGreen => {
                for pixel in &mut image {
                    let green = (*pixel >> 8) & 0xFF;
                    let red = ((*pixel >> 16) + green) & 0xFF;
                    let blue = (*pixel + green) & 0xFF;
                    *pixel = (*pixel & 0xFF00_FF00) | (red << 16) | blue;
                }
                image
            }
            Self::ColorIndexing {
                table,
                width_bits,
                width,
            } => {
                let packed_width = div_round_up(*width, *width_bits);
                let per_byte = 1usize << width_bits;
                let bits_per = 8 >> width_bits;
                let mask = (1u32 << bits_per) - 1;
                let mut out = vec![0u32; width.saturating_mul(height)];
                for y in 0..height {
                    for x in 0..*width {
                        let packed = image
                            .get(y * packed_width + (x >> width_bits))
                            .copied()
                            .unwrap_or(0);
                        let green = (packed >> 8) & 0xFF;
                        let index = (green >> ((x & (per_byte - 1)) as u32 * bits_per)) & mask;
                        if let Some(slot) = out.get_mut(y * width + x) {
                            *slot = table.get(index as usize).copied().unwrap_or(0);
                        }
                    }
                }
                out
            }
        }
    }
}

/// Per-channel sum modulo 256.
const fn add_pixels(a: u32, b: u32) -> u32 {
    let alpha_green = (a & 0xFF00_FF00).wrapping_add(b & 0xFF00_FF00) & 0xFF00_FF00;
    let red_blue = (a & 0x00FF_00FF).wrapping_add(b & 0x00FF_00FF) & 0x00FF_00FF;
    alpha_green | red_blue
}

/// Per-channel floor of the mean.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "masked so no channel carries into the next"
)]
const fn average2(a: u32, b: u32) -> u32 {
    (((a ^ b) & 0xFEFE_FEFE) >> 1) + (a & b)
}

fn channel(pixel: u32, shift: u32) -> i32 {
    ((pixel >> shift) & 0xFF) as i32
}

fn from_channels(channels: [i32; 4]) -> u32 {
    let [a, r, g, b] = channels;
    ((a as u32 & 0xFF) << 24)
        | ((r as u32 & 0xFF) << 16)
        | ((g as u32 & 0xFF) << 8)
        | (b as u32 & 0xFF)
}

const SHIFTS: [u32; 4] = [24, 16, 8, 0];

#[allow(
    clippy::arithmetic_side_effects,
    reason = "channels are bytes widened to i32"
)]
fn select(left: u32, top: u32, top_left: u32) -> u32 {
    let (mut to_left, mut to_top) = (0i32, 0i32);
    for shift in SHIFTS {
        let estimate = channel(left, shift) + channel(top, shift) - channel(top_left, shift);
        to_left += (estimate - channel(left, shift)).abs();
        to_top += (estimate - channel(top, shift)).abs();
    }
    if to_left < to_top { left } else { top }
}

#[allow(
    clippy::arithmetic_side_effects,
    reason = "channels are bytes widened to i32"
)]
fn clamp_add_subtract_full(a: u32, b: u32, c: u32) -> u32 {
    from_channels(SHIFTS.map(|s| (channel(a, s) + channel(b, s) - channel(c, s)).clamp(0, 255)))
}

#[allow(
    clippy::arithmetic_side_effects,
    reason = "channels are bytes widened to i32"
)]
fn clamp_add_subtract_half(a: u32, b: u32) -> u32 {
    from_channels(SHIFTS.map(|s| {
        let (a, b) = (channel(a, s), channel(b, s));
        (a + (a - b) / 2).clamp(0, 255)
    }))
}

/// What mode `mode` predicts from the neighbours (RFC 9649 Table 2).
fn predict(mode: u32, left: u32, top: u32, top_right: u32, top_left: u32) -> u32 {
    match mode {
        1 => left,
        2 => top,
        3 => top_right,
        4 => top_left,
        5 => average2(average2(left, top_right), top),
        6 => average2(left, top_left),
        7 => average2(left, top),
        8 => average2(top_left, top),
        9 => average2(top, top_right),
        10 => average2(average2(left, top_left), average2(top, top_right)),
        11 => select(left, top, top_left),
        12 => clamp_add_subtract_full(left, top, top_left),
        13 => clamp_add_subtract_half(average2(left, top), top_left),
        // 0, and 14 and 15, which libwebp also treats as 0.
        _ => 0xFF00_0000,
    }
}

/// Undo the predictor transform in place, row by row, each pixel from the ones
/// already undone.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "positions are inside the image, and a pixel with a row above has i >= width"
)]
fn unpredict(image: &mut [u32], width: usize, height: usize, blocks: &Blocks) {
    let get = |image: &[u32], i: usize| image.get(i).copied().unwrap_or(0);
    for y in 0..height {
        for x in 0..width {
            let i = y * width + x;
            let prediction = if y == 0 {
                if x == 0 {
                    0xFF00_0000
                } else {
                    get(image, i - 1)
                }
            } else if x == 0 {
                get(image, i - width)
            } else {
                let mode = (blocks.at(x, y) >> 8) & 0x0F;
                // For the last column, the pixel after the one above is the
                // first of this row, which is what the format specifies.
                predict(
                    mode,
                    get(image, i - 1),
                    get(image, i - width),
                    get(image, i - width + 1),
                    get(image, i - width - 1),
                )
            };
            if let Some(pixel) = image.get_mut(i) {
                *pixel = add_pixels(*pixel, prediction);
            }
        }
    }
}

/// `(t * c) >> 5` of two signed bytes.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "the product of two signed bytes fits an i32"
)]
const fn delta(t: u32, c: u32) -> i32 {
    ((t as u8 as i8 as i32) * (c as u8 as i8 as i32)) >> 5
}

/// Undo the colour transform on one pixel with its block's element: blue
/// holds green-to-red, green holds green-to-blue, red holds red-to-blue.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "byte channels plus deltas of at most 512 fit an i32, and are masked back to bytes"
)]
const fn uncolor(pixel: u32, element: u32) -> u32 {
    let green_to_red = element & 0xFF;
    let green_to_blue = (element >> 8) & 0xFF;
    let red_to_blue = (element >> 16) & 0xFF;
    let green = (pixel >> 8) & 0xFF;
    let red = ((((pixel >> 16) & 0xFF) as i32 + delta(green_to_red, green)) & 0xFF) as u32;
    let blue = ((((pixel & 0xFF) as i32) + delta(green_to_blue, green) + delta(red_to_blue, red))
        & 0xFF) as u32;
    (pixel & 0xFF00_FF00) | (red << 16) | blue
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use super::*;

    // -------------------------------------------------------------------
    // The fixtures use every tool the format has
    // -------------------------------------------------------------------

    /// The `VP8L` payload of a WebP file.
    fn payload(file: &[u8]) -> &[u8] {
        let at = file.windows(4).position(|w| w == b"VP8L").unwrap();
        let size = u32::from_le_bytes(file[at + 4..at + 8].try_into().unwrap()) as usize;
        &file[at + 8..at + 8 + size]
    }

    /// Which tools a stream's main image uses: its transforms (with colour
    /// indexing's bundling width), a colour cache, and meta prefix codes.
    fn tools(file: &[u8]) -> Vec<&'static str> {
        let data = payload(file);
        let (width, height) = dimensions(data).unwrap();
        let mut bits = Bits::new(&data[5..], 3);
        let mut budget = Budget::new(Limits::default());
        let (transforms, _) =
            read_transforms(&mut bits, width as usize, height as usize, &mut budget).unwrap();
        let mut used: Vec<&'static str> = transforms
            .iter()
            .map(|t| match t {
                Transform::Predictor(..) => "predictor",
                Transform::Color(..) => "colour",
                Transform::SubtractGreen => "subtract-green",
                Transform::ColorIndexing { width_bits, .. } => match width_bits {
                    0 => "indexing",
                    1 => "indexing, two a byte",
                    2 => "indexing, four a byte",
                    _ => "indexing, eight a byte",
                },
            })
            .collect();
        if bits.read(1) == 1 {
            used.push("colour cache");
            bits.read(4);
        }
        if bits.read(1) == 1 {
            used.push("meta prefix codes");
        }
        used
    }

    #[test]
    fn the_fixtures_between_them_use_every_tool_the_format_has() {
        // A decoder is only tested against the paths its fixtures take. The
        // fixtures are libwebp's choices, not this test's, so this pins that
        // they still cover everything: a regenerated fixture set that stopped
        // using a transform would otherwise pass while testing less.
        let files: [&[u8]; 8] = [
            include_bytes!("../../tests/data/webp_lossless_photo.webp"),
            include_bytes!("../../tests/data/webp_lossless_fast.webp"),
            include_bytes!("../../tests/data/webp_lossless_big.webp"),
            include_bytes!("../../tests/data/webp_lossless_alpha.webp"),
            include_bytes!("../../tests/data/webp_lossless_2c.webp"),
            include_bytes!("../../tests/data/webp_lossless_4c.webp"),
            include_bytes!("../../tests/data/webp_lossless_13c.webp"),
            include_bytes!("../../tests/data/webp_lossless_100c.webp"),
        ];
        let mut all: Vec<&str> = files.iter().flat_map(|f| tools(f)).collect();
        all.sort_unstable();
        all.dedup();
        for tool in [
            "predictor",
            "colour",
            "subtract-green",
            "indexing",
            "indexing, two a byte",
            "indexing, four a byte",
            "indexing, eight a byte",
            "colour cache",
            "meta prefix codes",
        ] {
            assert!(all.contains(&tool), "no fixture uses {tool}: {all:?}");
        }
    }

    // -------------------------------------------------------------------
    // Prefix codes
    // -------------------------------------------------------------------

    /// Bits, least significant first, as a stream.
    fn stream(bits: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; bits.len().div_ceil(8) + 8];
        for (i, &b) in bits.iter().enumerate() {
            out[i / 8] |= b << (i % 8);
        }
        out
    }

    #[test]
    fn a_canonical_code_reads_its_symbols_most_significant_bit_first() {
        // Lengths 1, 2, 3, 3: codes 0, 10, 110, 111 -- sent most significant
        // bit first into a least-significant-first stream.
        let code = Code::build(&[1, 2, 3, 3]).unwrap();
        let data = stream(&[0, 1, 0, 1, 1, 0, 1, 1, 1]);
        let mut bits = Bits::new(&data, 0);
        let read: Vec<u16> = (0..4).map(|_| code.read(&mut bits)).collect();
        assert_eq!(read, vec![0, 1, 2, 3]);
    }

    #[test]
    fn codes_longer_than_the_first_table_go_through_the_second() {
        // Fifteen-bit codes: symbol i < 15 of length i + 1, and one more of
        // length 15 to complete the tree.
        let mut lengths: Vec<u8> = (1..=15).collect();
        lengths.push(15);
        let code = Code::build(&lengths).unwrap();
        // Symbol 14 is fourteen 1s then a 0; symbol 15 is fifteen 1s.
        let mut sent = vec![1u8; 14];
        sent.push(0);
        sent.extend([1u8; 15]);
        let data = stream(&sent);
        let mut bits = Bits::new(&data, 0);
        assert_eq!(code.read(&mut bits), 14);
        assert_eq!(code.read(&mut bits), 15);
    }

    #[test]
    fn a_single_symbol_reads_no_bits_and_an_incomplete_tree_is_refused() {
        let code = Code::build(&[0, 0, 5, 0]).unwrap();
        let data = stream(&[1, 1, 1]);
        let mut bits = Bits::new(&data, 0);
        assert_eq!(code.read(&mut bits), 2);
        assert_eq!(bits.read(3), 0b111, "nothing was consumed");
        assert!(Code::build(&[1, 2, 3]).is_none(), "a missing 111");
        assert!(Code::build(&[1, 1, 1]).is_none(), "over-subscribed");
        assert!(Code::build(&[0, 0, 0]).is_none(), "no symbols at all");
    }

    // -------------------------------------------------------------------
    // The arithmetic
    // -------------------------------------------------------------------

    #[test]
    fn the_distance_map_counts_left_and_up() {
        // Code 1 is the pixel above; code 2 the one to the left; code 3 up
        // and to the left; code 4 up and to the right (RFC 9649 §3.6.2.2.1).
        assert_eq!(distance(1, 10), 10);
        assert_eq!(distance(2, 10), 1);
        assert_eq!(distance(3, 10), 11);
        assert_eq!(distance(4, 10), 9);
        // Past the map, the code less 120 is the distance itself.
        assert_eq!(distance(121, 10), 1);
        assert_eq!(distance(500, 10), 380);
        // A two-dimensional offset that lands before the start is one.
        assert_eq!(distance(4, 1), 1);
    }

    #[test]
    fn channels_add_and_average_without_carrying_into_each_other() {
        assert_eq!(add_pixels(0xFF01_80FF, 0x0101_8001), 0x0002_0000);
        assert_eq!(average2(0xFF00_FF01, 0x0100_0103), 0x8000_8002);
    }

    #[test]
    fn select_picks_the_neighbour_nearer_the_gradient() {
        // Estimate = L + T - TL. Here it equals T's channels exactly.
        let (left, top, top_left) = (0xFF10_1010, 0xFF80_8080, 0xFF10_1010);
        assert_eq!(select(left, top, top_left), top);
        // And a tie goes to the top pixel, as libwebp's does.
        assert_eq!(select(0xFF00_0000, 0xFF00_0000, 0xFF00_0000), 0xFF00_0000);
        let tie = select(0xFF20_0000, 0xFF00_2000, 0xFF10_1000);
        assert_eq!(tie, 0xFF00_2000);
    }

    #[test]
    fn the_colour_transform_undoes_red_first_and_blue_from_the_new_red() {
        // green_to_red = 32 (1.0 in 3.5 fixed point), red_to_blue = 32:
        // red += green, then blue += new red.
        let element = (32 << 16) | 32;
        let pixel = 0xFF_10_20_05; // red 0x10, green 0x20, blue 0x05
        let undone = uncolor(pixel, element);
        assert_eq!((undone >> 16) & 0xFF, 0x30, "red plus green");
        assert_eq!(undone & 0xFF, 0x35, "blue plus the new red");
        assert_eq!(
            undone & 0xFF00_FF00,
            pixel & 0xFF00_FF00,
            "alpha, green kept"
        );
    }

    #[test]
    fn predicted_edges_follow_the_format_whatever_the_block_says() {
        // Every block says mode 1 (L). The first pixel predicts black, the
        // first row from the left, the first column from above, and the rest
        // by their mode -- here all residuals are zero but the corner's.
        let blocks = Blocks {
            size_bits: 2,
            blocks_w: 1,
            data: vec![0xFF00_0100],
        };
        let mut image = vec![0x0012_3456, 0, 0, 0, 0, 0];
        unpredict(&mut image, 3, 2, &blocks);
        assert_eq!(image, vec![0xFF12_3456; 6]);
    }
}
