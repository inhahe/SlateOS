//! Where a grammar meets the runtime: the C structures the two share, as
//! Rust, and the functions the runtime calls a grammar through.
//!
//! A grammar is, to the tree-sitter runtime, a `TSLanguage` -- a struct of
//! counts, pointers to tables, and function pointers to its lexers and its
//! external scanner, laid out as `tree_sitter/parser.h` declares it. The
//! tables and lexers are generated from each grammar's `parser.c` by
//! `tsgrammar` (see `build.rs`); what they need from this side is here:
//!
//! - the `#[repr(C)]` mirrors of the shared structs ([`TSLanguage`],
//!   [`TSLexer`] and the table rows), so the runtime reads the generated
//!   statics as it would read the C compiler's;
//! - [`Lexer`], the runtime's lexer as a safe type, which every generated
//!   lexer and every hand-ported scanner works through;
//! - [`ExternalScanner`], the trait a hand-ported scanner implements, and
//!   [`scanner_table`], which turns one into the five functions the runtime
//!   calls.
//!
//! **This module is the crate's only `unsafe`.** Each block's argument is the
//! runtime's contract with a grammar -- which the C grammars rely on too --
//! written down where it is relied on.

use core::ffi::{c_char, c_void};
use core::marker::PhantomData;

#[cfg(target_endian = "big")]
compile_error!(
    "the grammars' tables are written little-endian, which is also all the runtime (tree-sitter-c2rust) supports"
);

/// `TSLexer`, as `tree_sitter/parser.h` declares it: the character ahead,
/// the symbol a token is, and the runtime's own functions.
#[repr(C)]
pub(crate) struct TSLexer {
    lookahead: i32,
    result_symbol: u16,
    advance: Option<unsafe extern "C" fn(*mut TSLexer, bool)>,
    mark_end: Option<unsafe extern "C" fn(*mut TSLexer)>,
    get_column: Option<unsafe extern "C" fn(*mut TSLexer) -> u32>,
    is_at_included_range_start: Option<unsafe extern "C" fn(*const TSLexer) -> bool>,
    eof: Option<unsafe extern "C" fn(*const TSLexer) -> bool>,
    /// `log`, which is variadic and never called from here: present for its
    /// size and place only.
    log: *const c_void,
}

/// The runtime's lexer, as a grammar's lexer or scanner uses it: the
/// character ahead, and what can be done with it.
pub struct Lexer<'a> {
    raw: *mut TSLexer,
    _borrow: PhantomData<&'a mut TSLexer>,
}

impl Lexer<'_> {
    /// The runtime's lexer, handed to a grammar's function.
    ///
    /// # Safety
    ///
    /// `raw` is the lexer the runtime passed to the function now running: it
    /// is valid, and nothing else uses it, for as long as the `Lexer` lives --
    /// which is the runtime's contract with every lexer function and scanner
    /// it calls.
    pub(crate) unsafe fn from_raw(raw: *mut TSLexer) -> Self {
        Self {
            raw,
            _borrow: PhantomData,
        }
    }

    /// The character ahead, as a code point; 0 at the end of the text.
    #[must_use]
    pub fn lookahead(&self) -> i32 {
        // SAFETY: `raw` is valid while `self` lives (`from_raw`); a field
        // read through it makes no reference to the runtime's struct.
        unsafe { (*self.raw).lookahead }
    }

    /// Take the character ahead into the token.
    pub fn advance(&mut self) {
        self.advance_with(false);
    }

    /// Step over the character ahead, leaving it out of the token (blanks
    /// before one).
    pub fn skip(&mut self) {
        self.advance_with(true);
    }

    /// Step over the character ahead: into the token, or skipped.
    pub fn advance_with(&mut self, skip: bool) {
        // SAFETY: `raw` is valid while `self` lives; the function is the
        // runtime's own, called with the lexer it belongs to, as C does.
        unsafe {
            if let Some(advance) = (*self.raw).advance {
                advance(self.raw, skip);
            }
        }
    }

    /// End the token here: what was advanced over so far is the token, and
    /// looking further ahead does not change that.
    pub fn mark_end(&mut self) {
        // SAFETY: as `advance_with`.
        unsafe {
            if let Some(mark_end) = (*self.raw).mark_end {
                mark_end(self.raw);
            }
        }
    }

    /// Whether the text has ended.
    #[must_use]
    pub fn eof(&self) -> bool {
        // SAFETY: as `advance_with`.
        unsafe { (*self.raw).eof.is_some_and(|eof| eof(self.raw)) }
    }

    /// Say which symbol the token is.
    pub fn set_result(&mut self, symbol: u16) {
        // SAFETY: as `lookahead`, and writing the one field the runtime
        // gives a lexer to write.
        unsafe {
            (*self.raw).result_symbol = symbol;
        }
    }

    /// The token so far is `symbol`, and ends here: `ACCEPT_TOKEN`.
    pub fn accept(&mut self, symbol: u16) {
        self.set_result(symbol);
        self.mark_end();
    }
}

