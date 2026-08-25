//! Keyboard layouts: what each physical key produces, in one place.
//!
//! A keyboard reports *which switch closed*, not *which letter it means*. The
//! same switch is `A` on a US board, `Q` on a French one and `A` again on
//! Dvorak but at a different place on the board. Something has to hold the
//! translation, and `design-decisions.md` §456 puts the act of translating in
//! the compositor, so that one system layout governs every application at once
//! rather than each client inventing its own.
//!
//! This crate is the *data* half of that: the tables themselves, in a crate
//! with no dependencies, so that both parties can read them.
//!
//! - The **compositor** turns a scancode into a `guitk::event::Key` and a
//!   character to insert. It must not depend on a widget toolkit to do it.
//! - The **desktop shell** names the installed layouts in the tray and draws a
//!   picture of the active one. It must not depend on a display server to do
//!   it.
//!
//! Before this crate existed those two had a table each — a US-only scancode
//! table in the compositor and a set of row strings in
//! `gui/desktop/src/input_method.rs` that nothing outside its own unit tests
//! ever read. Two tables describing one keyboard is two answers to one
//! question, and the shell's answer was the one shown to the user while the
//! compositor's was the one they typed with. See `known-issues.md` →
//! `TD-ONLY-ONE-KEYBOARD-LAYOUT`.
//!
//! ## What a layout is, here
//!
//! Exactly the 48 keys of the alphanumeric block — the four rows between Esc
//! and the space bar, plus the extra key that ISO keyboards have and ANSI ones
//! do not. Nothing else moves between layouts: Enter, Tab, the function row,
//! the arrows and the keypad mean the same thing everywhere, so they stay in
//! the compositor's physical table and are not repeated here. A layout that
//! listed them would be 88 rows of which 40 were identical in every layout,
//! and the forty would drift.
//!
//! Scancodes are **scan code set 1**, matching
//! `gui/compositor/src/keymap.rs` and `kernel/src/keyboard.rs`. None of the
//! alphanumeric block is an extended (`0xE0`-prefixed) key, so a `u16` holds
//! them with room to spare.
//!
//! ## Levels
//!
//! Each key carries up to four characters: plain, shifted, AltGr, and
//! AltGr+Shift. AltGr — the right-hand Alt key on a European board — is a
//! *level shift*, not a modifier in the Ctrl/Alt sense: on German, AltGr+Q is
//! `@`, which is a character the layout has nowhere else to put. Layouts that
//! do not use it leave those entries empty.
//!
//! Caps Lock is not a fifth level. It swaps plain and shifted **for letters
//! only**, which is why [`KeyDef::caps_applies`] exists: on German the key
//! right of `0` is `ß` unshifted and `?` shifted, and a Caps Lock that treated
//! that as a case pair would type a question mark for every `ß`.
//!
//! ## What is deliberately not here yet
//!
//! Dead keys (`´` then `e` → `é`) and compose sequences. Both need state
//! carried between two key events, and this crate is a pure lookup. The
//! layouts that use them — German, French, Spanish — currently emit the
//! accent character itself, which is what a dead key produces when you press
//! it twice, so the behaviour is wrong in a way that at least produces the
//! character the key is labelled with rather than nothing. Tracked as steps
//! (3) and (4) of `TD-ONLY-ONE-KEYBOARD-LAYOUT`.

use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Physical positions
// ---------------------------------------------------------------------------

/// Scan-code-set-1 codes for the alphanumeric block, named by their US caps.
///
/// The names are US labels because a *position* needs a name and the US board
/// is the one everyone can picture; `sc::Q` means "the key in the top-left of
/// the letters", not "the key that types Q". On AZERTY that key types `a`.
pub mod sc {
    /// Backtick/tilde on US — the key left of `1`.
    pub const GRAVE: u16 = 0x29;
    /// `1`.
    pub const NUM1: u16 = 0x02;
    /// `2`.
    pub const NUM2: u16 = 0x03;
    /// `3`.
    pub const NUM3: u16 = 0x04;
    /// `4`.
    pub const NUM4: u16 = 0x05;
    /// `5`.
    pub const NUM5: u16 = 0x06;
    /// `6`.
    pub const NUM6: u16 = 0x07;
    /// `7`.
    pub const NUM7: u16 = 0x08;
    /// `8`.
    pub const NUM8: u16 = 0x09;
    /// `9`.
    pub const NUM9: u16 = 0x0A;
    /// `0`.
    pub const NUM0: u16 = 0x0B;
    /// Minus on US — the key right of `0`.
    pub const MINUS: u16 = 0x0C;
    /// Equals on US.
    pub const EQUALS: u16 = 0x0D;

