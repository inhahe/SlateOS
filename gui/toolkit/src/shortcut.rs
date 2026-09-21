//! Turning a printed shortcut label back into the keystrokes it names.
//!
//! Apps here print their keys — in a footer (`apps/minesweeper`,
//! `apps/mixer`, `apps/wordsearch`), or in a card raised by `?`
//! (`apps/rssreader`, `apps/slides`). Every one of those lists is a second
//! copy of something the key handler already knows, and two copies of a fact
//! drift apart: `apps/rssreader` shipped an overlay of twenty-one shortcuts of
//! which about four worked, and three of its rows named operations that
//! existed nowhere in the crate.
//!
//! The only thing that keeps a printed list and a handler together is a test
//! that reads both, and such a test has to turn the *printed* label back into
//! an event. This module is that step, in one place rather than one per app:
//! five apps had a list and none had the test, and the reason was never that
//! the test was hard — it was that each one started with forty lines of
//! `"Left" => Key::Left`.
//!
//! ```
//! # use guitk::event::Key;
//! # use guitk::shortcut::keystrokes;
//! let strokes = keystrokes("Ctrl+PageUp / Ctrl+PageDown")?;
//! assert_eq!(strokes.len(), 2);
//! assert_eq!(strokes.first().map(|s| s.key), Some(Key::PageUp));
//! assert!(strokes.first().is_some_and(|s| s.modifiers.ctrl));
//! # Ok::<(), guitk::shortcut::UnknownKey>(())
//! ```
//!
//! **What this does not do is decide whether the key works.** It reports the
//! keystrokes a label names; whether anything answers them is a question only
//! the app's own `handle_event` can be asked, from a state where the key has
//! something to do. That second half stays in each app, because the states
//! differ — and it is worth knowing that many handlers decline *on purpose*
//! (`Left` at the first slide, `1` for a view already open), so the property
//! to assert is "some reachable state answers this key", not "this key is
//! taken right now".

use std::error::Error;
use std::fmt;

use crate::event::{Key, KeyEvent, Modifiers};
use crate::palette::Palette;
use crate::render::{FontWeightHint, RenderCommand, TextOverflow};
use crate::surface::{CommandSink, Surface};
use crate::text;

/// A part of a shortcut label that names no key this module can press.
///
/// Carries the whole label as well as the offending part, because a test that
/// fails on `"Esk"` is far easier to read when it also says which row that
/// came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownKey {
    /// The label as the app prints it.
    pub label: String,
    /// The piece of it that could not be read. Empty when the label was.
    pub part: String,
}

impl fmt::Display for UnknownKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.part.is_empty() {
            write!(f, "a shortcut label names no key at all")
        } else {
            write!(
                f,
                "the shortcut label {:?} names {:?}, which is not a key this can press",
                self.label, self.part
            )
        }
    }
}

impl Error for UnknownKey {}

/// Every keystroke a printed shortcut label names, in the order it names them.
///
/// The shapes understood, which are the ones the apps in this tree already
/// print:
///
/// | Written | Read as |
/// |---|---|
/// | `Ctrl+Shift+T` | one stroke, both modifiers |
/// | `Left / Right`, `Up/Down` | two strokes — `/` separates alternatives |
/// | `Arrows` | four strokes, one per arrow |
/// | `WASD` | four strokes, the movement keys a game binds beside the arrows |
/// | `Esc`, `PgUp`, `Return`, `Del` | the spellings apps actually use |
/// | `?` | `Shift` and the slash key, which is what produces it |
/// | `Ctrl+/` | one stroke — a `/` straight after `+` is a key, not a separator |
/// | `Ctrl+F / /` | two strokes — the second `/` is the key, not an empty part |
/// | `0-9, A-F` | sixteen strokes — a range, and `,` separates like `/` does |
///
/// Modifier names are matched without case, as are key names of more than one
/// character; a single-character name is a key cap (`A` and `a` are the same
/// key, and there is no lower-case key).
///
/// # Errors
///
/// [`UnknownKey`] if any part names something this cannot press, rather than
/// quietly returning the parts it did understand — a guard test handed a
/// shorter list than it asked for would pass while checking less, which is
/// the failure it exists to prevent.
pub fn keystrokes(label: &str) -> Result<Vec<KeyEvent>, UnknownKey> {
    let mut strokes = Vec::new();
    for alternative in split_alternatives(label) {
        let chord = alternative.trim();
        if chord.is_empty() {
            continue;
        }
        // "Arrows" is one word for four keys, and every app here that prints a
        // movement hint prints it that way.
        if chord.eq_ignore_ascii_case("arrows") {
            for key in [Key::Left, Key::Right, Key::Up, Key::Down] {
                strokes.push(stroke(key, Modifiers::NONE));
            }
            continue;
        }
        // `WASD` is the same abbreviation one step along: four keys under one
        // word, and the word is what a game prints. `apps/asteroids`,
        // `apps/game2048`, `apps/snake` and `apps/sokoban` all bind these
        // beside the arrows. Without this they would have to print
        // `W, A, S, D`, which is the list bending to the parser -- the same
        // argument the range syntax below is written on.
        //
        // Order is W, A, S, D rather than the arrows' left-right-up-down: the
        // word names the keys in the order the keys sit under the hand, and a
        // reader comparing the label to the strokes should find them in the
        // order the label wrote them.
        if chord.eq_ignore_ascii_case("wasd") {
            for key in [Key::W, Key::A, Key::S, Key::D] {
                strokes.push(stroke(key, Modifiers::NONE));
            }
            continue;
        }
        // `0-9`, `A-F`, `1-8`: a run of keys written the way a person writes
        // one. Apps reach for this constantly -- a hex editor's digits, a
        // game's eight levels -- and spelling it out as `0 / 1 / 2 / ...`
        // to satisfy a parser would be the list bending to the checker.
        if let Some(run) = span(chord) {
            strokes.extend(run);
            continue;
        }
        let Some(one) = chord_to_stroke(chord) else {
            return Err(UnknownKey {
                label: label.to_owned(),
                part: chord.to_owned(),
            });
        };
        strokes.push(one);
    }
    if strokes.is_empty() {
        return Err(UnknownKey {
            label: label.to_owned(),
            part: String::new(),
        });
    }
    Ok(strokes)
}

