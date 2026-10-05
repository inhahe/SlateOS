//! libmagic's shared definitions -- `file.h` and `magic.h` -- and the state a
//! lookup carries (`struct magic_set`).
//!
//! The layout of one magic rule ([`Magic`]) follows `struct magic` field for
//! field, the unions included: a rule's value is 128 raw bytes read as a
//! number, a float or a string as its type says, and a string rule's range and
//! flags share eight bytes with a number rule's mask. Behaviour libmagic gets
//! from that sharing -- a numeric rule's mask zeroed by the line that resets a
//! string's flags -- is kept by keeping the sharing.

use crate::cstd::cstr;

// ---- the rule types (`FILE_*`) ----------------------------------------------

pub const FILE_INVALID: u8 = 0;
pub const FILE_BYTE: u8 = 1;
pub const FILE_SHORT: u8 = 2;
pub const FILE_DEFAULT: u8 = 3;
pub const FILE_LONG: u8 = 4;
pub const FILE_STRING: u8 = 5;
pub const FILE_DATE: u8 = 6;
pub const FILE_BESHORT: u8 = 7;
pub const FILE_BELONG: u8 = 8;
pub const FILE_BEDATE: u8 = 9;
pub const FILE_LESHORT: u8 = 10;
pub const FILE_LELONG: u8 = 11;
pub const FILE_LEDATE: u8 = 12;
pub const FILE_PSTRING: u8 = 13;
pub const FILE_LDATE: u8 = 14;
pub const FILE_BELDATE: u8 = 15;
pub const FILE_LELDATE: u8 = 16;
pub const FILE_REGEX: u8 = 17;
pub const FILE_BESTRING16: u8 = 18;
pub const FILE_LESTRING16: u8 = 19;
pub const FILE_SEARCH: u8 = 20;
pub const FILE_MEDATE: u8 = 21;
pub const FILE_MELDATE: u8 = 22;
pub const FILE_MELONG: u8 = 23;
pub const FILE_QUAD: u8 = 24;
pub const FILE_LEQUAD: u8 = 25;
pub const FILE_BEQUAD: u8 = 26;
pub const FILE_QDATE: u8 = 27;
pub const FILE_LEQDATE: u8 = 28;
pub const FILE_BEQDATE: u8 = 29;
pub const FILE_QLDATE: u8 = 30;
pub const FILE_LEQLDATE: u8 = 31;
pub const FILE_BEQLDATE: u8 = 32;
pub const FILE_FLOAT: u8 = 33;
pub const FILE_BEFLOAT: u8 = 34;
pub const FILE_LEFLOAT: u8 = 35;
pub const FILE_DOUBLE: u8 = 36;
pub const FILE_BEDOUBLE: u8 = 37;
pub const FILE_LEDOUBLE: u8 = 38;
pub const FILE_BEID3: u8 = 39;
pub const FILE_LEID3: u8 = 40;
pub const FILE_INDIRECT: u8 = 41;
pub const FILE_QWDATE: u8 = 42;
pub const FILE_LEQWDATE: u8 = 43;
pub const FILE_BEQWDATE: u8 = 44;
pub const FILE_NAME: u8 = 45;
pub const FILE_USE: u8 = 46;
pub const FILE_CLEAR: u8 = 47;
pub const FILE_DER: u8 = 48;
pub const FILE_GUID: u8 = 49;
pub const FILE_OFFSET: u8 = 50;
pub const FILE_BEVARINT: u8 = 51;
pub const FILE_LEVARINT: u8 = 52;
pub const FILE_MSDOSDATE: u8 = 53;
pub const FILE_LEMSDOSDATE: u8 = 54;
pub const FILE_BEMSDOSDATE: u8 = 55;
pub const FILE_MSDOSTIME: u8 = 56;
pub const FILE_LEMSDOSTIME: u8 = 57;
pub const FILE_BEMSDOSTIME: u8 = 58;
pub const FILE_OCTAL: u8 = 59;
pub const FILE_NAMES_SIZE: usize = 60;

/// `IS_STRING`: the types whose value is text rather than a number.
#[must_use]
pub fn is_string(t: u8) -> bool {
    matches!(
        t,
        FILE_STRING
            | FILE_PSTRING
            | FILE_BESTRING16
            | FILE_LESTRING16
            | FILE_REGEX
            | FILE_SEARCH
            | FILE_INDIRECT
            | FILE_NAME
            | FILE_USE
            | FILE_OCTAL
    )
}

