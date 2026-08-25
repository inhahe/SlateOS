//! Tests for the layout tables.
//!
//! Two kinds. The first ask whether the *machinery* is right — that a row
//! string becomes the keys it names, that Caps Lock swaps the right things,
//! that an unknown id does not leave the machine mute. The second ask whether
//! the *data* is right, and they do it by sweeping every built-in rather than
//! spot-checking one, because a table is exactly the kind of thing where the
//! entry nobody looked at is the wrong one.

#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use super::{
    DEFAULT_ID, DeadFaces, Face, KeyDef, Layout, LayoutSpec, Level, ROW_BOTTOM, ROW_DIGITS,
    ROW_HOME, ROW_UPPER, block_scancodes, builtins, by_id, default_layout, sc,
};

/// The layout a test reaches for when it needs one and does not care which.
fn us() -> &'static Layout {
    by_id("us-qwerty").unwrap()
}

// ---------------------------------------------------------------------------
// The rows become the keys
// ---------------------------------------------------------------------------

#[test]
fn every_builtin_layout_fills_every_row() {
    // The check that makes `LayoutSpec::build`'s leniency safe. `build` zips
    // the row strings against the scancodes, so a row one character short
    // silently loses its last key — on the home row that is the key beside
    // Enter, which nobody would notice until they tried to type a backslash.
    // Here it is a failed build instead.
    let expected = [
        ROW_DIGITS.len(),
        ROW_UPPER.len(),
        ROW_HOME.len(),
        ROW_BOTTOM.len(),
    ];
    for layout in builtins() {
        for (index, want) in expected.iter().enumerate() {
            // The bottom row carries the ISO extra key ahead of `Z` when the
            // layout has one, so it is one longer than the scancode list.
            let iso = usize::from(index == 3 && layout.key(u32::from(sc::ISO_EXTRA)).is_some());
            assert_eq!(
                layout.row(index).len(),
                want + iso,
                "{} row {index}: rows must name every key, or the surplus is dropped",
                layout.id
            );
        }
    }
}

#[test]
fn a_row_string_lands_on_the_scancodes_it_is_written_against() {
    // The whole contract of the row form: the nth character of the home row
    // is what the nth home-row key produces. Checked on Dvorak rather than US
    // precisely because on US the answer would also be right if `build`
    // ignored the layout entirely and returned the physical table.
    let dvorak = by_id("dvorak").unwrap();
    assert_eq!(dvorak.character(u32::from(sc::A), Level::PLAIN), Some('a'));
    assert_eq!(dvorak.character(u32::from(sc::S), Level::PLAIN), Some('o'));
    assert_eq!(dvorak.character(u32::from(sc::D), Level::PLAIN), Some('e'));
    assert_eq!(dvorak.character(u32::from(sc::K), Level::PLAIN), Some('t'));
    // And the letter that moved furthest: US `Q` types `'` on Dvorak.
    assert_eq!(
        dvorak.character(u32::from(sc::Q), Level::PLAIN),
        Some('\''),
        "the top-left letter key"
    );
}

#[test]
fn the_iso_extra_key_exists_only_where_the_board_has_one() {
    // A US board has no key between left Shift and Z. Answering with a
    // character for it would mean a stuck or mis-wired key on a US machine
    // typed a `<` out of nowhere.
    assert_eq!(us().character(u32::from(sc::ISO_EXTRA), Level::PLAIN), None);
    let de = by_id("de-qwertz").unwrap();
    assert_eq!(
        de.character(u32::from(sc::ISO_EXTRA), Level::PLAIN),
        Some('<')
    );
    assert_eq!(
        de.character(u32::from(sc::ISO_EXTRA), Level::shift()),
        Some('>')
    );
}

#[test]
fn the_iso_extra_key_is_drawn_at_the_start_of_the_bottom_row() {
    // Where it is on the board. A preview that drew it last would put `<`
    // beside the slash, which is a picture of a keyboard nobody owns.
    let de = by_id("de-qwertz").unwrap();
    assert_eq!(de.row(3)[0].scancode, sc::ISO_EXTRA);
    assert_eq!(de.row(3)[1].plain, 'y', "German swaps Y and Z");
}

#[test]
fn a_key_outside_the_alphanumeric_block_is_not_the_layouts_business() {
    // Enter, F5 and the arrows mean the same thing on every layout, so they
    // are not repeated here — and `None` is what tells the compositor to fall
    // back to its physical table rather than inventing a character.
    for scancode in [0x1Cu32, 0x3F, 0xE04B, 0x39, 0x01] {
        for layout in builtins() {
            assert_eq!(
                layout.character(scancode, Level::PLAIN),
                None,
                "{} claimed {scancode:#x}",
                layout.id
            );
        }
    }
}