/// The keys a range like `0-9` or `A-F` names, if it is one.
///
/// Only an exact `X-Y` of two alphanumerics, so the `-` key itself (`Ctrl+-`,
/// or a bare `-`) is never mistaken for a range: those are one character or
/// carry a `+`, and neither matches.
///
/// Reversed or mixed ranges (`9-0`, `A-3`) are not ranges and fall through to
/// be reported as an unknown key, which is what they are. Silently returning
/// nothing for them would be this module's own failure mode -- a short list
/// where a longer one was asked for.
fn span(chord: &str) -> Option<Vec<KeyEvent>> {
    let mut chars = chord.chars();
    let from = chars.next()?;
    if chars.next()? != '-' {
        return None;
    }
    let to = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let (from, to) = (from.to_ascii_uppercase(), to.to_ascii_uppercase());
    if from.is_ascii_digit() != to.is_ascii_digit() || from > to {
        return None;
    }
    if !from.is_ascii_alphanumeric() || !to.is_ascii_alphanumeric() {
        return None;
    }
    let mut out = Vec::new();
    for cap in from..=to {
        let (key, shifted) = single_key(cap)?;
        let mut modifiers = Modifiers::NONE;
        modifiers.shift = shifted;
        out.push(stroke(key, modifiers));
    }
    Some(out)
}

/// Split a label on the `/` that separates alternatives.
///
/// Two `/` are *not* separators, because the slash is also a key and apps
/// print it as one:
///
/// * one directly after a `+` is the key (`Ctrl+/`);
/// * one with nothing but blanks before it since the last split is the key --
///   which is what makes `apps/rssreader`'s `"Ctrl+F / /"` read as `Ctrl+F`
///   *or* `/`, two keys, rather than as `Ctrl+F` and a part that is gone.
///   Dropping it silently was this function's first bug, and it would have
///   cost a guard test one of the two keys it meant to check, while passing.
fn split_alternatives(label: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut previous = '\0';
    for (at, ch) in label.char_indices() {
        let blank_so_far = label.get(start..at).is_none_or(|s| s.trim().is_empty());
        if (ch == '/' || ch == ',') && previous != '+' && !blank_so_far {
            if let Some(piece) = label.get(start..at) {
                parts.push(piece);
            }
            start = at.saturating_add(1);
        }
        previous = ch;
    }
    if let Some(piece) = label.get(start..) {
        parts.push(piece);
    }
    parts
}

/// One `Ctrl+Shift+X` chord.
fn chord_to_stroke(chord: &str) -> Option<KeyEvent> {
    let mut modifiers = Modifiers::NONE;
    let mut rest = chord;
    while let Some((head, tail)) = rest.split_once('+') {
        // A `+` with nothing after it is the key `+`, not a separator.
        if tail.is_empty() {
            break;
        }
        match head.trim().to_ascii_lowercase().as_str() {
            "ctrl" | "control" => modifiers.ctrl = true,
            "shift" => modifiers.shift = true,
            "alt" | "option" => modifiers.alt = true,
            "super" | "cmd" | "win" | "meta" => modifiers.super_key = true,
            _ => return None,
        }
        rest = tail;
    }
    let (key, shifted) = key_from_name(rest.trim())?;
    if shifted {
        modifiers.shift = true;
    }
    Some(stroke(key, modifiers))
}