    /// `Q` on US.
    pub const Q: u16 = 0x10;
    /// `W` on US.
    pub const W: u16 = 0x11;
    /// `E` on US.
    pub const E: u16 = 0x12;
    /// `R` on US.
    pub const R: u16 = 0x13;
    /// `T` on US.
    pub const T: u16 = 0x14;
    /// `Y` on US.
    pub const Y: u16 = 0x15;
    /// `U` on US.
    pub const U: u16 = 0x16;
    /// `I` on US.
    pub const I: u16 = 0x17;
    /// `O` on US.
    pub const O: u16 = 0x18;
    /// `P` on US.
    pub const P: u16 = 0x19;
    /// Left bracket on US.
    pub const LEFT_BRACKET: u16 = 0x1A;
    /// Right bracket on US.
    pub const RIGHT_BRACKET: u16 = 0x1B;

    /// `A` on US.
    pub const A: u16 = 0x1E;
    /// `S` on US.
    pub const S: u16 = 0x1F;
    /// `D` on US.
    pub const D: u16 = 0x20;
    /// `F` on US.
    pub const F: u16 = 0x21;
    /// `G` on US.
    pub const G: u16 = 0x22;
    /// `H` on US.
    pub const H: u16 = 0x23;
    /// `J` on US.
    pub const J: u16 = 0x24;
    /// `K` on US.
    pub const K: u16 = 0x25;
    /// `L` on US.
    pub const L: u16 = 0x26;
    /// Semicolon on US.
    pub const SEMICOLON: u16 = 0x27;
    /// Apostrophe on US.
    pub const APOSTROPHE: u16 = 0x28;
    /// Backslash on US — the key left of Enter on an ANSI board, and the key
    /// that carries `#` on a UK one and `#` again on German.
    pub const BACKSLASH: u16 = 0x2B;

    /// `Z` on US.
    pub const Z: u16 = 0x2C;
    /// `X` on US.
    pub const X: u16 = 0x2D;
    /// `C` on US.
    pub const C: u16 = 0x2E;
    /// `V` on US.
    pub const V: u16 = 0x2F;
    /// `B` on US.
    pub const B: u16 = 0x30;
    /// `N` on US.
    pub const N: u16 = 0x31;
    /// `M` on US.
    pub const M: u16 = 0x32;
    /// Comma on US.
    pub const COMMA: u16 = 0x33;
    /// Period on US.
    pub const PERIOD: u16 = 0x34;
    /// Slash on US.
    pub const SLASH: u16 = 0x35;

    /// The extra key an ISO keyboard has and an ANSI one does not, between
    /// left Shift and `Z`.
    ///
    /// It is why a German board can put `<` and `>` on their own key while the
    /// US board has to shift them onto comma and period. A layout that does
    /// not declare it simply has 47 keys instead of 48.
    pub const ISO_EXTRA: u16 = 0x56;
}

/// The number row, left to right: backtick, `1`…`0`, minus, equals.
pub const ROW_DIGITS: [u16; 13] = [
    sc::GRAVE,
    sc::NUM1,
    sc::NUM2,
    sc::NUM3,
    sc::NUM4,
    sc::NUM5,
    sc::NUM6,
    sc::NUM7,
    sc::NUM8,
    sc::NUM9,
    sc::NUM0,
    sc::MINUS,
    sc::EQUALS,
];