#[test]
fn a_scancode_too_large_for_the_table_is_answered_rather_than_wrapped() {
    // `key` narrows `u32` to `u16`. A `try_from` replaced by an `as` would
    // wrap `0x1_001E` onto `0x1E` and report that a nonexistent key types `a`.
    assert_eq!(us().character(0x1_001E, Level::PLAIN), None);
    assert_eq!(us().character(u32::MAX, Level::PLAIN), None);
}

// ---------------------------------------------------------------------------
// Levels
// ---------------------------------------------------------------------------

#[test]
fn shift_selects_the_upper_character() {
    assert_eq!(us().character(u32::from(sc::A), Level::shift()), Some('A'));
    assert_eq!(
        us().character(u32::from(sc::NUM1), Level::shift()),
        Some('!')
    );
}

#[test]
fn caps_lock_capitalises_letters_and_leaves_punctuation_alone() {
    // The rule Caps Lock actually follows, and the one a "if shift or caps"
    // implementation gets wrong: the latch is a *case* latch. With it on, `1`
    // is still `1`.
    let caps = Level {
        caps: true,
        ..Level::PLAIN
    };
    assert_eq!(us().character(u32::from(sc::A), caps), Some('A'));
    assert_eq!(us().character(u32::from(sc::NUM1), caps), Some('1'));
    assert_eq!(us().character(u32::from(sc::COMMA), caps), Some(','));
}

#[test]
fn caps_lock_and_shift_cancel_on_a_letter() {
    let both = Level {
        shift: true,
        caps: true,
        alt_gr: false,
    };
    assert_eq!(
        us().character(u32::from(sc::A), both),
        Some('a'),
        "shift with the latch on gives lowercase"
    );
    // …but not on a key that is not a case pair: Shift+1 is `!` whatever the
    // latch says, and a cancel applied there would type `1`.
    assert_eq!(us().character(u32::from(sc::NUM1), both), Some('!'));
}

#[test]
fn the_german_eszett_is_not_treated_as_the_lower_case_of_a_question_mark() {
    // `ß` is a letter and `?` is not, which is the whole reason
    // `caps_applies` tests both sides. A latch that looked only at the plain
    // character would type `?` for every `ß` while Caps Lock was on — a bug
    // that never shows up on a US board, because on US no key pairs a letter
    // with a non-letter.
    let de = by_id("de-qwertz").unwrap();
    let caps = Level {
        caps: true,
        ..Level::PLAIN
    };
    assert_eq!(de.character(u32::from(sc::MINUS), Level::PLAIN), Some('ß'));
    assert_eq!(
        de.character(u32::from(sc::MINUS), Level::shift()),
        Some('?')
    );
    assert_eq!(
        de.character(u32::from(sc::MINUS), caps),
        Some('ß'),
        "the latch must not reach a key whose shifted face is punctuation"
    );
}

#[test]
fn alt_gr_reaches_the_characters_a_national_layout_has_nowhere_else() {
    // The reason AltGr is a level and not a modifier. On a German board there
    // is no unshifted and no shifted `@` anywhere; AltGr+Q is the only way to
    // type one, so a layout system that treated AltGr as "Alt" would leave a
    // German user unable to write an email address.
    let de = by_id("de-qwertz").unwrap();
    assert_eq!(de.character(u32::from(sc::Q), Level::alt_gr()), Some('@'));
    assert_eq!(de.character(u32::from(sc::E), Level::alt_gr()), Some('€'));
    // And a key with no AltGr face produces nothing rather than its plain
    // character: AltGr+A is not `a`.
    assert_eq!(de.character(u32::from(sc::A), Level::alt_gr()), None);
}

#[test]
fn alt_gr_is_dead_on_a_layout_that_does_not_use_it() {
    assert!(!us().uses_alt_gr());
    assert!(by_id("de-qwertz").unwrap().uses_alt_gr());
    assert_eq!(us().character(u32::from(sc::Q), Level::alt_gr()), None);
}