/// A key name, and whether producing that character needs Shift held.
fn key_from_name(name: &str) -> Option<(Key, bool)> {
    let mut chars = name.chars();
    let first = chars.next()?;
    if chars.next().is_none() {
        return single_key(first);
    }

    let folded: String = name
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_' && *c != '-')
        .flat_map(char::to_lowercase)
        .collect();

    if let Some(number) = folded.strip_prefix('f') {
        if let Ok(nth) = number.parse::<usize>() {
            if let Some(key) = nth.checked_sub(1).and_then(|i| FUNCTION_KEYS.get(i)) {
                return Some((*key, false));
            }
        }
    }

    let key = match folded.as_str() {
        "left" => Key::Left,
        "right" => Key::Right,
        "up" => Key::Up,
        "down" => Key::Down,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" | "pgup" => Key::PageUp,
        "pagedown" | "pgdn" | "pgdown" => Key::PageDown,
        "backspace" | "bksp" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "insert" | "ins" => Key::Insert,
        "enter" | "return" => Key::Enter,
        "tab" => Key::Tab,
        "esc" | "escape" => Key::Escape,
        "space" | "spacebar" => Key::Space,
        "comma" => Key::Comma,
        "period" | "dot" => Key::Period,
        "semicolon" => Key::Semicolon,
        "colon" => Key::Colon,
        "slash" => Key::Slash,
        "backslash" => Key::Backslash,
        "minus" | "dash" => Key::Minus,
        "equals" => Key::Equals,
        "printscreen" => Key::PrintScreen,
        "scrolllock" => Key::ScrollLock,
        "pause" => Key::Pause,
        "capslock" => Key::CapsLock,
        "numlock" => Key::NumLock,
        "volumeup" => Key::VolumeUp,
        "volumedown" => Key::VolumeDown,
        "volumemute" => Key::VolumeMute,
        "playpause" | "mediaplaypause" => Key::MediaPlayPause,
        "nexttrack" | "medianexttrack" => Key::MediaNextTrack,
        "prevtrack" | "mediaprevtrack" => Key::MediaPrevTrack,
        "mediastop" => Key::MediaStop,
        _ => return None,
    };
    Some((key, false))
}

/// A one-character key cap.
fn single_key(cap: char) -> Option<(Key, bool)> {
    if cap.is_ascii_alphabetic() {
        let index = u8::try_from(cap)
            .ok()?
            .to_ascii_lowercase()
            .checked_sub(b'a')?;
        return LETTERS.get(usize::from(index)).map(|key| (*key, false));
    }
    if cap.is_ascii_digit() {
        let index = u8::try_from(cap).ok()?.checked_sub(b'0')?;
        return DIGITS.get(usize::from(index)).map(|key| (*key, false));
    }
    let key = match cap {
        ' ' => Key::Space,
        ',' => Key::Comma,
        '.' => Key::Period,
        ';' => Key::Semicolon,
        ':' => Key::Colon,
        '/' => Key::Slash,
        '\\' => Key::Backslash,
        '[' => Key::LeftBracket,
        ']' => Key::RightBracket,
        '-' => Key::Minus,
        '=' => Key::Equals,
        '\'' => Key::Apostrophe,
        '`' => Key::Grave,
        // The shifted caps an app is likely to print. `?` is the one that
        // matters: it raises every help overlay in this tree, and a
        // `Key::Slash` with no Shift is a different key press entirely.
        '?' => return Some((Key::Slash, true)),
        '<' => return Some((Key::Comma, true)),
        '>' => return Some((Key::Period, true)),
        '+' => return Some((Key::Equals, true)),
        '_' => return Some((Key::Minus, true)),
        '~' => return Some((Key::Grave, true)),
        '"' => return Some((Key::Apostrophe, true)),
        _ => return None,
    };
    Some((key, false))
}

/// A press, with no text: what a shortcut is.
///
/// `text` stays empty because a shortcut is matched on its key and modifiers.
/// A caller testing that typing `a` inserts an `a` wants a `KeyEvent` it built
/// itself, carrying the text the platform would have produced.
fn stroke(key: Key, modifiers: Modifiers) -> KeyEvent {
    KeyEvent {
        key,
        pressed: true,
        modifiers,
        text: String::new(),
    }
}

// ── Drawing one ────────────────────────────────────────────────────────────

/// The height of one row of the card.
const ROW_HEIGHT: f32 = 20.0;

/// Room above the first row for the heading.
const HEAD_HEIGHT: f32 = 38.0;

/// Room below the last row, so the final row is not against the edge.
const FOOT_HEIGHT: f32 = 18.0;

/// The gap between the key column and the description column.
const COLUMN_GAP: f32 = 26.0;

/// Where the card's text starts, in from its left edge.
const PAD: f32 = 16.0;