/// The upper letter row, `Q`…`]` on US. Twelve keys; Tab and Enter are not
/// part of the block.
pub const ROW_UPPER: [u16; 12] = [
    sc::Q,
    sc::W,
    sc::E,
    sc::R,
    sc::T,
    sc::Y,
    sc::U,
    sc::I,
    sc::O,
    sc::P,
    sc::LEFT_BRACKET,
    sc::RIGHT_BRACKET,
];

/// The home row, `A`…`'` plus the key left of Enter.
///
/// Backslash is last rather than absent: on ANSI it sits above Enter and on
/// ISO beside it, but it is the same switch (`0x2B`) either way, and putting
/// it at the end of the home row is where every layout below wants to name it.
pub const ROW_HOME: [u16; 12] = [
    sc::A,
    sc::S,
    sc::D,
    sc::F,
    sc::G,
    sc::H,
    sc::J,
    sc::K,
    sc::L,
    sc::SEMICOLON,
    sc::APOSTROPHE,
    sc::BACKSLASH,
];

/// The bottom letter row, `Z`…`/` on US. The ISO extra key is declared
/// separately and is prepended to this row when a layout has one.
pub const ROW_BOTTOM: [u16; 10] = [
    sc::Z,
    sc::X,
    sc::C,
    sc::V,
    sc::B,
    sc::N,
    sc::M,
    sc::COMMA,
    sc::PERIOD,
    sc::SLASH,
];

/// Every scancode a layout may define, in physical reading order.
#[must_use]
pub fn block_scancodes() -> Vec<u16> {
    let mut out = Vec::with_capacity(48);
    out.extend_from_slice(&ROW_DIGITS);
    out.extend_from_slice(&ROW_UPPER);
    out.extend_from_slice(&ROW_HOME);
    out.push(sc::ISO_EXTRA);
    out.extend_from_slice(&ROW_BOTTOM);
    out
}

// ---------------------------------------------------------------------------
// One key
// ---------------------------------------------------------------------------

/// What one physical key produces on one layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyDef {
    /// Scan code set 1 code of the physical key.
    pub scancode: u16,
    /// The character with no modifier held.
    pub plain: char,
    /// The character with Shift held.
    pub shifted: char,
    /// The character with AltGr held, if the layout puts one there.
    pub altgr: Option<char>,
    /// The character with AltGr and Shift held, if the layout puts one there.
    pub altgr_shifted: Option<char>,
}

impl KeyDef {
    /// Whether Caps Lock should swap [`Self::plain`] and [`Self::shifted`] on
    /// this key.
    ///
    /// Only where *both* sides are letters. Caps Lock is a case latch, not a
    /// second Shift: on a German board the key right of `0` gives `ß` plain
    /// and `?` shifted, and a latch that treated every key as a case pair
    /// would type `?` for every `ß` — which is how a user discovers that the
    /// rule was written as "if it is a letter" and stopped there. `ß` is a
    /// letter; `?` is not, and that is the half of the test that does the
    /// work.
    #[must_use]
    pub fn caps_applies(&self) -> bool {
        self.plain.is_alphabetic() && self.shifted.is_alphabetic()
    }

    /// The character this key produces with the given modifiers held.
    #[must_use]
    pub fn character(&self, level: Level) -> Option<char> {
        if level.alt_gr {
            // No Caps Lock involvement: the latch selects between the first two
            // levels, and a layout that wanted a capital on the AltGr level
            // spells it out in `altgr_shifted`.
            return if level.shift {
                self.altgr_shifted.or(self.altgr)
            } else {
                self.altgr
            };
        }
        let upper = if self.caps_applies() {
            level.shift != level.caps
        } else {
            level.shift
        };
        Some(if upper { self.shifted } else { self.plain })
    }
}

/// Which of a key's levels the currently-held modifiers select.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Level {
    /// Either Shift key is held.
    pub shift: bool,
    /// The Caps Lock latch is on.
    pub caps: bool,
    /// AltGr — the right-hand Alt key — is held.
    pub alt_gr: bool,
}

impl Level {
    /// Nothing held.
    pub const PLAIN: Self = Self {
        shift: false,
        caps: false,
        alt_gr: false,
    };