#[test]
fn alt_gr_with_shift_falls_back_to_the_plain_alt_gr_face() {
    // Nothing in the built-ins declares a fourth level, so holding Shift as
    // well must not silently produce nothing — AltGr+Shift+Q is still `@`.
    // Losing the character there is the kind of bug a user reports as "it
    // works but only sometimes".
    let de = by_id("de-qwertz").unwrap();
    let level = Level {
        shift: true,
        caps: false,
        alt_gr: true,
    };
    assert_eq!(de.character(u32::from(sc::Q), level), Some('@'));
}

#[test]
fn a_declared_fourth_level_beats_the_fallback() {
    let spec = LayoutSpec {
        id: "test",
        display_name: "Test",
        short_label: "TT",
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
        iso_extra: None,
        altgr: &[(sc::Q, '@', Some('Ω'))],
        dead: &[],
    };
    let layout = spec.build();
    let level = Level {
        shift: true,
        caps: false,
        alt_gr: true,
    };
    assert_eq!(layout.character(u32::from(sc::Q), level), Some('Ω'));
    assert_eq!(
        layout.character(u32::from(sc::Q), Level::alt_gr()),
        Some('@')
    );
}

// ---------------------------------------------------------------------------
// Selection
// ---------------------------------------------------------------------------

#[test]
fn an_unknown_layout_id_leaves_the_user_with_a_keyboard() {
    // `input.yaml` is hand-editable. A typo in the layout name must not mean
    // no layout at all, because the way you would fix the typo is by typing.
    assert!(by_id("this-layout-does-not-exist").is_none());
    assert_eq!(default_layout().id, DEFAULT_ID);
    assert_eq!(
        default_layout().character(u32::from(sc::A), Level::PLAIN),
        Some('a')
    );
}

#[test]
fn every_builtin_has_a_distinct_id_and_can_be_found_by_it() {
    // Ids are what a saved setting stores. Two layouts sharing one means the
    // user's choice silently resolves to whichever is listed first.
    let mut seen: Vec<&str> = Vec::new();
    for layout in builtins() {
        assert!(
            !seen.contains(&layout.id),
            "duplicate layout id {}",
            layout.id
        );
        seen.push(layout.id);
        assert_eq!(by_id(layout.id).map(|l| l.id), Some(layout.id));
    }
    assert!(seen.contains(&DEFAULT_ID), "the default must be installed");
}

#[test]
fn every_builtin_names_itself_for_a_human_and_for_a_tray() {
    for layout in builtins() {
        assert!(!layout.display_name.is_empty(), "{}", layout.id);
        assert!(!layout.short_label.is_empty(), "{}", layout.id);
        assert!(
            layout.short_label.chars().count() <= 3,
            "{} has a tray label of {:?}, which will not fit",
            layout.id,
            layout.short_label
        );
        assert!(!layout.language.is_empty(), "{}", layout.id);
    }
}

// ---------------------------------------------------------------------------
// The data itself
// ---------------------------------------------------------------------------

#[test]
fn no_layout_puts_two_characters_on_one_physical_key() {
    // The failure mode of a table written as rows: a row with a character too
    // many pushes every later key along, so two entries claim one scancode
    // and one key is lost. Counting catches it; reading the row does not.
    for layout in builtins() {
        let mut seen: Vec<u16> = Vec::new();
        for key in layout.keys() {
            assert!(
                !seen.contains(&key.scancode),
                "{} defines {:#x} twice",
                layout.id,
                key.scancode
            );
            seen.push(key.scancode);
        }
    }
}

#[test]
fn every_key_a_layout_defines_is_part_of_the_alphanumeric_block() {
    let block = block_scancodes();
    for layout in builtins() {
        for key in layout.keys() {
            assert!(
                block.contains(&key.scancode),
                "{} defines {:#x}, which is not a key layouts may move",
                layout.id,
                key.scancode
            );
        }
    }
}

#[test]
fn every_english_layout_can_type_the_whole_alphabet() {
    // The check that catches a mistyped alternate layout, which is otherwise
    // very hard to eyeball: Dvorak, Colemak and Workman are permutations of
    // the same 26 letters, so a doubled letter means another is missing and
    // the user simply cannot type it.
    for layout in builtins().iter().filter(|l| l.language == "en") {
        for letter in 'a'..='z' {
            let hits = layout.keys().iter().filter(|k| k.plain == letter).count();
            assert_eq!(hits, 1, "{} has {hits} keys for {letter:?}", layout.id);
        }
        for digit in '0'..='9' {
            let hits = layout.keys().iter().filter(|k| k.plain == digit).count();
            assert_eq!(hits, 1, "{} has {hits} keys for {digit:?}", layout.id);
        }
    }
}