/// Whether `c` is in `ranges` -- sorted, apart, each inclusive: a generated
/// lexer's character set, as `set_contains` in `tree_sitter/parser.h`.
#[must_use]
pub fn set_contains(ranges: &[(i32, i32)], c: i32) -> bool {
    ranges
        .binary_search_by(|&(start, end)| {
            if end < c {
                core::cmp::Ordering::Less
            } else if start > c {
                core::cmp::Ordering::Greater
            } else {
                core::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// A table's bytes, aligned for any of the rows it holds.
#[repr(C, align(8))]
pub struct Aligned<T: ?Sized>(pub T);

/// A row of the parse actions: `TSParseActionEntry`, eight bytes, read only
/// by the runtime.
#[repr(C)]
pub struct ParseActionEntry {
    _bytes: [u16; 4],
}

/// `TSMapSlice`.
#[repr(C)]
pub struct MapSlice {
    _index: u16,
    _length: u16,
}

/// `TSFieldMapEntry`.
#[repr(C)]
pub struct FieldMapEntry {
    _field_id: u16,
    _child_index: u8,
    _inherited: bool,
}

/// `TSSymbolMetadata`.
#[repr(C)]
pub struct SymbolMetadata {
    _visible: bool,
    _named: bool,
    _supertype: bool,
}

/// `TSLexerMode` (`TSLexMode`, its first two fields, before ABI 15).
#[repr(C)]
pub struct LexerMode {
    _lex_state: u16,
    _external_lex_state: u16,
    _reserved_word_set_id: u16,
}

/// `TSLanguageMetadata`.
#[repr(C)]
pub struct LanguageMetadata {
    /// The grammar's version, as its author numbered it.
    pub major_version: u8,
    /// See [`major_version`](Self::major_version).
    pub minor_version: u8,
    /// See [`major_version`](Self::major_version).
    pub patch_version: u8,
}

/// A language's external scanner, as the runtime calls it.
#[repr(C)]
pub struct ExternalScannerTable {
    states: *const bool,
    symbol_map: *const u16,
    create: Option<unsafe extern "C" fn() -> *mut c_void>,
    destroy: Option<unsafe extern "C" fn(*mut c_void)>,
    scan: Option<unsafe extern "C" fn(*mut c_void, *mut TSLexer, *const bool) -> bool>,
    serialize: Option<unsafe extern "C" fn(*mut c_void, *mut c_char) -> u32>,
    deserialize: Option<unsafe extern "C" fn(*mut c_void, *const c_char, u32)>,
}

impl ExternalScannerTable {
    /// No external scanner.
    pub const NONE: Self = Self {
        states: core::ptr::null(),
        symbol_map: core::ptr::null(),
        create: None,
        destroy: None,
        scan: None,
        serialize: None,
        deserialize: None,
    };
}

/// `TSLanguage`, as `tree_sitter/parser.h` (ABI 15) declares it, field for
/// field. An ABI-14 grammar fills the same struct and leaves the fields after
/// `primary_state_ids` empty, which the runtime does not read below 15.
#[repr(C)]
pub struct TSLanguage {
    pub abi_version: u32,
    pub symbol_count: u32,
    pub alias_count: u32,
    pub token_count: u32,
    pub external_token_count: u32,
    pub state_count: u32,
    pub large_state_count: u32,
    pub production_id_count: u32,
    pub field_count: u32,
    pub max_alias_sequence_length: u16,
    pub parse_table: *const u16,
    pub small_parse_table: *const u16,
    pub small_parse_table_map: *const u32,
    pub parse_actions: *const ParseActionEntry,
    pub symbol_names: *const *const c_char,
    pub field_names: *const *const c_char,
    pub field_map_slices: *const MapSlice,
    pub field_map_entries: *const FieldMapEntry,
    pub symbol_metadata: *const SymbolMetadata,
    pub public_symbol_map: *const u16,
    pub alias_map: *const u16,
    pub alias_sequences: *const u16,
    pub lex_modes: *const LexerMode,
    pub lex_fn: Option<unsafe extern "C" fn(*mut TSLexer, u16) -> bool>,
    pub keyword_lex_fn: Option<unsafe extern "C" fn(*mut TSLexer, u16) -> bool>,
    pub keyword_capture_token: u16,
    pub external_scanner: ExternalScannerTable,
    pub primary_state_ids: *const u16,
    pub name: *const c_char,
    pub reserved_words: *const u16,
    pub max_reserved_word_set_size: u16,
    pub supertype_count: u32,
    pub supertype_symbols: *const u16,
    pub supertype_map_slices: *const MapSlice,
    pub supertype_map_entries: *const u16,
    pub metadata: LanguageMetadata,
}

/// A grammar's language, as a static the runtime can be handed.
#[repr(transparent)]
pub struct SyncLanguage(pub TSLanguage);

// SAFETY: every pointer in a grammar's `TSLanguage` points at an immutable
// static -- its tables, its names, its functions -- and the runtime only
// reads through them; reading from several threads at once is sound.
unsafe impl Sync for SyncLanguage {}

/// A grammar's symbol or field names: pointers to C string literals.
#[repr(transparent)]
pub struct SyncPtrs<T>(pub T);

// SAFETY: the pointers are to string literals, which are immutable statics;
// nothing writes through them.
unsafe impl<const N: usize> Sync for SyncPtrs<[*const c_char; N]> {}

/// A grammar's lexer function, as the runtime calls it: `$name` wraps the
/// generated `$body`, which works through a [`Lexer`].
macro_rules! lexer_entry {
    ($name:ident, $body:ident) => {
        unsafe extern "C" fn $name(lexer: *mut crate::ffi::TSLexer, state: u16) -> bool {
            // SAFETY: the runtime calls a grammar's lexer with its own lexer,
            // valid and used by nothing else for the length of the call.
            let mut lexer = unsafe { crate::ffi::Lexer::from_raw(lexer) };
            $body(&mut lexer, state)
        }
    };
}
pub(crate) use lexer_entry;

/// The handle the runtime takes a grammar by, for the `LANGUAGE` static a
/// generated file defines.
macro_rules! language_fn {
    () => {
        extern "C" fn language() -> *const () {
            (&raw const LANGUAGE).cast()
        }

        /// The grammar, as the runtime takes it.
        pub(crate) fn language_fn() -> tree_sitter_language::LanguageFn {
            // SAFETY: `language` returns a pointer to a static `TSLanguage`,
            // laid out as `tree_sitter/parser.h` lays it out and valid for
            // the life of the process -- what the runtime asks of the
            // function a `LanguageFn` wraps.
            unsafe { tree_sitter_language::LanguageFn::from_raw(language) }
        }
    };
}
pub(crate) use language_fn;

/// The most a scanner may write when its state is saved:
/// `TREE_SITTER_SERIALIZATION_BUFFER_SIZE`.
pub const SERIALIZATION_BUFFER_SIZE: usize = 1024;

/// A hand-ported external scanner: the part of a grammar's lexing its
/// author wrote by hand in C (`scanner.c`), for tokens a state machine
/// cannot recognise -- indentation, raw strings, nested comments.
pub trait ExternalScanner: Default {
    /// Its tokens, in the grammar's order: what a `valid` list is indexed
    /// by. Tested against the grammar's own list (`EXTERNAL_TOKENS`).
    const TOKENS: &'static [&'static str];

    /// Try to recognise, at the lexer's position, one of the tokens `valid`
    /// allows; say which with [`Lexer::set_result`] and answer whether one
    /// was found.
    fn scan(&mut self, lexer: &mut Lexer<'_>, valid: &[bool]) -> bool;

    /// Save the scanner's state into `buffer`, answering how many bytes it
    /// wrote -- at most `buffer.len()`, which is [`SERIALIZATION_BUFFER_SIZE`].
    fn serialize(&self, buffer: &mut [u8]) -> usize;

    /// Restore a state [`serialize`](Self::serialize) wrote; empty means the
    /// state it starts in.
    fn deserialize(&mut self, bytes: &[u8]);
}

/// The runtime's table of functions for scanner `S`, over the grammar's
/// generated `states` and `symbol_map`.
#[must_use]
pub const fn scanner_table<S: ExternalScanner>(
    states: *const bool,
    symbol_map: *const u16,
) -> ExternalScannerTable {
    ExternalScannerTable {
        states,
        symbol_map,
        create: Some(scanner_create::<S>),
        destroy: Some(scanner_destroy::<S>),
        scan: Some(scanner_scan::<S>),
        serialize: Some(scanner_serialize::<S>),
        deserialize: Some(scanner_deserialize::<S>),
    }
}

extern "C" fn scanner_create<S: ExternalScanner>() -> *mut c_void {
    Box::into_raw(Box::new(S::default())).cast()
}

unsafe extern "C" fn scanner_destroy<S: ExternalScanner>(payload: *mut c_void) {
    if !payload.is_null() {
        // SAFETY: `payload` is what `scanner_create::<S>` returned for this
        // language -- a `Box<S>` -- and the runtime destroys each exactly
        // once, after its last use.
        drop(unsafe { Box::from_raw(payload.cast::<S>()) });
    }
}

unsafe extern "C" fn scanner_scan<S: ExternalScanner>(
    payload: *mut c_void,
    lexer: *mut TSLexer,
    valid: *const bool,
) -> bool {
    // SAFETY: `payload` is a live `Box<S>` from `scanner_create::<S>` that
    // the runtime uses for nothing else during the call; `valid` is a row of
    // the grammar's scanner states -- `external_token_count` flags, which is
    // `S::TOKENS.len()` (tested per grammar) -- or the runtime's own list of
    // as many; `lexer` is as in `lexer_entry!`.
    let (scanner, valid, mut lexer) = unsafe {
        (
            &mut *payload.cast::<S>(),
            core::slice::from_raw_parts(valid, S::TOKENS.len()),
            Lexer::from_raw(lexer),
        )
    };
    scanner.scan(&mut lexer, valid)
}

unsafe extern "C" fn scanner_serialize<S: ExternalScanner>(
    payload: *mut c_void,
    buffer: *mut c_char,
) -> u32 {
    // SAFETY: `payload` as in `scanner_scan`, only read; `buffer` is the
    // runtime's serialization buffer, `SERIALIZATION_BUFFER_SIZE` bytes,
    // ours alone for the call.
    let (scanner, buffer) = unsafe {
        (
            &*payload.cast::<S>(),
            core::slice::from_raw_parts_mut(buffer.cast::<u8>(), SERIALIZATION_BUFFER_SIZE),
        )
    };
    let written = scanner.serialize(buffer).min(SERIALIZATION_BUFFER_SIZE);
    u32::try_from(written).unwrap_or(0)
}

unsafe extern "C" fn scanner_deserialize<S: ExternalScanner>(
    payload: *mut c_void,
    buffer: *const c_char,
    length: u32,
) {
    // SAFETY: `payload` as in `scanner_scan`.
    let scanner = unsafe { &mut *payload.cast::<S>() };
    let length = usize::try_from(length).unwrap_or(0);
    let bytes: &[u8] = if length == 0 || buffer.is_null() {
        &[]
    } else {
        // SAFETY: the runtime passes a state it saved: `length` bytes it
        // holds for the length of the call.
        unsafe { core::slice::from_raw_parts(buffer.cast::<u8>(), length) }
    };
    scanner.deserialize(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The mirrors are the C structs' sizes on this target**: what a C
    /// compiler makes of `tree_sitter/parser.h` for x86-64. A field out of
    /// place in `TSLanguage` would have every grammar read from the wrong
    /// table; `the_runtime_reads_each_grammar_as_it_was_written` in lib.rs
    /// checks the fields the runtime reads by reading them through it.
    #[test]
    #[cfg(target_pointer_width = "64")]
    fn the_mirrors_are_the_c_structs_sizes() {
        use core::mem::{offset_of, size_of};
        assert_eq!(size_of::<ParseActionEntry>(), 8);
        assert_eq!(size_of::<MapSlice>(), 4);
        assert_eq!(size_of::<FieldMapEntry>(), 4);
        assert_eq!(size_of::<SymbolMetadata>(), 3);
        assert_eq!(size_of::<LexerMode>(), 6);
        assert_eq!(size_of::<TSLexer>(), 56);
        assert_eq!(size_of::<ExternalScannerTable>(), 56);
        // Nine u32s, a u16, padding to 8: 40.
        assert_eq!(offset_of!(TSLanguage, parse_table), 40);
        assert_eq!(offset_of!(TSLanguage, lex_fn), 40 + 13 * 8);
        assert_eq!(offset_of!(TSLanguage, keyword_capture_token), 160);
        assert_eq!(offset_of!(TSLanguage, external_scanner), 168);
        assert_eq!(offset_of!(TSLanguage, primary_state_ids), 224);
        assert_eq!(offset_of!(TSLanguage, name), 232);
        assert_eq!(offset_of!(TSLanguage, max_reserved_word_set_size), 248);
        assert_eq!(offset_of!(TSLanguage, supertype_count), 252);
        assert_eq!(offset_of!(TSLanguage, metadata), 280);
        assert_eq!(size_of::<TSLanguage>(), 288);
    }

    /// **A set holds a character when a range does**, ends included.
    #[test]
    fn a_set_holds_what_its_ranges_hold() {
        let set = [(65, 90), (97, 122), (192, 591)];
        for (c, want) in [
            (64, false),
            (65, true),
            (90, true),
            (91, false),
            (100, true),
            (591, true),
            (592, false),
        ] {
            assert_eq!(set_contains(&set, c), want, "{c}");
        }
        assert!(!set_contains(&[], 1));
    }
}