    /// Shift held and nothing else.
    #[must_use]
    pub const fn shift() -> Self {
        Self {
            shift: true,
            ..Self::PLAIN
        }
    }

    /// AltGr held and nothing else.
    #[must_use]
    pub const fn alt_gr() -> Self {
        Self {
            alt_gr: true,
            ..Self::PLAIN
        }
    }
}

// ---------------------------------------------------------------------------
// A layout
// ---------------------------------------------------------------------------

/// A named keyboard layout: the alphanumeric block, and how to describe it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    /// Stable identifier, and what is written to `input.yaml` — `"dvorak"`,
    /// `"de-qwertz"`. Never translated, never renamed once shipped: it is the
    /// only thing tying a user's saved choice to a table.
    pub id: &'static str,
    /// Name for a settings list — `"German (QWERTZ)"`.
    pub display_name: &'static str,
    /// Two or three characters for the tray indicator — `"DE"`.
    pub short_label: &'static str,
    /// ISO 639 language code the layout is for. `"en"` for the alternate
    /// English layouts, which differ from US QWERTY in position rather than
    /// in language.
    ///
    /// This is what separates "a rearrangement of English" from "a national
    /// layout", which is a real distinction with a real reader: only the
    /// English ones are required to reach the whole alphabet from the plain
    /// level (see `every_english_layout_can_type_the_whole_alphabet`).
    ///
    /// There is deliberately **no** `is_rtl` beside it. A right-to-left script
    /// is a fact about the text a layout writes, not about where its caps sit:
    /// Arabic and Hebrew boards put their letters on physically standard
    /// positions — ض and / are both on the key engraved `Q` — so a preview
    /// that mirrored its rows for them would be drawing a keyboard that does
    /// not exist. Nothing here would read the flag correctly, so nothing here
    /// carries it. See `design-decisions.md` §549.
    pub language: &'static str,
    /// The keys, in physical reading order: number row, upper row, home row,
    /// then the ISO extra key (if any) followed by the bottom row.
    keys: Vec<KeyDef>,
    /// Index one past the last key of each row, into [`Self::keys`].
    row_ends: [usize; 4],
}

impl Layout {
    /// What this key produces, or `None` if the layout does not define it.
    ///
    /// `None` means "not part of the alphanumeric block, or not present on
    /// this layout" — Enter, F5, and the ISO extra key on a US board all
    /// answer `None`, and the caller falls back to the physical table.
    #[must_use]
    pub fn key(&self, scancode: u32) -> Option<&KeyDef> {
        let scancode = u16::try_from(scancode).ok()?;
        self.keys.iter().find(|k| k.scancode == scancode)
    }

    /// The character this scancode produces at this level, if any.
    #[must_use]
    pub fn character(&self, scancode: u32, level: Level) -> Option<char> {
        self.key(scancode)?.character(level)
    }

    /// One row of the block, left to right, for drawing a picture of the
    /// keyboard. Rows are numbered 0 (digits) to 3 (bottom).
    #[must_use]
    pub fn row(&self, index: usize) -> &[KeyDef] {
        let Some(&end) = self.row_ends.get(index) else {
            return &[];
        };
        let start = index
            .checked_sub(1)
            .and_then(|prev| self.row_ends.get(prev).copied())
            .unwrap_or(0);
        self.keys.get(start..end).unwrap_or(&[])
    }

    /// Every key on the layout, in physical reading order.
    #[must_use]
    pub fn keys(&self) -> &[KeyDef] {
        &self.keys
    }

    /// Whether any key on this layout puts a character on the AltGr level.
    ///
    /// The tray and the settings list use it to say whether AltGr does
    /// anything here, which on a US layout it does not.
    #[must_use]
    pub fn uses_alt_gr(&self) -> bool {
        self.keys.iter().any(|k| k.altgr.is_some())
    }
}

// ---------------------------------------------------------------------------
// Building a layout from rows
// ---------------------------------------------------------------------------