#[test]
fn every_layout_can_type_the_whole_alphabet_somewhere() {
    // Weaker than the English sweep and applied to all of them: a national
    // layout may put a letter on a shifted or AltGr face (French `µ`, German
    // `ß`), but a Latin-script layout that cannot produce some letter at all
    // is a typo, not a design.
    for layout in builtins() {
        for letter in 'a'..='z' {
            let upper = letter.to_ascii_uppercase();
            let found = layout.keys().iter().any(|k| {
                k.plain == letter
                    || k.plain == upper
                    || k.shifted == letter
                    || k.shifted == upper
                    || k.altgr == Some(letter)
            });
            assert!(found, "{} cannot type {letter:?}", layout.id);
        }
    }
}

#[test]
fn a_letter_key_pairs_its_own_capital() {
    // A permutation written by hand is exactly where the shifted row drifts
    // out of step with the unshifted one — one character inserted in the
    // shifted string and every letter after it is capitalised as its
    // neighbour, so `d` types `F`.
    for layout in builtins() {
        for key in layout.keys() {
            if key.plain.is_ascii_lowercase() {
                assert_eq!(
                    key.shifted,
                    key.plain.to_ascii_uppercase(),
                    "{}: {:#x} is {:?} plain but {:?} shifted",
                    layout.id,
                    key.scancode,
                    key.plain,
                    key.shifted
                );
            }
        }
    }
}

#[test]
fn the_alternate_english_layouts_move_keys_rather_than_inventing_them() {
    // Dvorak, Colemak and Workman are rearrangements of US QWERTY: the same
    // multiset of characters, in a different order. A layout that gained or
    // lost one is a typo — and this catches the case the alphabet sweep
    // cannot, which is punctuation.
    let mut reference: Vec<char> = us().keys().iter().map(|k| k.plain).collect();
    reference.sort_unstable();
    for id in ["dvorak", "colemak", "workman"] {
        let layout = by_id(id).unwrap();
        let mut chars: Vec<char> = layout.keys().iter().map(|k| k.plain).collect();
        chars.sort_unstable();
        assert_eq!(chars, reference, "{id} is not a permutation of US QWERTY");
    }
}

#[test]
fn a_layout_that_defines_alt_gr_for_a_key_it_does_not_have_is_ignored_quietly() {
    // `build` attaches AltGr faces by scancode, and a scancode that is not in
    // the block has no key to attach to. Dropping it is right — the
    // alternative is a key with an AltGr face and no plain one, which would
    // produce a character only while AltGr was held.
    let spec = LayoutSpec {
        id: "test",
        display_name: "Test",
        short_label: "TT",
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
        iso_extra: None,
        // 0x1C is Enter, which is not a key a layout may move.
        altgr: &[(0x1C, 'X', None)],
        dead: &[],
    };
    let layout = spec.build();
    assert_eq!(layout.character(0x1C, Level::alt_gr()), None);
    assert!(!layout.uses_alt_gr());
}

#[test]
fn caps_applies_is_decided_by_both_faces_of_the_key() {
    // Direct unit check of the predicate the latch turns on, stated as the
    // three cases that matter rather than through a whole layout.
    let letter = KeyDef {
        scancode: sc::A,
        plain: 'a',
        shifted: 'A',
        altgr: None,
        altgr_shifted: None,
        dead: DeadFaces::NONE,
    };
    let digit = KeyDef {
        plain: '1',
        shifted: '!',
        ..letter
    };
    let half = KeyDef {
        plain: 'ß',
        shifted: '?',
        ..letter
    };
    assert!(letter.caps_applies());
    assert!(!digit.caps_applies());
    assert!(!half.caps_applies(), "a letter paired with punctuation");
}

// ---------------------------------------------------------------------------
// Dead keys
// ---------------------------------------------------------------------------

/// Every level a key can be asked about, so that a sweep is a sweep. Caps Lock
/// is included because it selects between the first two faces and is therefore
/// capable of selecting the wrong one.
const ALL_LEVELS: [Level; 8] = [
    Level {
        shift: false,
        caps: false,
        alt_gr: false,
    },
    Level {
        shift: true,
        caps: false,
        alt_gr: false,
    },
    Level {
        shift: false,
        caps: true,
        alt_gr: false,
    },
    Level {
        shift: true,
        caps: true,
        alt_gr: false,
    },
    Level {
        shift: false,
        caps: false,
        alt_gr: true,
    },
    Level {
        shift: true,
        caps: false,
        alt_gr: true,
    },
    Level {
        shift: false,
        caps: true,
        alt_gr: true,
    },
    Level {
        shift: true,
        caps: true,
        alt_gr: true,
    },
];