// ---- formats a description may use (`FILE_FMT_*`) ---------------------------

pub const FILE_FMT_NONE: u8 = 0;
pub const FILE_FMT_NUM: u8 = 1;
pub const FILE_FMT_STR: u8 = 2;
pub const FILE_FMT_QUAD: u8 = 3;
pub const FILE_FMT_FLOAT: u8 = 4;
pub const FILE_FMT_DOUBLE: u8 = 5;

// ---- `struct magic`'s `flag` --------------------------------------------------

/// `(...)` appears: an indirect offset.
pub const INDIR: u8 = 0x01;
/// `>&`: relative to the end of the last match.
pub const OFFADD: u8 = 0x02;
/// `>&(`: the indirect offset is itself relative.
pub const INDIROFFADD: u8 = 0x04;
/// The comparison is unsigned.
pub const UNSIGNED: u8 = 0x08;
/// No space before this rule's description.
pub const NOSPACE: u8 = 0x10;
/// A test for binary data (top-level rules only).
pub const BINTEST: u8 = 0x20;
/// A test for text.
pub const TEXTTEST: u8 = 0x40;
/// The offset is from the end of the file.
pub const OFFNEGATIVE: u8 = 0x80;

// ---- strength factors -----------------------------------------------------------

pub const FILE_FACTOR_OP_PLUS: u8 = b'+';
pub const FILE_FACTOR_OP_MINUS: u8 = b'-';
pub const FILE_FACTOR_OP_TIMES: u8 = b'*';
pub const FILE_FACTOR_OP_DIV: u8 = b'/';
pub const FILE_FACTOR_OP_NONE: u8 = 0;

// ---- operators, for a mask and for an indirection ----------------------------------

pub const FILE_OPAND: u8 = 0;
pub const FILE_OPOR: u8 = 1;
pub const FILE_OPXOR: u8 = 2;
pub const FILE_OPADD: u8 = 3;
pub const FILE_OPMINUS: u8 = 4;
pub const FILE_OPMULTIPLY: u8 = 5;
pub const FILE_OPDIVIDE: u8 = 6;
pub const FILE_OPMODULO: u8 = 7;
pub const FILE_OPS_MASK: u8 = 0x07;
pub const FILE_OPSIGNED: u8 = 0x20;
pub const FILE_OPINVERSE: u8 = 0x40;
pub const FILE_OPINDIRECT: u8 = 0x80;

// ---- a string rule's flags ----------------------------------------------------------

pub const STRING_COMPACT_WHITESPACE: u32 = 1 << 0;
pub const STRING_COMPACT_OPTIONAL_WHITESPACE: u32 = 1 << 1;
pub const STRING_IGNORE_LOWERCASE: u32 = 1 << 2;
pub const STRING_IGNORE_UPPERCASE: u32 = 1 << 3;
pub const REGEX_OFFSET_START: u32 = 1 << 4;
pub const STRING_TEXTTEST: u32 = 1 << 5;
pub const STRING_BINTEST: u32 = 1 << 6;
pub const PSTRING_1_LE: u32 = 1 << 7;
pub const PSTRING_2_BE: u32 = 1 << 8;
pub const PSTRING_2_LE: u32 = 1 << 9;
pub const PSTRING_4_BE: u32 = 1 << 10;
pub const PSTRING_4_LE: u32 = 1 << 11;
pub const REGEX_LINE_COUNT: u32 = 1 << 11;
pub const PSTRING_LEN: u32 = PSTRING_1_LE | PSTRING_2_LE | PSTRING_2_BE | PSTRING_4_LE | PSTRING_4_BE;
pub const PSTRING_LENGTH_INCLUDES_ITSELF: u32 = 1 << 12;
pub const STRING_TRIM: u32 = 1 << 13;
pub const STRING_FULL_WORD: u32 = 1 << 14;
pub const STRING_IGNORE_CASE: u32 = STRING_IGNORE_LOWERCASE | STRING_IGNORE_UPPERCASE;
pub const STRING_DEFAULT_RANGE: u32 = 100;
pub const INDIRECT_RELATIVE: u32 = 1 << 0;

// ---- limits -------------------------------------------------------------------------

/// The longest description, NUL included.
pub const MAXDESC: usize = 64;
/// The longest MIME type, NUL included.
pub const MAXMIME: usize = 80;
/// The size of a value: the longest string a rule can hold, NUL included.
pub const MAXSTRING: usize = 128;