/// A layout written the way a person reads a keyboard: four rows of
/// characters, unshifted and shifted, in physical order.
///
/// This is the *only* form the built-in layouts are written in. Deriving the
/// per-key table from it — rather than keeping both — is the point: the row
/// strings are what a human can check against a photograph of the keyboard,
/// and the per-key table is what a lookup needs, and one of them has to be
/// computed from the other or they will disagree.
pub struct LayoutSpec {
    /// See [`Layout::id`].
    pub id: &'static str,
    /// See [`Layout::display_name`].
    pub display_name: &'static str,
    /// See [`Layout::short_label`].
    pub short_label: &'static str,
    /// See [`Layout::language`].
    pub language: &'static str,
    /// Unshifted characters: digits row (13), upper row (12), home row (12),
    /// bottom row (10).
    pub plain: [&'static str; 4],
    /// Shifted characters, same four rows and same lengths.
    pub shifted: [&'static str; 4],
    /// The ISO extra key's plain and shifted characters, on layouts that have
    /// one. `None` on ANSI layouts, which simply have 47 keys.
    pub iso_extra: Option<(char, char)>,
    /// AltGr characters, listed sparsely as `(scancode, plain, shifted)`.
    ///
    /// Sparse rather than four more rows because the layouts that use AltGr
    /// use it on eight or ten keys out of forty-eight, and forty rows of
    /// padding would hide the ten that matter.
    pub altgr: &'static [(u16, char, Option<char>)],
}

impl LayoutSpec {
    /// Expand the rows into a [`Layout`].
    ///
    /// A row whose plain and shifted strings are not both exactly the expected
    /// length contributes only as many keys as the shorter of the two — the
    /// surplus is dropped rather than panicking, because a mistyped table
    /// should cost a key, not the whole desktop at startup. Nothing relies on
    /// that leniency: `every_builtin_layout_fills_every_row` fails the build
    /// if any built-in is short, which is where a mistyped row is meant to be
    /// caught.
    #[must_use]
    pub fn build(&self) -> Layout {
        let rows: [&[u16]; 4] = [&ROW_DIGITS, &ROW_UPPER, &ROW_HOME, &ROW_BOTTOM];
        let mut keys: Vec<KeyDef> = Vec::with_capacity(48);
        let mut row_ends = [0usize; 4];
        for (index, scancodes) in rows.iter().enumerate() {
            // The ISO extra key belongs to the bottom row and is drawn before
            // `Z`, which is where it sits on the board.
            if index == 3
                && let Some((plain, shifted)) = self.iso_extra
            {
                keys.push(KeyDef {
                    scancode: sc::ISO_EXTRA,
                    plain,
                    shifted,
                    altgr: None,
                    altgr_shifted: None,
                });
            }
            let plain = self.plain.get(index).copied().unwrap_or("");
            let shifted = self.shifted.get(index).copied().unwrap_or("");
            for ((&scancode, plain), shifted) in
                scancodes.iter().zip(plain.chars()).zip(shifted.chars())
            {
                keys.push(KeyDef {
                    scancode,
                    plain,
                    shifted,
                    altgr: None,
                    altgr_shifted: None,
                });
            }
            // `rows` and `row_ends` are both four long, so this always finds
            // its slot — but "always" is an argument about a constant defined
            // elsewhere in the file, and an index that panics on a keyboard
            // layout would take the display server down with it.
            if let Some(end) = row_ends.get_mut(index) {
                *end = keys.len();
            }
        }
        for &(scancode, plain, shifted) in self.altgr {
            if let Some(key) = keys.iter_mut().find(|k| k.scancode == scancode) {
                key.altgr = Some(plain);
                key.altgr_shifted = shifted;
            }
        }
        Layout {
            id: self.id,
            display_name: self.display_name,
            short_label: self.short_label,
            language: self.language,
            keys,
            row_ends,
        }
    }
}

// ---------------------------------------------------------------------------
// The built-in layouts
// ---------------------------------------------------------------------------

/// The layouts that ship with the system, in the order a settings list shows
/// them: the default first, then the alternate English layouts, then the
/// national ones.
#[must_use]
pub fn builtins() -> &'static [Layout] {
    static BUILTINS: OnceLock<Vec<Layout>> = OnceLock::new();
    BUILTINS.get_or_init(|| SPECS.iter().map(LayoutSpec::build).collect())
}

/// The layout with this id, or `None`.
#[must_use]
pub fn by_id(id: &str) -> Option<&'static Layout> {
    builtins().iter().find(|l| l.id == id)
}