#[test]
fn a_face_is_selected_exactly_when_a_character_is() {
    // `character` reads whichever face `face` picked, so the two can only
    // disagree by one of them growing a rule the other lacks. Sweeping every
    // key of every layout at every level is cheap and is the only way to catch
    // a fourth-level fallback that was added to one and not the other.
    for layout in builtins() {
        for key in layout.keys() {
            for level in ALL_LEVELS {
                assert_eq!(
                    key.face(level).is_some(),
                    key.character(level).is_some(),
                    "{} sc {:#04x} at {level:?}",
                    layout.id,
                    key.scancode
                );
            }
        }
    }
}

#[test]
fn every_dead_face_has_a_character() {
    // A dead flag on a face the key does not have is a table entry that can
    // never fire, and almost certainly a scancode typo — the layout author
    // meant a different key and this one silently declared nothing. `build`
    // attaches the flags after the AltGr pass precisely so that an AltGr face
    // exists by the time this runs.
    for layout in builtins() {
        for key in layout.keys() {
            if key.dead.altgr {
                assert!(
                    key.altgr.is_some(),
                    "{} sc {:#04x} declares its AltGr face dead but has no character there",
                    layout.id,
                    key.scancode
                );
            }
            if key.dead.altgr_shifted {
                assert!(
                    key.altgr_shifted.is_some(),
                    "{} sc {:#04x} declares its AltGr+Shift face dead but has none",
                    layout.id,
                    key.scancode
                );
            }
        }
    }
}

#[test]
fn every_dead_key_is_an_accent() {
    // The catcher for a mistyped scancode that *does* land on a real key: the
    // flag attaches quietly and the wrong key goes dead. A dead `t` would sail
    // through every other test here and be discovered by someone unable to
    // type their own name.
    let accents = ['^', '´', '`', '¨', '~'];
    for layout in builtins() {
        for key in layout.keys() {
            for level in ALL_LEVELS {
                if key.is_dead(level) {
                    // `is_dead` answers `false` for a level with no character,
                    // so arriving here without one means those two disagreed —
                    // which is worth naming rather than reporting as a bare
                    // failed unwrap in a sweep over four hundred keys.
                    let Some(ch) = key.character(level) else {
                        panic!(
                            "{} sc {:#04x} at {level:?} is dead with no character",
                            layout.id, key.scancode
                        );
                    };
                    assert!(
                        accents.contains(&ch),
                        "{} sc {:#04x} at {level:?} is dead and types {ch:?}, which is not an accent",
                        layout.id,
                        key.scancode
                    );
                }
            }
        }
    }
}

#[test]
fn every_declared_dead_key_reached_a_key() {
    // `build` drops a declaration whose scancode is not in the block, the same
    // way it drops an AltGr one, so that a typo costs a key rather than the
    // display server. Nothing is allowed to rely on that: here a dropped
    // declaration is a failed build.
    for spec in super::SPECS {
        let layout = spec.build();
        for &(scancode, faces) in spec.dead {
            let key = layout
                .key(u32::from(scancode))
                .unwrap_or_else(|| panic!("{}: sc {scancode:#04x} is not in the block", spec.id));
            assert_eq!(key.dead, faces, "{} sc {scancode:#04x}", spec.id);
        }
    }
}

#[test]
fn only_the_national_layouts_have_dead_keys() {
    // English boards reach every letter they need directly, so a pending
    // accent on one would be state that can never resolve. The compositor uses
    // this to skip the whole state machine.
    for layout in builtins() {
        let expected = matches!(layout.id, "de-qwertz" | "fr-azerty" | "es-qwerty");
        assert_eq!(
            layout.uses_dead_keys(),
            expected,
            "{} uses_dead_keys",
            layout.id
        );
    }
}

#[test]
fn french_has_a_dead_circumflex_and_a_live_one_on_the_same_board() {
    // The case that decided the shape of the whole feature. `^` is dead on the
    // key right of `P`, where it makes `ê`; the same character on AltGr+9 is
    // the circumflex a programmer types into a shell and must arrive as
    // itself. A table keyed on the character rather than the face would have
    // to be wrong about one of them.
    let fr = by_id("fr-azerty").unwrap();
    let dead = u32::from(sc::LEFT_BRACKET);
    let live = u32::from(sc::NUM9);
    assert_eq!(fr.character(dead, Level::PLAIN), Some('^'));
    assert_eq!(fr.character(live, Level::alt_gr()), Some('^'));
    assert!(fr.is_dead(dead, Level::PLAIN));
    assert!(!fr.is_dead(live, Level::alt_gr()));
}