/// `FILE_REGEX_MAX`. The other limits are with [`crate::funcs::Ms`].
pub const FILE_REGEX_MAX: usize = 8192;

// ---- `magic.h`'s flags -------------------------------------------------------------

pub const MAGIC_DEBUG: u32 = 0x000_0001;
pub const MAGIC_SYMLINK: u32 = 0x000_0002;
pub const MAGIC_COMPRESS: u32 = 0x000_0004;
pub const MAGIC_DEVICES: u32 = 0x000_0008;
pub const MAGIC_MIME_TYPE: u32 = 0x000_0010;
pub const MAGIC_CONTINUE: u32 = 0x000_0020;
pub const MAGIC_CHECK: u32 = 0x000_0040;
pub const MAGIC_PRESERVE_ATIME: u32 = 0x000_0080;
pub const MAGIC_RAW: u32 = 0x000_0100;
pub const MAGIC_ERROR: u32 = 0x000_0200;
pub const MAGIC_MIME_ENCODING: u32 = 0x000_0400;
pub const MAGIC_MIME: u32 = MAGIC_MIME_TYPE | MAGIC_MIME_ENCODING;
pub const MAGIC_APPLE: u32 = 0x000_0800;
pub const MAGIC_EXTENSION: u32 = 0x100_0000;
pub const MAGIC_COMPRESS_TRANSP: u32 = 0x200_0000;
pub const MAGIC_NO_COMPRESS_FORK: u32 = 0x400_0000;
pub const MAGIC_NODESC: u32 = MAGIC_EXTENSION | MAGIC_MIME | MAGIC_APPLE;

pub const MAGIC_NO_CHECK_COMPRESS: u32 = 0x000_1000;
pub const MAGIC_NO_CHECK_TAR: u32 = 0x000_2000;
pub const MAGIC_NO_CHECK_SOFT: u32 = 0x000_4000;
pub const MAGIC_NO_CHECK_APPTYPE: u32 = 0x000_8000;
pub const MAGIC_NO_CHECK_ELF: u32 = 0x001_0000;
pub const MAGIC_NO_CHECK_TEXT: u32 = 0x002_0000;
pub const MAGIC_NO_CHECK_CDF: u32 = 0x004_0000;
pub const MAGIC_NO_CHECK_CSV: u32 = 0x008_0000;
pub const MAGIC_NO_CHECK_TOKENS: u32 = 0x010_0000;
pub const MAGIC_NO_CHECK_ENCODING: u32 = 0x020_0000;
pub const MAGIC_NO_CHECK_JSON: u32 = 0x040_0000;
pub const MAGIC_NO_CHECK_SIMH: u32 = 0x080_0000;
pub const MAGIC_NO_CHECK_ASCII: u32 = MAGIC_NO_CHECK_TEXT;

// ---- the value union (`union VALUETYPE`) -----------------------------------------

/// 128 bytes read as whichever member the rule's type wants. The numeric
/// members are host order, and the host is little-endian -- x86-64, where
/// SlateOS runs and where libmagic is measured.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Value(pub [u8; MAXSTRING]);

impl Default for Value {
    fn default() -> Value {
        Value([0; MAXSTRING])
    }
}

impl core::fmt::Debug for Value {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Value").field(&self.q()).finish()
    }
}

impl Value {
    fn le<const N: usize>(&self) -> [u8; N] {
        let mut out = [0u8; N];
        if let Some(src) = self.0.get(..N) {
            out.copy_from_slice(src);
        }
        out
    }

    /// `.b`
    #[must_use]
    pub fn b(&self) -> u8 {
        self.0[0]
    }
    /// `.h`
    #[must_use]
    pub fn h(&self) -> u16 {
        u16::from_le_bytes(self.le())
    }
    /// `.l`
    #[must_use]
    pub fn l(&self) -> u32 {
        u32::from_le_bytes(self.le())
    }
    /// `.q`
    #[must_use]
    pub fn q(&self) -> u64 {
        u64::from_le_bytes(self.le())
    }
    /// `.f`
    #[must_use]
    pub fn f(&self) -> f32 {
        f32::from_bits(self.l())
    }
    /// `.d`
    #[must_use]
    pub fn d(&self) -> f64 {
        f64::from_bits(self.q())
    }
    pub fn set_b(&mut self, v: u8) {
        self.0[0] = v;
    }
    pub fn set_h(&mut self, v: u16) {
        self.put(&v.to_le_bytes());
    }
    pub fn set_l(&mut self, v: u32) {
        self.put(&v.to_le_bytes());
    }
    pub fn set_q(&mut self, v: u64) {
        self.put(&v.to_le_bytes());
    }
    pub fn set_f(&mut self, v: f32) {
        self.set_l(v.to_bits());
    }
    pub fn set_d(&mut self, v: f64) {
        self.set_q(v.to_bits());
    }
    fn put(&mut self, bytes: &[u8]) {
        if let Some(dst) = self.0.get_mut(..bytes.len()) {
            dst.copy_from_slice(bytes);
        }
    }
    /// `.s` as a C string: up to its first NUL.
    #[must_use]
    pub fn s(&self) -> &[u8] {
        cstr(&self.0)
    }
}