/// The id of the layout used when `input.yaml` says nothing.
pub const DEFAULT_ID: &str = "us-qwerty";

/// The layout used when `input.yaml` says nothing, or names one that does not
/// exist.
///
/// Falling back rather than failing: an unknown id in a hand-edited config
/// file must not leave the machine with no keyboard at all, and US QWERTY is
/// the layout whose caps match what is printed on most hardware.
#[must_use]
pub fn default_layout() -> &'static Layout {
    by_id(DEFAULT_ID)
        .or_else(|| builtins().first())
        .unwrap_or_else(|| {
            // Unreachable: `SPECS` is a non-empty constant and its first entry is
            // `us-qwerty`. Written as a leak rather than a panic so that a future
            // edit which empties `SPECS` degrades to an empty layout — every key
            // falls through to the compositor's physical table, which is US
            // QWERTY — instead of taking the display server down at startup.
            static EMPTY: OnceLock<Layout> = OnceLock::new();
            EMPTY.get_or_init(|| {
                LayoutSpec {
                    id: "empty",
                    display_name: "Empty",
                    short_label: "--",
                    language: "en",
                    plain: ["", "", "", ""],
                    shifted: ["", "", "", ""],
                    iso_extra: None,
                    altgr: &[],
                }
                .build()
            })
        })
}