#[test]
fn german_holds_a_dead_key_and_a_live_one_on_the_two_faces_of_one_key() {
    // The key left of `1`: `^` waits for a vowel, `°` is a degree sign that
    // combines with nothing. Shift alone tells them apart.
    let de = by_id("de-qwertz").unwrap();
    let key = u32::from(sc::GRAVE);
    assert_eq!(de.character(key, Level::PLAIN), Some('^'));
    assert_eq!(de.character(key, Level::shift()), Some('°'));
    assert!(de.is_dead(key, Level::PLAIN));
    assert!(!de.is_dead(key, Level::shift()));
}

#[test]
fn the_caps_lock_latch_does_not_reach_a_dead_key() {
    // Caps Lock swaps the first two faces only where both are letters, and an
    // accent key's faces are not. So Caps Lock on a German board must not turn
    // the acute into the grave — which is what a latch written as "a second
    // Shift" would do, and it would do it to the one key where the two faces
    // are different accents rather than one letter in two cases.
    let de = by_id("de-qwertz").unwrap();
    let key = u32::from(sc::EQUALS);
    let caps = Level {
        caps: true,
        ..Level::PLAIN
    };
    assert_eq!(de.character(key, caps), Some('´'));
    assert!(de.is_dead(key, caps));
    assert_eq!(de.character(key, Level::shift()), Some('`'));
    assert!(de.is_dead(key, Level::shift()));
}

#[test]
fn a_dead_key_still_reports_the_character_it_is_labelled_with() {
    // This crate answers "what is printed on that key", and a caller that does
    // not ask `is_dead` gets `´` rather than nothing. That is deliberate: the
    // shell's keyboard preview draws exactly this, and the wrong-but-visible
    // behaviour is the safer one for anything that forgets to ask.
    for (id, scancode) in [
        ("de-qwertz", sc::EQUALS),
        ("fr-azerty", sc::LEFT_BRACKET),
        ("es-qwerty", sc::APOSTROPHE),
    ] {
        let layout = by_id(id).unwrap();
        assert!(
            layout
                .character(u32::from(scancode), Level::PLAIN)
                .is_some(),
            "{id} sc {scancode:#04x}"
        );
    }
}

#[test]
fn a_fourth_level_that_falls_back_takes_the_deadness_with_it() {
    // AltGr+Shift on a key with no fourth-level character falls back to the
    // AltGr character. Its *deadness* has to fall back too, or the key would
    // type a live tilde while the compositor waited for a vowel — and the next
    // letter typed would vanish into a composition nobody asked for.
    let key = KeyDef {
        scancode: sc::NUM2,
        plain: '2',
        shifted: '"',
        altgr: Some('~'),
        altgr_shifted: None,
        dead: DeadFaces::ALT_GR,
    };
    let both = Level {
        shift: true,
        ..Level::alt_gr()
    };
    assert_eq!(key.face(both), Some(Face::AltGr));
    assert_eq!(
        key.character(both),
        Some('~'),
        "falls back to the AltGr face"
    );
    assert!(key.is_dead(both), "and to that face's deadness");
}

#[test]
fn a_level_with_no_character_is_never_dead() {
    // Belt and braces for the flag `every_dead_face_has_a_character` forbids:
    // if one ever slipped in, `is_dead` must still answer `false`, because a
    // key that produces nothing cannot be waiting to combine with anything.
    let key = KeyDef {
        scancode: sc::A,
        plain: 'a',
        shifted: 'A',
        altgr: None,
        altgr_shifted: None,
        dead: DeadFaces {
            altgr: true,
            altgr_shifted: true,
            ..DeadFaces::NONE
        },
    };
    assert_eq!(key.character(Level::alt_gr()), None);
    assert!(!key.is_dead(Level::alt_gr()));
}

#[test]
fn an_unknown_scancode_is_not_dead() {
    // The fallback path: a scancode outside the block is answered by the
    // compositor's physical table, where nothing is dead. Saying `false` here
    // rather than "unknown" is what lets the caller ask unconditionally.
    for layout in builtins() {
        assert!(!layout.is_dead(0x1C, Level::PLAIN), "{} Enter", layout.id);
        assert!(!layout.is_dead(0x3B, Level::PLAIN), "{} F1", layout.id);
    }
}