// ---- one rule (`struct magic`) -------------------------------------------------------

/// One line of the magic database.
///
/// Laid out exactly as a compiled database's 376-byte record (`struct
/// magic`, little-endian) -- the assertions below hold it to that -- so that
/// the database a program carries can be used where it lies, as upstream uses
/// a mapped `magic.mgc` (see `apprentice::map_builtin`). Every field is an
/// integer or a byte array: any bytes are a valid `Magic`.
#[derive(Clone, Debug)]
#[repr(C)]
pub struct Magic {
    /// The number of `>` before the offset.
    pub cont_level: u16,
    pub flag: u8,
    pub factor: u8,
    /// The relation: `=`, `!`, `<`, `>`, `&`, `^` or `x`.
    pub reln: u8,
    pub vallen: u8,
    pub typ: u8,
    pub in_type: u8,
    pub in_op: u8,
    pub mask_op: u8,
    /// `cond`: the conditional upstream compiles out, always 0.
    cond: u8,
    pub factor_op: u8,
    pub offset: i32,
    pub in_offset: i32,
    pub lineno: u32,
    /// The union of `num_mask` and the pair `str_range`, `str_flags` -- the
    /// low and the high half of it.
    u: u64,
    pub value: Value,
    pub desc: [u8; MAXDESC],
    pub mimetype: [u8; MAXMIME],
    pub apple: [u8; 8],
    pub ext: [u8; 64],
}

impl Default for Magic {
    fn default() -> Magic {
        Magic {
            cont_level: 0,
            flag: 0,
            factor: 0,
            reln: 0,
            vallen: 0,
            typ: 0,
            in_type: 0,
            in_op: 0,
            mask_op: 0,
            cond: 0,
            factor_op: FILE_FACTOR_OP_NONE,
            offset: 0,
            in_offset: 0,
            lineno: 0,
            u: 0,
            value: Value::default(),
            desc: [0; MAXDESC],
            mimetype: [0; MAXMIME],
            apple: [0; 8],
            ext: [0; 64],
        }
    }
}

impl Magic {
    /// `num_mask`
    #[must_use]
    pub fn num_mask(&self) -> u64 {
        self.u
    }
    pub fn set_num_mask(&mut self, v: u64) {
        self.u = v;
    }
    /// `str_range`: the low half of the union.
    #[must_use]
    pub fn str_range(&self) -> u32 {
        #[allow(clippy::cast_possible_truncation)]
        let r = self.u as u32;
        r
    }
    pub fn set_str_range(&mut self, v: u32) {
        self.u = (self.u & !0xffff_ffff) | u64::from(v);
    }
    /// `str_flags`: the high half.
    #[must_use]
    pub fn str_flags(&self) -> u32 {
        #[allow(clippy::cast_possible_truncation)]
        let f = (self.u >> 32) as u32;
        f
    }
    pub fn set_str_flags(&mut self, v: u32) {
        self.u = (self.u & 0xffff_ffff) | (u64::from(v) << 32);
    }
    /// The description, as a C string.
    #[must_use]
    pub fn desc(&self) -> &[u8] {
        cstr(&self.desc)
    }
    /// The MIME type, as a C string.
    #[must_use]
    pub fn mimetype(&self) -> &[u8] {
        cstr(&self.mimetype)
    }
    /// The extensions, as a C string.
    #[cfg(test)]
    #[must_use]
    pub fn ext(&self) -> &[u8] {
        cstr(&self.ext)
    }