/// The row-string form of every built-in layout.
///
/// Each entry is checkable against a photograph of the keyboard it names,
/// which is the reason the tables are written this way and not as forty-eight
/// `(scancode, char, char)` triples.
static SPECS: &[LayoutSpec] = &[
    // -- English -----------------------------------------------------------
    LayoutSpec {
        id: "us-qwerty",
        display_name: "US English (QWERTY)",
        short_label: "EN",
        language: "en",
        plain: [
            "`1234567890-=",
            "qwertyuiop[]",
            "asdfghjkl;'\\",
            "zxcvbnm,./",
        ],
        shifted: [
            "~!@#$%^&*()_+",
            "QWERTYUIOP{}",
            "ASDFGHJKL:\"|",
            "ZXCVBNM<>?",
        ],
        // ANSI: no key between left Shift and Z.
        iso_extra: None,
        altgr: &[],
    },
    LayoutSpec {
        id: "uk-qwerty",
        display_name: "UK English (QWERTY)",
        short_label: "UK",
        language: "en",
        plain: [
            "`1234567890-=",
            "qwertyuiop[]",
            "asdfghjkl;'#",
            "zxcvbnm,./",
        ],
        shifted: [
            "¬!\"£$%^&*()_+",
            "QWERTYUIOP{}",
            "ASDFGHJKL:@~",
            "ZXCVBNM<>?",
        ],
        // The UK board is ISO, so backslash moves off the home row and onto
        // the key beside left Shift; `#` takes its place.
        iso_extra: Some(('\\', '|')),
        altgr: &[(sc::NUM4, '€', None)],
    },
    LayoutSpec {
        id: "dvorak",
        display_name: "Dvorak",
        short_label: "DV",
        language: "en",
        plain: [
            "`1234567890[]",
            "',.pyfgcrl/=",
            "aoeuidhtns-\\",
            ";qjkxbmwvz",
        ],
        shifted: [
            "~!@#$%^&*(){}",
            "\"<>PYFGCRL?+",
            "AOEUIDHTNS_|",
            ":QJKXBMWVZ",
        ],
        iso_extra: None,
        altgr: &[],
    },
    LayoutSpec {
        id: "colemak",
        display_name: "Colemak",
        short_label: "CO",
        language: "en",
        plain: [
            "`1234567890-=",
            "qwfpgjluy;[]",
            "arstdhneio'\\",
            "zxcvbkm,./",
        ],
        shifted: [
            "~!@#$%^&*()_+",
            "QWFPGJLUY:{}",
            "ARSTDHNEIO\"|",
            "ZXCVBKM<>?",
        ],
        iso_extra: None,
        altgr: &[],
    },
    LayoutSpec {
        id: "workman",
        display_name: "Workman",
        short_label: "WK",
        language: "en",
        plain: [
            "`1234567890-=",
            "qdrwbjfup;[]",
            "ashtgyneoi'\\",
            "zxmcvkl,./",
        ],
        shifted: [
            "~!@#$%^&*()_+",
            "QDRWBJFUP:{}",
            "ASHTGYNEOI\"|",
            "ZXMCVKL<>?",
        ],
        iso_extra: None,
        altgr: &[],
    },
    // -- National ----------------------------------------------------------
    LayoutSpec {
        id: "de-qwertz",
        display_name: "German (QWERTZ)",
        short_label: "DE",
        language: "de",
        plain: [
            "^1234567890ß´",
            "qwertzuiopü+",
            "asdfghjklöä#",
            "yxcvbnm,.-",
        ],
        shifted: [
            "°!\"§$%&/()=?`",
            "QWERTZUIOPÜ*",
            "ASDFGHJKLÖÄ'",
            "YXCVBNM;:_",
        ],
        iso_extra: Some(('<', '>')),
        altgr: &[
            (sc::NUM2, '²', None),
            (sc::NUM3, '³', None),
            (sc::NUM7, '{', None),
            (sc::NUM8, '[', None),
            (sc::NUM9, ']', None),
            (sc::NUM0, '}', None),
            (sc::MINUS, '\\', None),
            (sc::Q, '@', None),
            (sc::E, '€', None),
            (sc::RIGHT_BRACKET, '~', None),
            (sc::M, 'µ', None),
            (sc::ISO_EXTRA, '|', None),
        ],
    },
    LayoutSpec {
        id: "fr-azerty",
        display_name: "French (AZERTY)",
        short_label: "FR",
        language: "fr",
        plain: [
            "²&é\"'(-è_çà)=",
            "azertyuiop^$",
            "qsdfghjklmù*",
            "wxcvbn,;:!",
        ],
        shifted: [
            "~1234567890°+",
            "AZERTYUIOP¨£",
            "QSDFGHJKLM%µ",
            "WXCVBN?./§",
        ],
        iso_extra: Some(('<', '>')),
        altgr: &[
            (sc::NUM2, '~', None),
            (sc::NUM3, '#', None),
            (sc::NUM4, '{', None),
            (sc::NUM5, '[', None),
            (sc::NUM6, '|', None),
            (sc::NUM7, '`', None),
            (sc::NUM8, '\\', None),
            (sc::NUM9, '^', None),
            (sc::NUM0, '@', None),
            (sc::MINUS, ']', None),
            (sc::EQUALS, '}', None),
            (sc::E, '€', None),
        ],
    },
    LayoutSpec {
        id: "es-qwerty",
        display_name: "Spanish (QWERTY)",
        short_label: "ES",
        language: "es",
        plain: [
            "º1234567890'¡",
            "qwertyuiop`+",
            "asdfghjklñ´ç",
            "zxcvbnm,.-",
        ],
        shifted: [
            "ª!\"·$%&/()=?¿",
            "QWERTYUIOP^*",
            "ASDFGHJKLÑ¨Ç",
            "ZXCVBNM;:_",
        ],
        iso_extra: Some(('<', '>')),
        altgr: &[
            (sc::GRAVE, '\\', None),
            (sc::NUM1, '|', None),
            (sc::NUM2, '@', None),
            (sc::NUM3, '#', None),
            (sc::NUM4, '~', None),
            (sc::NUM6, '¬', None),
            (sc::E, '€', None),
            (sc::LEFT_BRACKET, '[', None),
            (sc::RIGHT_BRACKET, ']', None),
            (sc::SEMICOLON, '{', None),
            (sc::APOSTROPHE, '}', None),
        ],
    },
];

#[cfg(test)]
mod tests;