/// Draw `rows` as a card laid over the window.
///
/// Written once because it had been written eight times. Every app that grew a
/// shortcut overlay on 2026-09-18 carried its own forty-line copy of this, and
/// the copies had already drifted in the one number none of them could check:
/// the x the description column starts at was a hand-picked constant -- 176,
/// 196, 206, 216 -- guessed per app from the longest key label somebody
/// eyeballed. Here it is *measured*, so a row reading `Ctrl+Shift+PageDown`
/// cannot overlap its own description.
///
/// `keep_clear` is the lowest `y` the card may start at: an app with a toolbar
/// passes its height so the card cannot cover it. The card is centred in what
/// is left.
///
/// Emits into anything a draw site already has -- `Vec<RenderCommand>`,
/// `Frame`, `RenderTree` -- through [`CommandSink`].
pub fn render_card<S: CommandSink + ?Sized>(
    out: &mut S,
    palette: &Palette,
    window: (f32, f32),
    keep_clear: f32,
    rows: &[(&str, &str)],
    closing: &str,
) {
    let (window_w, window_h) = window;
    // The key column is as wide as the widest key label, not as wide as
    // somebody guessed. A `max_width` that is too small elides the key itself,
    // which is the one string on the card that must be readable exactly.
    let keys_w = rows
        .iter()
        .map(|(keys, _)| text::measure(keys, 11.0, FontWeightHint::Bold))
        .fold(0.0_f32, f32::max);

    let w = (keys_w + COLUMN_GAP + 220.0 + PAD * 2.0)
        .max(320.0)
        .min(window_w * 0.86);

    // How many rows there is room for, and what to do about the rest.
    //
    // **A row that does not fit is named, never dropped.** `apps/rssreader`
    // drew twenty of its twenty-one shortcuts for weeks because its overlay
    // was a fixed height with a `break` when the rows ran past the bottom, and
    // the row it lost was the one naming the key that closes it. Nothing could
    // see it: the list and the key handler agreed, and the box was a third
    // quantity agreeing with neither. So this counts what fits, and if any are
    // left over it spends one of those lines saying how many -- which is worth
    // more than the row it displaces, because a reader who can see that
    // something is missing goes looking.
    let room = (window_h - keep_clear - HEAD_HEIGHT - FOOT_HEIGHT).max(0.0);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "room/ROW_HEIGHT is a small non-negative count of rows"
    )]
    let fits = (room / ROW_HEIGHT).floor().max(0.0) as usize;
    let (shown, hidden) = if fits >= rows.len() {
        (rows, 0)
    } else {
        // One line goes to the count, so `shown` is one shorter than `fits`.
        let keep = fits.saturating_sub(1);
        (
            rows.get(..keep).unwrap_or(&[]),
            rows.len().saturating_sub(keep),
        )
    };

    #[expect(
        clippy::cast_precision_loss,
        reason = "a shortcut list is tens of rows, far below f32's integer-exact range"
    )]
    let drawn_rows = shown.len().saturating_add(usize::from(hidden > 0)) as f32;
    let h = drawn_rows.mul_add(ROW_HEIGHT, HEAD_HEIGHT + FOOT_HEIGHT);
    let x = ((window_w - w) / 2.0).max(0.0);
    let y = ((window_h - h) / 2.0).max(keep_clear);

    palette.push_surface(out, x, y, w, h, 6.0, Surface::Card);
    out.emit(RenderCommand::Text {
        x: x + PAD,
        y: y + 12.0,
        text: format!("Keys  --  {closing}"),
        color: palette.ink(palette.blue),
        font_size: 13.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(w - PAD * 2.0),
        overflow: TextOverflow::Ellipsis,
    });

    let what_x = x + PAD + keys_w + COLUMN_GAP;
    let mut row_y = y + HEAD_HEIGHT;
    for (keys, what) in shown {
        out.emit(RenderCommand::Text {
            x: x + PAD,
            y: row_y,
            text: (*keys).to_string(),
            color: palette.ink(palette.peach),
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(keys_w),
            overflow: TextOverflow::Ellipsis,
        });
        out.emit(RenderCommand::Text {
            x: what_x,
            y: row_y,
            text: (*what).to_string(),
            color: palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((x + w - PAD - what_x).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        row_y += ROW_HEIGHT;
    }

    if hidden > 0 {
        out.emit(RenderCommand::Text {
            x: x + PAD,
            y: row_y,
            text: format!("... and {hidden} more, in a taller window"),
            color: palette.ink(palette.yellow),
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(w - PAD * 2.0),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

const LETTERS: [Key; 26] = [
    Key::A,
    Key::B,
    Key::C,
    Key::D,
    Key::E,
    Key::F,
    Key::G,
    Key::H,
    Key::I,
    Key::J,
    Key::K,
    Key::L,
    Key::M,
    Key::N,
    Key::O,
    Key::P,
    Key::Q,
    Key::R,
    Key::S,
    Key::T,
    Key::U,
    Key::V,
    Key::W,
    Key::X,
    Key::Y,
    Key::Z,
];

const DIGITS: [Key; 10] = [
    Key::Num0,
    Key::Num1,
    Key::Num2,
    Key::Num3,
    Key::Num4,
    Key::Num5,
    Key::Num6,
    Key::Num7,
    Key::Num8,
    Key::Num9,
];

const FUNCTION_KEYS: [Key; 12] = [
    Key::F1,
    Key::F2,
    Key::F3,
    Key::F4,
    Key::F5,
    Key::F6,
    Key::F7,
    Key::F8,
    Key::F9,
    Key::F10,
    Key::F11,
    Key::F12,
];

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::{UnknownKey, keystrokes};
    use crate::event::Key;

    /// The keys a label names, with no modifiers involved.
    fn keys(label: &str) -> Vec<Key> {
        keystrokes(label)
            .unwrap_or_else(|e| panic!("{e}"))
            .into_iter()
            .map(|s| s.key)
            .collect()
    }

    // ── The card ───────────────────────────────────────────────────────────

    use super::render_card;
    use crate::palette::Palette;
    use crate::render::RenderCommand;

    const ROWS: &[(&str, &str)] = &[
        ("F1 / ?", "This list"),
        ("Ctrl+Shift+PageDown", "Move this slide down"),
        ("T", "Add a text box"),
    ];

    fn drawn() -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        render_card(
            &mut cmds,
            &Palette::for_mode(false),
            (1280.0, 720.0),
            40.0,
            ROWS,
            "F1 or ? closes this",
        );
        cmds
    }

    fn texts(cmds: &[RenderCommand]) -> Vec<(f32, f32, String)> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text { x, y, text, .. } => Some((*x, *y, text.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_card_too_short_for_its_rows_says_how_many_it_left_out() {
        // The `apps/rssreader` defect, made structurally impossible. Its
        // overlay drew twenty of twenty-one rows and said nothing, and the row
        // it lost was the one naming the key that closes it. A reader who can
        // see that something is missing goes looking; one who cannot, cannot.
        let many: Vec<(&str, &str)> = (0..40).map(|_| ("Ctrl+Q", "Quit")).collect::<Vec<_>>();
        let mut cmds = Vec::new();
        render_card(
            &mut cmds,
            &Palette::for_mode(false),
            (900.0, 300.0),
            0.0,
            &many,
            "x",
        );
        let placed = texts(&cmds);
        let all = placed
            .iter()
            .map(|(_, _, t)| t.as_str())
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(
            all.contains("more, in a taller window"),
            "forty rows in a 300px window and the card said nothing: {all}"
        );

        // And what it *did* draw stayed inside the window, rather than running
        // off the bottom where the truth is equally unreadable.
        for (_, y, t) in &placed {
            assert!(*y <= 300.0, "{t:?} was drawn at {y}, past the window");
        }
    }

    #[test]
    fn a_card_with_room_leaves_nothing_out_and_says_nothing_about_it() {
        let mut cmds = Vec::new();
        render_card(
            &mut cmds,
            &Palette::for_mode(false),
            (900.0, 800.0),
            0.0,
            ROWS,
            "x",
        );
        let all = texts(&cmds)
            .iter()
            .map(|(_, _, t)| t.clone())
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(
            !all.contains("more, in a taller window"),
            "three rows in an 800px window and it claimed to have dropped some"
        );
        for (keys, _) in ROWS {
            assert!(all.contains(keys), "{keys:?} is not on the card");
        }
    }

    #[test]
    fn every_row_reaches_the_card() {
        let placed = texts(&drawn());
        let all = placed
            .iter()
            .map(|(_, _, t)| t.as_str())
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(all.contains("F1 or ? closes this"), "no heading: {all}");
        for (keys, what) in ROWS {
            assert!(all.contains(keys), "{keys:?} is not on the card");
            assert!(all.contains(what), "{what:?} is not on the card");
        }
    }

    #[test]
    fn the_rows_are_stacked_rather_than_heaped() {
        // Reading the strings back proves every row was *drawn*; it says
        // nothing about where. Drawn at one y they are a single illegible
        // smear, and a test that only joins the texts together passes exactly
        // as loudly. `apps/minesweeper` has the same test over its footer for
        // the same reason.
        let placed = texts(&drawn());
        let mut ys: Vec<f32> = placed
            .iter()
            .filter(|(_, _, t)| ROWS.iter().any(|(k, _)| k == t))
            .map(|(_, y, _)| *y)
            .collect();
        assert_eq!(ys.len(), ROWS.len(), "not every key reached the card");
        ys.sort_by(f32::total_cmp);
        for pair in ys.windows(2) {
            let (Some(a), Some(b)) = (pair.first(), pair.get(1)) else {
                continue;
            };
            assert!(b - a >= 12.0, "rows at {a} and {b} overlap");
        }
    }

    #[test]
    fn the_description_column_clears_the_longest_key() {
        // The number this function exists to get right. Eight apps each
        // guessed it -- 176, 196, 206, 216 -- from the longest key label
        // somebody eyeballed, and a row like "Ctrl+Shift+PageDown" runs under
        // its own description in the ones that guessed low.
        let placed = texts(&drawn());
        let longest = "Ctrl+Shift+PageDown";
        let (key_x, _, _) = placed
            .iter()
            .find(|(_, _, t)| t == longest)
            .expect("the long key is drawn")
            .clone();
        let width = crate::text::measure(longest, 11.0, crate::render::FontWeightHint::Bold);
        for (x, _, t) in &placed {
            if ROWS.iter().any(|(_, w)| w == t) {
                assert!(
                    *x >= key_x + width,
                    "the description {t:?} starts at {x}, inside a key ending at {}",
                    key_x + width
                );
            }
        }
    }

    #[test]
    fn the_card_stays_inside_the_window_and_clear_of_the_toolbar() {
        let mut cmds = Vec::new();
        render_card(
            &mut cmds,
            &Palette::for_mode(false),
            (1280.0, 720.0),
            40.0,
            ROWS,
            "x",
        );
        for cmd in &cmds {
            if let RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                ..
            } = cmd
            {
                assert!(*x >= -0.01 && *y >= 40.0 - 0.01, "card at {x},{y}");
                assert!(x + width <= 1280.01, "card runs off the right");
                assert!(y + height <= 720.01, "card runs off the bottom");
            }
        }
    }

    #[test]
    fn a_short_window_still_gets_a_card_below_the_toolbar() {
        // The centring is `max(keep_clear)`, so a window shorter than the card
        // pins it under the toolbar rather than drawing it over one.
        let mut cmds = Vec::new();
        render_card(
            &mut cmds,
            &Palette::for_mode(false),
            (400.0, 120.0),
            40.0,
            ROWS,
            "x",
        );
        let placed = texts(&cmds);
        assert!(!placed.is_empty(), "nothing was drawn at all");
        for (_, y, t) in &placed {
            assert!(*y >= 40.0, "{t:?} was drawn at {y}, over the toolbar");
        }
    }

    #[test]
    fn a_single_letter_is_that_key() {
        assert_eq!(keys("T"), vec![Key::T]);
        // There is no lower-case key, so a lower-case cap is the same one.
        assert_eq!(keys("t"), vec![Key::T]);
        assert_eq!(keys("A"), vec![Key::A]);
        assert_eq!(keys("Z"), vec![Key::Z]);
    }

    #[test]
    fn every_letter_and_digit_maps_to_its_own_key() {
        // A table written out by hand is a table that can be off by one in the
        // middle, where no test that spot-checks the ends would see it.
        let letters = keys("A/B/C/D/E/F/G/H/I/J/K/L/M/N/O/P/Q/R/S/T/U/V/W/X/Y/Z");
        assert_eq!(letters.len(), 26);
        assert_eq!(letters.first(), Some(&Key::A));
        assert_eq!(letters.get(12), Some(&Key::M));
        assert_eq!(letters.last(), Some(&Key::Z));
        let mut unique = letters.clone();
        unique.sort_by_key(|k| format!("{k:?}"));
        unique.dedup();
        assert_eq!(unique.len(), 26, "two letters share a key");

        let digits = keys("0/1/2/3/4/5/6/7/8/9");
        assert_eq!(digits.first(), Some(&Key::Num0));
        assert_eq!(digits.get(7), Some(&Key::Num7));
        assert_eq!(digits.last(), Some(&Key::Num9));
    }

    #[test]
    fn a_range_is_every_key_between_its_ends() {
        assert_eq!(
            keys("A-F"),
            vec![Key::A, Key::B, Key::C, Key::D, Key::E, Key::F]
        );
        assert_eq!(keys("0-9").len(), 10);
        assert_eq!(keys("0-9").first(), Some(&Key::Num0));
        assert_eq!(keys("0-9").last(), Some(&Key::Num9));
        assert_eq!(keys("1-8").len(), 8, "the shape apps print for levels");
        // A hex editor's byte entry, which is what this was written for.
        assert_eq!(keys("0-9, A-F").len(), 16);
    }

    #[test]
    fn a_comma_separates_alternatives_like_a_slash() {
        assert_eq!(keys("A, B"), vec![Key::A, Key::B]);
        assert_eq!(keys("Home, End"), vec![Key::Home, Key::End]);
        // ...but a comma that *is* the key still is one, by the same rule that
        // saves the slash: nothing but blanks before it.
        assert_eq!(keys(","), vec![Key::Comma]);
        assert_eq!(keys("Ctrl+,"), vec![Key::Comma]);
    }

    #[test]
    fn a_dash_that_is_not_a_range_is_still_the_minus_key() {
        assert_eq!(keys("-"), vec![Key::Minus]);
        assert_eq!(keys("Ctrl+-"), vec![Key::Minus]);
        assert_eq!(keys("Ctrl+= / Ctrl+-"), vec![Key::Equals, Key::Minus]);
    }

    #[test]
    fn a_backwards_or_mixed_range_is_refused_rather_than_silently_short() {
        // Returning nothing for these would be this module's own failure mode:
        // a guard test handed a shorter list than it asked for passes while
        // checking less.
        for bad in ["9-0", "F-A", "A-3", "3-A"] {
            assert!(keystrokes(bad).is_err(), "{bad:?} was accepted as a range");
        }
    }

    #[test]
    fn the_function_keys_run_from_one_to_twelve() {
        assert_eq!(keys("F1"), vec![Key::F1]);
        assert_eq!(keys("F2"), vec![Key::F2]);
        assert_eq!(keys("F12"), vec![Key::F12]);
        // Off the end of the row rather than silently F1.
        assert!(keystrokes("F13").is_err());
        assert!(keystrokes("F0").is_err());
    }

    #[test]
    fn a_slash_separates_alternatives_with_or_without_spaces() {
        assert_eq!(keys("Left / Right"), vec![Key::Left, Key::Right]);
        assert_eq!(keys("Up/Down"), vec![Key::Up, Key::Down]);
        assert_eq!(keys("S / O / L / A"), vec![Key::S, Key::O, Key::L, Key::A]);
    }

    #[test]
    fn arrows_is_one_word_for_four_keys() {
        assert_eq!(
            keys("Arrows"),
            vec![Key::Left, Key::Right, Key::Up, Key::Down]
        );
        assert_eq!(keys("arrows").len(), 4);
    }

    #[test]
    fn wasd_is_one_word_for_four_keys() {
        assert_eq!(keys("WASD"), vec![Key::W, Key::A, Key::S, Key::D]);
        assert_eq!(keys("wasd").len(), 4);
        // The shape a game actually prints: both halves of one movement row.
        assert_eq!(
            keys("Arrows / WASD"),
            vec![
                Key::Left,
                Key::Right,
                Key::Up,
                Key::Down,
                Key::W,
                Key::A,
                Key::S,
                Key::D
            ]
        );
    }

    #[test]
    fn modifiers_are_read_off_the_front() {
        let one = keystrokes("Ctrl+Shift+T").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(one.len(), 1);
        let Some(stroke) = one.first() else {
            panic!("no stroke");
        };
        assert_eq!(stroke.key, Key::T);
        assert!(stroke.modifiers.ctrl);
        assert!(stroke.modifiers.shift);
        assert!(!stroke.modifiers.alt);
        assert!(stroke.pressed);

        for spelling in ["Alt+X", "alt+X", "Option+X"] {
            let strokes = keystrokes(spelling).unwrap_or_else(|e| panic!("{e}"));
            assert!(
                strokes.first().is_some_and(|s| s.modifiers.alt),
                "{spelling} lost its modifier"
            );
        }
        for spelling in ["Super+X", "Cmd+X", "Win+X", "Meta+X"] {
            let strokes = keystrokes(spelling).unwrap_or_else(|e| panic!("{e}"));
            assert!(
                strokes.first().is_some_and(|s| s.modifiers.super_key),
                "{spelling} lost its modifier"
            );
        }
    }

    #[test]
    fn a_modifier_applies_to_each_alternative_that_names_it() {
        let both = keystrokes("Ctrl+PageUp / Ctrl+PageDown").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(both.len(), 2);
        assert!(both.iter().all(|s| s.modifiers.ctrl));
        assert_eq!(both.first().map(|s| s.key), Some(Key::PageUp));
        assert_eq!(both.last().map(|s| s.key), Some(Key::PageDown));

        // And is *not* carried over to an alternative that does not: apps write
        // "Ctrl+C / Ctrl+V" precisely because they cannot write "Ctrl+C / V".
        let mixed = keystrokes("Ctrl+C / V").unwrap_or_else(|e| panic!("{e}"));
        assert!(mixed.first().is_some_and(|s| s.modifiers.ctrl));
        assert!(mixed.last().is_some_and(|s| !s.modifiers.ctrl));
    }

    #[test]
    fn question_mark_is_shift_and_slash() {
        let strokes = keystrokes("?").unwrap_or_else(|e| panic!("{e}"));
        let Some(stroke) = strokes.first() else {
            panic!("no stroke");
        };
        assert_eq!(stroke.key, Key::Slash);
        assert!(
            stroke.modifiers.shift,
            "a slash with no shift is a different key press"
        );
    }

    #[test]
    fn a_slash_straight_after_a_plus_is_the_key_not_a_separator() {
        let strokes = keystrokes("Ctrl+/").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            strokes.len(),
            1,
            "the label was split where it should not be"
        );
        assert_eq!(strokes.first().map(|s| s.key), Some(Key::Slash));
        assert!(strokes.first().is_some_and(|s| s.modifiers.ctrl));
    }

    #[test]
    fn a_trailing_plus_is_the_plus_key() {
        let strokes = keystrokes("Ctrl++").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(strokes.len(), 1);
        assert_eq!(strokes.first().map(|s| s.key), Some(Key::Equals));
        assert!(strokes.first().is_some_and(|s| s.modifiers.ctrl));
        assert!(strokes.first().is_some_and(|s| s.modifiers.shift));
    }

    #[test]
    fn the_short_spellings_apps_actually_print_are_understood() {
        assert_eq!(keys("Esc"), keys("Escape"));
        assert_eq!(keys("Del"), keys("Delete"));
        assert_eq!(keys("PgUp"), keys("PageUp"));
        assert_eq!(keys("PgDn"), keys("PageDown"));
        assert_eq!(keys("Return"), keys("Enter"));
        // And the spacing and casing apps vary on.
        assert_eq!(keys("page up"), keys("PageUp"));
        assert_eq!(keys("PAGEUP"), keys("PageUp"));
    }

    #[test]
    fn an_unreadable_part_is_an_error_naming_both_it_and_the_row() {
        let Err(UnknownKey { label, part }) = keystrokes("Left / Esk") else {
            panic!("a misspelled key was accepted");
        };
        assert_eq!(part, "Esk");
        assert_eq!(label, "Left / Esk", "the error should name the whole row");

        // Not a shorter list: a guard test handed one stroke where it asked for
        // two would pass while checking half of what it meant to.
        assert!(keystrokes("Ctrl+Nope").is_err());
        assert!(keystrokes("Hyper+X").is_err(), "unknown modifier accepted");
    }

    #[test]
    fn an_empty_label_is_an_error_rather_than_an_empty_list() {
        // The whole point of the type: nothing to press is never a pass.
        for empty in ["", "   ", "  "] {
            let Err(e) = keystrokes(empty) else {
                panic!("{empty:?} was accepted");
            };
            assert!(e.part.is_empty());
            assert_eq!(e.to_string(), "a shortcut label names no key at all");
        }
    }

    #[test]
    fn a_slash_with_nothing_before_it_is_the_slash_key() {
        // `apps/rssreader` prints "Ctrl+F / /" for search, and the last part of
        // that is a key. Read as a separator it vanishes, and a guard test then
        // checks one key where it meant to check two -- passing, while covering
        // half of what it was asked to.
        assert_eq!(keys("Ctrl+F / /"), vec![Key::F, Key::Slash]);
        assert_eq!(keys("/"), vec![Key::Slash]);
        assert_eq!(keys(" / "), vec![Key::Slash]);
        let strokes = keystrokes("Ctrl+F / /").unwrap_or_else(|e| panic!("{e}"));
        assert!(strokes.first().is_some_and(|s| s.modifiers.ctrl));
        assert!(
            strokes.last().is_some_and(|s| !s.modifiers.ctrl),
            "the modifier leaked onto the alternative"
        );
    }

    #[test]
    fn the_error_reads_as_a_sentence() {
        let Err(e) = keystrokes("Esk") else {
            panic!("accepted");
        };
        assert_eq!(
            e.to_string(),
            "the shortcut label \"Esk\" names \"Esk\", which is not a key this can press"
        );
    }

    #[test]
    fn a_shortcut_carries_no_text() {
        // Text is what typing produces; a shortcut is matched on key and
        // modifiers. A stroke carrying "t" would be a keystroke that inserts.
        let strokes = keystrokes("Ctrl+T / ? / Arrows").unwrap_or_else(|e| panic!("{e}"));
        assert!(strokes.iter().all(|s| s.text.is_empty()));
        assert!(strokes.iter().all(|s| s.pressed));
    }

    /// The five lists that exist in this tree today all parse.
    ///
    /// Not a substitute for each app's own guard test — this says the labels
    /// are readable, not that anything answers them — but a label this cannot
    /// read makes that guard test impossible to write, so it is worth knowing
    /// here rather than five crates away.
    #[test]
    fn every_label_printed_by_an_app_in_this_tree_parses() {
        let printed = [
            // minesweeper
            "Arrows",
            "Space",
            "F",
            "C",
            "D",
            "N",
            // mixer
            "Left/Right",
            "Tab",
            "Up/Down",
            "M",
            "O",
            "I",
            "Enter",
            "Esc",
            // wordsearch
            "H",
            "F2",
            // rssreader, whose twenty-one hints are why this corpus is read off
            // every app rather than the ones that came to mind: it is the only
            // app printing "Ctrl+F / /", and that label is what caught the
            // splitter dropping a key.
            "J / Down",
            "K / Up",
            "Shift+J",
            "Shift+K",
            "Tab / Shift+Tab",
            "R / Enter",
            "Ctrl+F / /",
            "Shift+R",
            "Ctrl+R",
            "V",
            "Space",
            "Ctrl+O",
            "Ctrl+S",
            // slides
            "Left / Right",
            "Home / End",
            "1 / 2",
            "Ctrl+N",
            "Ctrl+D",
            "Ctrl+C / Ctrl+V",
            "Ctrl+PageUp / Ctrl+PageDown",
            "T",
            "S / O / L / A",
            "Enter / F2",
            "Delete",
            "Shift+Delete",
            "Ctrl+T",
            "Ctrl+Shift+T",
            "Ctrl+R",
            "B",
            "Ctrl+E",
            "?",
        ];
        for label in printed {
            assert!(
                keystrokes(label).is_ok(),
                "{label:?} is printed by an app and cannot be read back"
            );
        }
    }
}