    /// The rule as `struct magic` lays it out in a compiled `.mgc` file, in
    /// little-endian order: 376 bytes, `FILE_MAGICSIZE`. Byte 10 is the
    /// conditional upstream compiles out, always zero.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; FILE_MAGICSIZE] {
        let mut b = [0u8; FILE_MAGICSIZE];
        b[0..2].copy_from_slice(&self.cont_level.to_le_bytes());
        b[2] = self.flag;
        b[3] = self.factor;
        b[4] = self.reln;
        b[5] = self.vallen;
        b[6] = self.typ;
        b[7] = self.in_type;
        b[8] = self.in_op;
        b[9] = self.mask_op;
        b[10] = self.cond;
        b[11] = self.factor_op;
        b[12..16].copy_from_slice(&self.offset.to_le_bytes());
        b[16..20].copy_from_slice(&self.in_offset.to_le_bytes());
        b[20..24].copy_from_slice(&self.lineno.to_le_bytes());
        b[24..32].copy_from_slice(&self.u.to_le_bytes());
        b[32..160].copy_from_slice(&self.value.0);
        b[160..224].copy_from_slice(&self.desc);
        b[224..304].copy_from_slice(&self.mimetype);
        b[304..312].copy_from_slice(&self.apple);
        b[312..376].copy_from_slice(&self.ext);
        b
    }

    /// A rule from a compiled file's 376 bytes, little-endian -- or, with
    /// `swap`, written on a machine of the other byte order, swapped as
    /// upstream's `bs1` swaps it.
    #[must_use]
    pub fn from_bytes(b: &[u8; FILE_MAGICSIZE], swap: bool) -> Magic {
        let u16le = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        let u32le = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        let mut m = Magic {
            cont_level: u16le(0),
            flag: b[2],
            factor: b[3],
            reln: b[4],
            vallen: b[5],
            typ: b[6],
            in_type: b[7],
            in_op: b[8],
            mask_op: b[9],
            cond: b[10],
            factor_op: b[11],
            offset: i32::from_le_bytes([b[12], b[13], b[14], b[15]]),
            in_offset: i32::from_le_bytes([b[16], b[17], b[18], b[19]]),
            lineno: u32le(20),
            u: u64::from(u32le(24)) | (u64::from(u32le(28)) << 32),
            ..Magic::default()
        };
        m.value.0.copy_from_slice(&b[32..160]);
        m.desc.copy_from_slice(&b[160..224]);
        m.mimetype.copy_from_slice(&b[224..304]);
        m.apple.copy_from_slice(&b[304..312]);
        m.ext.copy_from_slice(&b[312..376]);
        if swap {
            m.cont_level = m.cont_level.swap_bytes();
            m.offset = m.offset.swap_bytes();
            m.in_offset = m.in_offset.swap_bytes();
            m.lineno = m.lineno.swap_bytes();
            if is_string(m.typ) {
                m.set_str_range(m.str_range().swap_bytes());
                m.set_str_flags(m.str_flags().swap_bytes());
            } else {
                let q = m.value.q().swap_bytes();
                m.value.set_q(q);
                m.u = m.u.swap_bytes();
            }
        }
        m
    }
}

/// `FILE_MAGICSIZE`: the bytes of one compiled rule.
pub const FILE_MAGICSIZE: usize = 376;

// `Magic` is the compiled record, field for field.
const _: () = {
    use core::mem::{align_of, offset_of, size_of};
    assert!(size_of::<Magic>() == FILE_MAGICSIZE);
    assert!(align_of::<Magic>() == 8);
    assert!(offset_of!(Magic, cont_level) == 0);
    assert!(offset_of!(Magic, flag) == 2);
    assert!(offset_of!(Magic, mask_op) == 9);
    assert!(offset_of!(Magic, cond) == 10);
    assert!(offset_of!(Magic, factor_op) == 11);
    assert!(offset_of!(Magic, offset) == 12);
    assert!(offset_of!(Magic, in_offset) == 16);
    assert!(offset_of!(Magic, lineno) == 20);
    assert!(offset_of!(Magic, u) == 24);
    assert!(offset_of!(Magic, value) == 32);
    assert!(offset_of!(Magic, desc) == 160);
    assert!(offset_of!(Magic, mimetype) == 224);
    assert!(offset_of!(Magic, apple) == 304);
    assert!(offset_of!(Magic, ext) == 312);
};
/// `MAGICNO`: a compiled file's first word.
pub const MAGICNO: u32 = 0xF11E_041C;
/// `VERSIONNO`: the compiled format's version.
pub const VERSIONNO: u32 = 18;
