//! Dictionary and thesaurus — look a word up, read it, keep it.
//!
//! Five screens (search, entry, history, favourites, featured word) over a
//! built-in word list, in a real window: every tab, result row, list row and
//! button is clickable, and the keyboard reaches all of it.
//!
//! # What wiring this up found
//!
//! The program drew five screens and could not be used, because `main` built a
//! `DictionaryApp`, dropped it and exited. Nothing below was reachable to
//! notice until it had a window on it.
//!
//! 1. **There was no pointer input at all.** Not a partial hit test — no hit
//!    test. The tab bar, the result rows, the history and favourites rows, the
//!    favourite button and the "[Enter] View full entry" prompt were painted
//!    and inert, so every one of them was a keyboard-only control wearing the
//!    costume of a button.
//! 2. **The status bar advertised a key that could not work.** It read
//!    `[/] Search`, and the handler was `"/" | "f" if ctrl` — a guard on a
//!    `|` pattern applies to *both* alternatives, so plain `/` fell through to
//!    `_ => {}` and only Ctrl+/ did anything.
//! 3. **A selection could walk off the bottom of the list.** `Down` moved
//!    `selected_result` up to `len - 1` while the renderer drew
//!    `.take(visible)` rows from the top and nothing ever scrolled, so on any
//!    search with more hits than fit, the selection left the screen and
//!    `Enter` opened an entry the user could not see.
//! 4. **The favourites list had no keys at all.** `Up`/`Down` were handled for
//!    `Screen::History` and never for `Screen::Favorites`, so
//!    `favorites_scroll` was frozen at 0: `Enter` on the favourites screen
//!    always opened the first favourite, whichever row you thought you were
//!    on.
//! 5. **`detail_scroll` was written and never read.** `Up`/`Down` on the entry
//!    screen incremented it, `render_detail` ignored it, and the sections were
//!    guarded by `dy + 20.0 < y + h` — so on a long entry the etymology was
//!    silently dropped and no key could bring it back.
//! 6. **The layout was a constant.** `width`/`height` were set to 800x600 in
//!    `new` and never assigned again: there was no resize path, and `render`
//!    took no size. Five 110px tabs needed 574px, the status bar drew its
//!    right-hand hint at `width - 250` (a negative x below 250px wide), and
//!    the word-of-the-day card was 100px tall with its prompt drawn at
//!    `y + 152`, outside it.
//! 7. **The word of the day never changed.** `word_of_day_index` was
//!    initialised to `0` with the comment "First word as word of the day" and
//!    nothing ever wrote it, so it was permanently the first entry. There is
//!    no wall clock behind a window yet (see `known-issues.md`
//!    `C-NO-CALENDAR-CLOCK-FOR-APPS`), so it is now a *featured* word the
//!    reader can step through, which is what the code can honestly do.
//! 8. **A blanket `#![allow(dead_code)]` sat on line 1** and was hiding
//!    `is_favorite`, `PartOfSpeech::short` and `DictEntry::related` — all
//!    written, none reached. They are used or gone.
//! 9. **Escape in the search box left a state with no way out but Escape.** It
//!    cleared `search_active` while leaving `screen == Search`, and the typing
//!    branch is gated on both, so the keyboard did nothing until Escape was
//!    pressed a second time.
//! 10. **The doc claimed "200+ common words" over a list of fifteen.** The
//!     claim is gone and the list is now thirty, every one of them written out
//!     rather than counted. Seven of the additions are there because
//!     `PartOfSpeech` has ten variants and the old list used three: a part of
//!     speech no entry carries is a label with nothing behind it.
//! 11. **The cross-references pointed nowhere.** Making the chips clickable
//!     exposed the data behind them: of the 265 words the entries named in
//!     their `related` lists, exactly one was a word this dictionary has, so
//!     the entry screen's only navigation feature got you out of one entry in
//!     thirty. The lists now name siblings on purpose — 57 live links, every
//!     entry reaching at least one other.
//! 12. **The search field's hit box could not do anything.** It answered a
//!     click with `Go(Search)`, and it is drawn only on the search screen, so
//!     every click that could reach it was a no-op — fault one's mistake made
//!     a second time by the rewrite that was meant to end it. A field with no
//!     caret cannot be focused, so it now says so, and says what to press.
//!
//! The search was also quadratic for no reason: five sequential passes over
//! the whole dictionary, each asking `results.contains(&i)` about every entry
//! it looked at, to produce an order that one pass and a stable sort produce
//! exactly. Doing it in one pass made room for the two fields the old search
//! never looked at — antonyms, and `related`, which was stored on every entry
//! and read by nothing.
//!
//! Faults 11 and 12 were found by breaking this file on purpose and checking
//! that the test which claims to cover each piece is the one that fails —
//! `mutate.py` beside this file runs the twelve mutants. Two of them survived
//! the first sweep, and neither survivor was a weak test: they were a control
//! wired to nothing (12) and a clip that had nothing to trim, because the list
//! stopped on an exact row boundary and left a blank strip below it. The list
//! now draws one row into that strip so a list continuing below the fold looks
//! like it does, and the clip cuts that row's hit box off at the pane's edge,
//! so the half that was never drawn cannot be clicked.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;

// ── Catppuccin Mocha palette ───────────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

/// The size the window asks for. Everything is derived from the size the
/// compositor actually gives, which is not required to be this one.
const WINDOW_WIDTH: f32 = 860.0;
const WINDOW_HEIGHT: f32 = 640.0;

// ── Part of Speech ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartOfSpeech {
    Noun,
    Verb,
    Adjective,
    Adverb,
    Pronoun,
    Preposition,
    Conjunction,
    Interjection,
    Determiner,
    Abbreviation,
}

impl PartOfSpeech {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Noun => "noun",
            Self::Verb => "verb",
            Self::Adjective => "adjective",
            Self::Adverb => "adverb",
            Self::Pronoun => "pronoun",
            Self::Preposition => "preposition",
            Self::Conjunction => "conjunction",
            Self::Interjection => "interjection",
            Self::Determiner => "determiner",
            Self::Abbreviation => "abbreviation",
        }
    }

    #[must_use]
    pub fn short(self) -> &'static str {
        match self {
            Self::Noun => "n.",
            Self::Verb => "v.",
            Self::Adjective => "adj.",
            Self::Adverb => "adv.",
            Self::Pronoun => "pron.",
            Self::Preposition => "prep.",
            Self::Conjunction => "conj.",
            Self::Interjection => "interj.",
            Self::Determiner => "det.",
            Self::Abbreviation => "abbr.",
        }
    }

    #[must_use]
    pub fn color(self) -> Color {
        match self {
            Self::Noun => BLUE,
            Self::Verb => GREEN,
            Self::Adjective => PEACH,
            Self::Adverb => YELLOW,
            Self::Pronoun => TEAL,
            Self::Preposition => MAUVE,
            Self::Conjunction => LAVENDER,
            Self::Interjection => RED,
            Self::Determiner => SUBTEXT0,
            Self::Abbreviation => OVERLAY0,
        }
    }
}

// ── Dictionary Entry ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Definition {
    pub part_of_speech: PartOfSpeech,
    pub text: String,
    pub example: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DictEntry {
    pub word: String,
    pub pronunciation: String,
    pub definitions: Vec<Definition>,
    pub synonyms: Vec<String>,
    pub antonyms: Vec<String>,
    pub etymology: String,
    pub related: Vec<String>,
}

// ── Built-in Dictionary ────────────────────────────────────────────────────

#[must_use]
pub fn build_dictionary() -> Vec<DictEntry> {
    vec![
        DictEntry {
            word: "algorithm".into(),
            pronunciation: "/ˈælɡəˌrɪðəm/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Noun,
                    text: "A process or set of rules to be followed in calculations or problem-solving operations.".into(),
                    example: Some("The search algorithm finds the shortest path.".into()),
                },
            ],
            synonyms: vec!["procedure".into(), "method".into(), "process".into(), "routine".into()],
            antonyms: vec![],
            etymology: "From Latin 'algorithmus', from al-Khwarizmi, 9th-century Persian mathematician.".into(),
            related: vec!["computation".into(), "heuristic".into(), "program".into(), "iterate".into(), "cache".into(), "compile".into()],
        },
        DictEntry {
            word: "kernel".into(),
            pronunciation: "/ˈkɜːrnəl/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Noun,
                    text: "The central or most important part of something.".into(),
                    example: Some("The kernel of the argument was simple.".into()),
                },
                Definition {
                    part_of_speech: PartOfSpeech::Noun,
                    text: "The core component of an operating system that manages hardware and system resources.".into(),
                    example: Some("The kernel handles memory allocation and process scheduling.".into()),
                },
                Definition {
                    part_of_speech: PartOfSpeech::Noun,
                    text: "The softer, usually edible part inside the shell of a nut or seed.".into(),
                    example: Some("Crack the walnut to get at the kernel.".into()),
                },
            ],
            synonyms: vec!["core".into(), "nucleus".into(), "heart".into(), "center".into(), "essence".into()],
            antonyms: vec!["periphery".into(), "shell".into(), "exterior".into()],
            etymology: "Old English 'cyrnel', diminutive of 'corn' (seed, grain).".into(),
            related: vec!["microkernel".into(), "monolithic".into(), "operating system".into(), "concurrency".into(), "cache".into()],
        },
        DictEntry {
            word: "compile".into(),
            pronunciation: "/kəmˈpaɪl/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Verb,
                    text: "To produce a set of machine-code instructions from source code.".into(),
                    example: Some("It takes 30 seconds to compile the project.".into()),
                },
                Definition {
                    part_of_speech: PartOfSpeech::Verb,
                    text: "To collect and assemble information from various sources.".into(),
                    example: Some("She compiled a list of references.".into()),
                },
            ],
            synonyms: vec!["build".into(), "assemble".into(), "collect".into(), "translate".into()],
            antonyms: vec!["interpret".into(), "disassemble".into(), "scatter".into()],
            etymology: "Latin 'compilare' — to plunder, collect.".into(),
            related: vec!["compiler".into(), "linker".into(), "source code".into(), "algorithm".into(), "verbose".into(), "API".into()],
        },
        DictEntry {
            word: "ephemeral".into(),
            pronunciation: "/ɪˈfɛmərəl/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Adjective,
                    text: "Lasting for a very short time.".into(),
                    example: Some("The ephemeral beauty of cherry blossoms.".into()),
                },
            ],
            synonyms: vec!["fleeting".into(), "transient".into(), "momentary".into(), "brief".into(), "short-lived".into()],
            antonyms: vec!["permanent".into(), "eternal".into(), "lasting".into(), "enduring".into()],
            etymology: "Greek 'ephemeros' — lasting only a day.".into(),
            related: vec!["temporary".into(), "impermanent".into(), "obsolete".into(), "immutable".into()],
        },
        DictEntry {
            word: "ubiquitous".into(),
            pronunciation: "/juːˈbɪkwɪtəs/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Adjective,
                    text: "Present, appearing, or found everywhere.".into(),
                    example: Some("Smartphones have become ubiquitous in modern life.".into()),
                },
            ],
            synonyms: vec!["omnipresent".into(), "universal".into(), "pervasive".into(), "widespread".into()],
            antonyms: vec!["rare".into(), "scarce".into(), "uncommon".into()],
            etymology: "Latin 'ubique' — everywhere.".into(),
            related: vec!["prevalent".into(), "commonplace".into(), "obsolete".into(), "every".into()],
        },
        DictEntry {
            word: "concurrency".into(),
            pronunciation: "/kənˈkʌrənsi/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Noun,
                    text: "The ability of different parts of a program to be executed out-of-order or simultaneously.".into(),
                    example: Some("Rust's ownership system prevents data races in concurrency.".into()),
                },
                Definition {
                    part_of_speech: PartOfSpeech::Noun,
                    text: "The fact of two or more events happening at the same time.".into(),
                    example: Some("The concurrency of the two festivals created traffic problems.".into()),
                },
            ],
            synonyms: vec!["parallelism".into(), "simultaneity".into(), "coexistence".into()],
            antonyms: vec!["sequential".into(), "serial".into()],
            etymology: "Latin 'concurrere' — to run together.".into(),
            related: vec!["thread".into(), "async".into(), "mutex".into(), "parallelism".into(), "kernel".into(), "iterate".into()],
        },
        DictEntry {
            word: "resilient".into(),
            pronunciation: "/rɪˈzɪliənt/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Adjective,
                    text: "Able to withstand or recover quickly from difficult conditions.".into(),
                    example: Some("The resilient community rebuilt after the storm.".into()),
                },
                Definition {
                    part_of_speech: PartOfSpeech::Adjective,
                    text: "Able to spring back into shape after bending or stretching.".into(),
                    example: Some("A resilient material that returns to its original form.".into()),
                },
            ],
            synonyms: vec!["tough".into(), "hardy".into(), "adaptable".into(), "flexible".into()],
            antonyms: vec!["fragile".into(), "brittle".into(), "vulnerable".into()],
            etymology: "Latin 'resilire' — to spring back.".into(),
            related: vec!["resilience".into(), "robust".into(), "durable".into(), "tenacious".into(), "ephemeral".into()],
        },
        DictEntry {
            word: "pragmatic".into(),
            pronunciation: "/præɡˈmætɪk/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Adjective,
                    text: "Dealing with things sensibly and realistically, based on practical considerations.".into(),
                    example: Some("A pragmatic approach to solving the problem.".into()),
                },
            ],
            synonyms: vec!["practical".into(), "realistic".into(), "sensible".into(), "down-to-earth".into()],
            antonyms: vec!["idealistic".into(), "impractical".into(), "theoretical".into()],
            etymology: "Greek 'pragmatikos' — relating to fact, from 'pragma' (deed).".into(),
            related: vec!["pragmatism".into(), "utilitarian".into(), "meticulous".into(), "lucid".into()],
        },
        DictEntry {
            word: "serendipity".into(),
            pronunciation: "/ˌsɛrənˈdɪpɪti/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Noun,
                    text: "The occurrence of events by chance in a happy or beneficial way.".into(),
                    example: Some("Finding that book was pure serendipity.".into()),
                },
            ],
            synonyms: vec!["luck".into(), "fortune".into(), "chance".into(), "happenstance".into()],
            antonyms: vec!["misfortune".into(), "design".into(), "plan".into()],
            etymology: "Coined by Horace Walpole in 1754, from the fairy tale 'The Three Princes of Serendip'.".into(),
            related: vec!["coincidence".into(), "providence".into(), "eureka".into()],
        },
        DictEntry {
            word: "paradigm".into(),
            pronunciation: "/ˈpærəˌdaɪm/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Noun,
                    text: "A typical example or pattern of something; a model.".into(),
                    example: Some("The shift to object-oriented programming was a paradigm change.".into()),
                },
                Definition {
                    part_of_speech: PartOfSpeech::Noun,
                    text: "A worldview underlying theories and methodology of a scientific subject.".into(),
                    example: Some("The Copernican paradigm replaced the geocentric model.".into()),
                },
            ],
            synonyms: vec!["model".into(), "pattern".into(), "framework".into(), "archetype".into()],
            antonyms: vec!["anomaly".into()],
            etymology: "Greek 'paradeigma' — pattern, example.".into(),
            related: vec!["paradigm shift".into(), "framework".into(), "methodology".into(), "obsolete".into(), "ubiquitous".into()],
        },
        DictEntry {
            word: "iterate".into(),
            pronunciation: "/ˈɪtəˌreɪt/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Verb,
                    text: "To perform or utter repeatedly.".into(),
                    example: Some("We iterate over the collection to process each item.".into()),
                },
            ],
            synonyms: vec!["repeat".into(), "loop".into(), "cycle".into(), "reiterate".into()],
            antonyms: vec![],
            etymology: "Latin 'iterare' — to do again, from 'iterum' (again).".into(),
            related: vec!["iteration".into(), "iterator".into(), "recursive".into(), "algorithm".into(), "concurrency".into()],
        },
        DictEntry {
            word: "verbose".into(),
            pronunciation: "/vɜːrˈboʊs/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Adjective,
                    text: "Using or expressed in more words than are needed.".into(),
                    example: Some("The verbose error messages made debugging easier.".into()),
                },
            ],
            synonyms: vec!["wordy".into(), "long-winded".into(), "prolix".into(), "loquacious".into()],
            antonyms: vec!["concise".into(), "terse".into(), "brief".into(), "succinct".into()],
            etymology: "Latin 'verbosus' — full of words, from 'verbum' (word).".into(),
            related: vec!["verbosity".into(), "loquacity".into(), "lucid".into(), "ambiguous".into()],
        },
        DictEntry {
            word: "immutable".into(),
            pronunciation: "/ɪˈmjuːtəbəl/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Adjective,
                    text: "Unchanging over time or unable to be changed.".into(),
                    example: Some("In Rust, variables are immutable by default.".into()),
                },
            ],
            synonyms: vec!["unchangeable".into(), "fixed".into(), "permanent".into(), "constant".into()],
            antonyms: vec!["mutable".into(), "changeable".into(), "variable".into()],
            etymology: "Latin 'immutabilis' — unchangeable.".into(),
            related: vec!["mutable".into(), "const".into(), "readonly".into(), "ephemeral".into(), "cache".into()],
        },
        DictEntry {
            word: "cache".into(),
            pronunciation: "/kæʃ/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Noun,
                    text: "A hardware or software component that stores data for faster future access.".into(),
                    example: Some("The L1 cache provides the fastest memory access.".into()),
                },
                Definition {
                    part_of_speech: PartOfSpeech::Noun,
                    text: "A collection of items stored in a hidden or secure place.".into(),
                    example: Some("A cache of weapons was found in the basement.".into()),
                },
                Definition {
                    part_of_speech: PartOfSpeech::Verb,
                    text: "To store data in a cache for quick retrieval.".into(),
                    example: Some("The browser caches web pages for faster loading.".into()),
                },
            ],
            synonyms: vec!["store".into(), "buffer".into(), "repository".into(), "stash".into()],
            antonyms: vec![],
            etymology: "French 'cache' — hiding place, from 'cacher' (to hide).".into(),
            related: vec!["buffer".into(), "memory".into(), "L1".into(), "L2".into(), "kernel".into(), "algorithm".into()],
        },
        DictEntry {
            word: "encrypt".into(),
            pronunciation: "/ɪnˈkrɪpt/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Verb,
                    text: "To convert data into a coded form to prevent unauthorized access.".into(),
                    example: Some("Always encrypt sensitive data before transmission.".into()),
                },
            ],
            synonyms: vec!["encode".into(), "cipher".into(), "scramble".into(), "encipher".into()],
            antonyms: vec!["decrypt".into(), "decode".into(), "decipher".into()],
            etymology: "Greek 'en-' + 'kryptos' (hidden).".into(),
            related: vec!["encryption".into(), "AES".into(), "RSA".into(), "cryptography".into(), "API".into(), "kernel".into()],
        },
        DictEntry {
            word: "lucid".into(),
            pronunciation: "/ˈluːsɪd/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Adjective,
                    text: "Expressed clearly; easy to understand.".into(),
                    example: Some("A lucid account of how the scheduler picks a thread.".into()),
                },
                Definition {
                    part_of_speech: PartOfSpeech::Adjective,
                    text: "Able to think clearly, especially between periods of confusion.".into(),
                    example: Some("He had a lucid hour in the afternoon.".into()),
                },
            ],
            synonyms: vec!["clear".into(), "plain".into(), "intelligible".into(), "coherent".into()],
            antonyms: vec!["obscure".into(), "muddled".into(), "confused".into()],
            etymology: "Latin 'lucidus' (bright, clear), from 'lucere' (to shine).".into(),
            related: vec!["clarity".into(), "translucent".into(), "elucidate".into(), "ambiguous".into(), "candid".into(), "verbose".into()],
        },
        DictEntry {
            word: "candid".into(),
            pronunciation: "/ˈkændɪd/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Adjective,
                    text: "Truthful and straightforward, especially about something awkward.".into(),
                    example: Some("A candid note in the commit message about what still fails.".into()),
                },
                Definition {
                    part_of_speech: PartOfSpeech::Adjective,
                    text: "Of a photograph: taken informally, without the subject posing.".into(),
                    example: Some("A candid shot of the team mid-argument.".into()),
                },
            ],
            synonyms: vec!["frank".into(), "honest".into(), "forthright".into(), "blunt".into()],
            antonyms: vec!["guarded".into(), "evasive".into(), "diplomatic".into()],
            etymology: "Latin 'candidus' (white, pure), from 'candere' (to shine).".into(),
            related: vec!["candour".into(), "candidate".into(), "incandescent".into(), "lucid".into(), "verbatim".into()],
        },
        DictEntry {
            word: "ambiguous".into(),
            pronunciation: "/æmˈbɪɡjuəs/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Adjective,
                    text: "Open to more than one interpretation; not having one obvious meaning.".into(),
                    example: Some("The specification was ambiguous about which end owns the buffer.".into()),
                },
            ],
            synonyms: vec!["equivocal".into(), "unclear".into(), "vague".into(), "obscure".into()],
            antonyms: vec!["unambiguous".into(), "explicit".into(), "definite".into(), "lucid".into()],
            etymology: "Latin 'ambiguus' (doubtful), from 'ambigere' (to waver), 'ambi-' (both ways) + 'agere' (to drive).".into(),
            related: vec!["ambiguity".into(), "equivocation".into(), "parse".into(), "nuance".into(), "verbose".into()],
        },
        DictEntry {
            word: "meticulous".into(),
            pronunciation: "/məˈtɪkjələs/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Adjective,
                    text: "Showing great attention to detail; very careful and precise.".into(),
                    example: Some("Meticulous accounting of every allocation and its matching free.".into()),
                },
            ],
            synonyms: vec!["thorough".into(), "painstaking".into(), "scrupulous".into(), "exacting".into()],
            antonyms: vec!["careless".into(), "slapdash".into(), "cursory".into()],
            etymology: "Latin 'meticulosus' (fearful), from 'metus' (fear); the sense of careful attention is nineteenth-century.".into(),
            related: vec!["diligence".into(), "rigour".into(), "precision".into(), "pragmatic".into(), "verbatim".into()],
        },
        DictEntry {
            word: "nuance".into(),
            pronunciation: "/ˈnjuːɑːns/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Noun,
                    text: "A subtle difference in meaning, expression or sound.".into(),
                    example: Some("The nuance between committed and reserved memory matters here.".into()),
                },
                Definition {
                    part_of_speech: PartOfSpeech::Verb,
                    text: "To give something subtle shades of difference.".into(),
                    example: Some("The argument is nuanced rather than hedged.".into()),
                },
            ],
            synonyms: vec!["shade".into(), "subtlety".into(), "gradation".into(), "distinction".into()],
            antonyms: vec![],
            etymology: "French 'nuance' (shade of colour), from 'nuer' (to shade), from 'nue' (cloud), from Latin 'nubes'.".into(),
            related: vec!["connotation".into(), "gradation".into(), "register".into(), "ambiguous".into(), "meticulous".into()],
        },
        DictEntry {
            word: "obsolete".into(),
            pronunciation: "/ˈɒbsəliːt/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Adjective,
                    text: "No longer produced or used; out of date.".into(),
                    example: Some("The syscall is obsolete but the table keeps its number.".into()),
                },
                Definition {
                    part_of_speech: PartOfSpeech::Verb,
                    text: "To make something out of date by replacing it.".into(),
                    example: Some("Versioned tables let a new call obsolete an old one without breaking it.".into()),
                },
            ],
            synonyms: vec!["outdated".into(), "superseded".into(), "antiquated".into(), "defunct".into()],
            antonyms: vec!["current".into(), "modern".into(), "prevailing".into()],
            etymology: "Latin 'obsoletus', past participle of 'obsolescere' (to fall into disuse).".into(),
            related: vec!["deprecated".into(), "legacy".into(), "compatibility".into(), "ephemeral".into(), "ubiquitous".into()],
        },
        DictEntry {
            word: "tenacious".into(),
            pronunciation: "/təˈneɪʃəs/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Adjective,
                    text: "Holding firmly to something; not readily letting go.".into(),
                    example: Some("A tenacious bug that survived three rewrites of the allocator.".into()),
                },
                Definition {
                    part_of_speech: PartOfSpeech::Adjective,
                    text: "Persistent in seeking something despite difficulty.".into(),
                    example: Some("She was tenacious about reproducing the fault.".into()),
                },
            ],
            synonyms: vec!["persistent".into(), "dogged".into(), "determined".into(), "stubborn".into()],
            antonyms: vec!["irresolute".into(), "yielding".into(), "fickle".into()],
            etymology: "Latin 'tenax' (holding fast), from 'tenere' (to hold).".into(),
            related: vec!["tenacity".into(), "tenure".into(), "retain".into(), "resilient".into()],
        },
        DictEntry {
            word: "zenith".into(),
            pronunciation: "/ˈzenɪθ/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Noun,
                    text: "The point of the sky directly overhead an observer.".into(),
                    example: Some("The sun reaches its zenith at solar noon.".into()),
                },
                Definition {
                    part_of_speech: PartOfSpeech::Noun,
                    text: "The highest point reached by something; the time of greatest success.".into(),
                    example: Some("Throughput was at its zenith before the lock was added.".into()),
                },
            ],
            synonyms: vec!["peak".into(), "apex".into(), "summit".into(), "pinnacle".into()],
            antonyms: vec!["nadir".into(), "trough".into(), "low point".into()],
            etymology: "Medieval Latin 'cenit', a misreading of Arabic 'samt' in 'samt ar-ras' (path over the head).".into(),
            related: vec!["nadir".into(), "azimuth".into(), "culmination".into(), "paradigm".into()],
        },
        // The seven entries below exist because `PartOfSpeech` has ten
        // variants and the word list used three. A part of speech the
        // dictionary cannot show is a label with nothing behind it — its
        // colour is untested, its abbreviation unreachable, and a reader
        // looking for a preposition finds an empty category. One word each.
        DictEntry {
            word: "verbatim".into(),
            pronunciation: "/vɜːˈbeɪtɪm/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Adverb,
                    text: "In exactly the same words as were used originally.".into(),
                    example: Some("The error is quoted verbatim, punctuation and all.".into()),
                },
                Definition {
                    part_of_speech: PartOfSpeech::Adjective,
                    text: "Corresponding word for word to the original.".into(),
                    example: Some("A verbatim transcript of the session.".into()),
                },
            ],
            synonyms: vec!["exactly".into(), "literally".into(), "word for word".into()],
            antonyms: vec!["loosely".into(), "approximately".into(), "paraphrased".into()],
            etymology: "Medieval Latin 'verbatim', from Latin 'verbum' (word).".into(),
            related: vec!["quotation".into(), "transcript".into(), "literal".into(), "candid".into(), "meticulous".into()],
        },
        DictEntry {
            word: "they".into(),
            pronunciation: "/ðeɪ/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Pronoun,
                    text: "The people or things previously mentioned or easily identified.".into(),
                    example: Some("The tests ran, and they all passed.".into()),
                },
                Definition {
                    part_of_speech: PartOfSpeech::Pronoun,
                    text: "One person of unspecified or non-binary gender.".into(),
                    example: Some("Whoever filed the bug left no note about what they expected.".into()),
                },
            ],
            synonyms: vec!["them".into(), "those".into()],
            antonyms: vec![],
            etymology: "Old Norse 'their', which displaced Old English 'hie' in the thirteenth century.".into(),
            related: vec!["pronoun".into(), "antecedent".into(), "agreement".into(), "every".into()],
        },
        DictEntry {
            word: "via".into(),
            pronunciation: "/ˈvaɪə/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Preposition,
                    text: "Travelling through a place on the way to a destination.".into(),
                    example: Some("The packet reached the host via three routers.".into()),
                },
                Definition {
                    part_of_speech: PartOfSpeech::Preposition,
                    text: "By way of; by means of.".into(),
                    example: Some("The driver talks to the kernel via a channel, not a syscall.".into()),
                },
            ],
            synonyms: vec!["through".into(), "by way of".into(), "using".into()],
            antonyms: vec!["directly".into()],
            etymology: "Latin 'via', the ablative of 'via' (way, road).".into(),
            related: vec!["route".into(), "indirection".into(), "proxy".into(), "albeit".into()],
        },
        DictEntry {
            word: "albeit".into(),
            pronunciation: "/ɔːlˈbiːɪt/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Conjunction,
                    text: "Although; even though.".into(),
                    example: Some("It works, albeit slowly, on a single core.".into()),
                },
            ],
            synonyms: vec!["although".into(), "though".into(), "even if".into()],
            antonyms: vec![],
            etymology: "Middle English 'al be it' — literally 'although it be'.".into(),
            related: vec!["concession".into(), "clause".into(), "conjunction".into(), "via".into()],
        },
        DictEntry {
            word: "eureka".into(),
            pronunciation: "/jʊəˈriːkə/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Interjection,
                    text: "A cry of joy on finding or working something out.".into(),
                    example: Some("Eureka — the fault was the wheel delta being read as pixels.".into()),
                },
                Definition {
                    part_of_speech: PartOfSpeech::Noun,
                    text: "The moment at which something is suddenly understood.".into(),
                    example: Some("The profiler gave us our eureka.".into()),
                },
            ],
            synonyms: vec!["aha".into(), "breakthrough".into(), "revelation".into()],
            antonyms: vec![],
            etymology: "Greek 'heureka' (I have found it), attributed to Archimedes.".into(),
            related: vec!["heuristic".into(), "insight".into(), "discovery".into(), "serendipity".into()],
        },
        DictEntry {
            word: "every".into(),
            pronunciation: "/ˈevri/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Determiner,
                    text: "All the members of a group, considered one at a time.".into(),
                    example: Some("Every unsafe block must carry a safety comment.".into()),
                },
                Definition {
                    part_of_speech: PartOfSpeech::Determiner,
                    text: "Once in each stated interval.".into(),
                    example: Some("The frame clock fires every sixteen milliseconds.".into()),
                },
            ],
            synonyms: vec!["each".into(), "all".into()],
            antonyms: vec!["no".into(), "none".into(), "some".into()],
            etymology: "Old English 'æfre ælc' (ever each), run together in Middle English.".into(),
            related: vec!["quantifier".into(), "determiner".into(), "universal".into(), "ubiquitous".into(), "they".into()],
        },
        DictEntry {
            word: "API".into(),
            pronunciation: "/ˌeɪ piː ˈaɪ/".into(),
            definitions: vec![
                Definition {
                    part_of_speech: PartOfSpeech::Abbreviation,
                    text: "Application programming interface: the set of calls one program offers another.".into(),
                    example: Some("The syscall table is the kernel's API, and it is versioned.".into()),
                },
            ],
            synonyms: vec!["interface".into(), "contract".into()],
            antonyms: vec!["implementation".into(), "internals".into()],
            etymology: "An initialism, in use since the nineteen-sixties.".into(),
            related: vec!["ABI".into(), "syscall".into(), "library".into(), "compatibility".into(), "compile".into(), "encrypt".into()],
        },
    ]
}

// ── Screens ────────────────────────────────────────────────────────────────

/// How many screens the tab bar offers.
pub const SCREENS: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Search,
    Entry,
    History,
    Favorites,
    Featured,
}

impl Screen {
    pub const ALL: [Screen; SCREENS] = [
        Self::Search,
        Self::Entry,
        Self::History,
        Self::Favorites,
        Self::Featured,
    ];

    #[must_use]
    pub fn index(self) -> usize {
        match self {
            Self::Search => 0,
            Self::Entry => 1,
            Self::History => 2,
            Self::Favorites => 3,
            Self::Featured => 4,
        }
    }

    #[must_use]
    pub fn from_index(i: usize) -> Option<Self> {
        Self::ALL.get(i).copied()
    }

    /// The three forms of a tab's caption, widest first.
    ///
    /// Five tabs across a narrow window is the case that broke the old bar:
    /// five fixed 110px cells wanted 574px and simply ran off the edge. A
    /// caption that has a short form can keep its cell instead of overflowing
    /// it, so the bar shrinks with the window rather than escaping it.
    #[must_use]
    pub fn captions(self) -> [&'static str; 3] {
        match self {
            Self::Search => ["Search", "Find", "S"],
            Self::Entry => ["Entry", "Word", "E"],
            Self::History => ["History", "Hist", "H"],
            Self::Favorites => ["Favourites", "Favs", "*"],
            Self::Featured => ["Featured", "Word", "F"],
        }
    }

    /// Whether this screen shows a list of words that can be selected with the
    /// arrow keys and opened with Enter.
    #[must_use]
    pub fn is_list(self) -> bool {
        matches!(self, Self::Search | Self::History | Self::Favorites)
    }
}

/// Everything in the window a click can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// One of the five tabs.
    Tab(usize),
    /// A row of whichever list the current screen is showing, by its position
    /// in that list. The screen decides what the row means, which is why the
    /// three list screens share one variant: only one of them is ever drawn.
    Row(usize),
    /// A cross-reference — a synonym, an antonym or a related word — naming
    /// the dictionary entry at this index. Recorded only for words the
    /// dictionary actually has, so a chip that is clickable is a chip that
    /// leads somewhere.
    Link(usize),
    /// The search field: clicking it puts you on the search screen.
    SearchBox,
    /// Empty the query.
    ClearQuery,
    /// Empty the history list.
    ClearHistory,
    /// Add the open entry to the favourites, or take it out again.
    Favorite,
    /// Leave the entry screen for the one you opened it from.
    Back,
    /// Step the featured word.
    PrevFeatured,
    NextFeatured,
    /// Open the featured word as a full entry.
    OpenFeatured,
}

/// The frame type this program records its hit boxes into.
pub type Frame = guitk::frame::Frame<Target>;

/// One thing the reader can ask for. Every key and every click turns into one
/// of these, so a click and the key that does the same job cannot drift apart.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    /// Show a screen.
    Go(Screen),
    /// Move `n` tabs along, wrapping. The entry tab is skipped when nothing is
    /// open, because a tab that shows an empty screen is a dead stop in a
    /// cycle that is supposed to get you somewhere.
    CycleScreen(isize),
    /// Open the dictionary entry at this index.
    Open(usize),
    /// Open whatever the current list has selected.
    OpenSelected,
    /// Move the selection within the current list.
    Move(Step),
    /// Append a character to the query.
    Type(char),
    /// Drop the last character of the query.
    Backspace,
    ClearQuery,
    ClearHistory,
    /// Add the open entry to the favourites, or take it out again.
    ToggleFavorite,
    /// Leave the entry screen.
    Back,
    /// Scroll the open entry by this many pixels.
    Scroll(f32),
    /// Scroll the current list by this many whole rows, leaving the selection
    /// where it is — which is what a wheel does everywhere else.
    ScrollRows(isize),
    /// Step the featured word by `n`, wrapping.
    StepFeatured(isize),
}

/// How far a selection or a scroll moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Prev,
    Next,
    PageUp,
    PageDown,
    First,
    Last,
}

// ── Layout ─────────────────────────────────────────────────────────────────

/// The share of the window's height the content keeps no matter what.
const CONTENT_SHARE: f32 = 0.55;

/// Which band goes first when they do not all fit: the status bar, then the
/// tab bar.
///
/// The status line goes first because everything it says is also said by the
/// screen it sits under. The tab bar goes second and reluctantly: dropping it
/// costs the pointer its only way between screens, so it survives until the
/// window is too short to draw it and still show a word.
const BAND_DROP_ORDER: [usize; 2] = [1, 0];

/// Every rectangle in the window, derived from the window's own size.
///
/// Built fresh on every frame and never remembered. The old program stored
/// `width`/`height` on the model, set them once to 800x600 and never assigned
/// them again — so every rectangle in the window was a constant, and a click
/// was tested against a layout the window had never had.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    pub tabs: Rect,
    /// Everything between the bars, already inset by `pad`.
    pub content: Rect,
    pub status: Rect,
    pub font: f32,
    pub small: f32,
    pub big: f32,
    pub pad: f32,
}

impl Layout {
    #[must_use]
    pub fn new(width: f32, height: f32) -> Self {
        let w = width.max(1.0);
        let h = height.max(1.0);
        let font = (h / 36.0).clamp(9.0, 16.0);
        let small = (font - 2.0).max(7.0);
        let big = (font * 1.75).clamp(14.0, 30.0);
        let pad = (w.min(h) * 0.02).clamp(3.0, 12.0);

        // What each band would like, in [tabs, status] order.
        let mut wants = [(h * 0.075).clamp(22.0, 40.0), (h * 0.05).clamp(16.0, 26.0)];
        // What is left for the bars once the content has its share *and* the
        // gaps that separate it from them. The padding comes out of this side:
        // charging it to the content is how a promised share becomes less than
        // one, which is the difference between a readable definition and a
        // clipped one in a small window.
        let budget = (h - h * CONTENT_SHARE - pad * 2.0).max(0.0);
        for &i in &BAND_DROP_ORDER {
            if wants.iter().sum::<f32>() <= budget {
                break;
            }
            if let Some(band) = wants.get_mut(i) {
                *band = 0.0;
            }
        }
        let [tab_h, status_h] = wants;

        let tabs = if tab_h > 0.0 {
            Rect::new(0.0, 0.0, w, tab_h)
        } else {
            Rect::EMPTY
        };
        let status = if status_h > 0.0 {
            Rect::new(0.0, h - status_h, w, status_h)
        } else {
            Rect::EMPTY
        };
        let top = if tab_h > 0.0 { tabs.bottom() } else { 0.0 };
        let bottom = if status_h > 0.0 { status.y } else { h };
        let content = Rect::new(
            pad,
            top + pad,
            (w - pad * 2.0).max(0.0),
            (bottom - top - pad * 2.0).max(0.0),
        );

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            tabs,
            content,
            status,
            font,
            small,
            big,
            pad,
        }
    }

    /// A band too short to hold a line of type, or too narrow to hold its
    /// contents, is not drawn at all. Drawing it would spend the height and
    /// show nothing legible for it.
    #[must_use]
    pub fn shows_tabs(&self) -> bool {
        self.tabs.h >= 16.0 && self.tabs.w >= 150.0
    }

    #[must_use]
    pub fn shows_status(&self) -> bool {
        self.status.h >= 11.0 && self.status.w >= 120.0
    }

    /// The `i`th tab's cell, or [`Rect::EMPTY`] when the bar is not drawn.
    #[must_use]
    pub fn tab(&self, i: usize) -> Rect {
        if !self.shows_tabs() || i >= SCREENS {
            return Rect::EMPTY;
        }
        let gap = (self.pad * 0.35).max(1.0);
        let cell = self.tabs.w / SCREENS as f32;
        Rect::new(
            cell * i as f32 + gap,
            self.tabs.y + gap,
            (cell - gap * 2.0).max(0.0),
            (self.tabs.h - gap * 2.0).max(0.0),
        )
    }

    /// The caption that fits `cell`, from the widest form that does, down to a
    /// single character.
    #[must_use]
    pub fn tab_caption(&self, screen: Screen, cell: Rect) -> &'static str {
        let room = (cell.w - self.pad).max(0.0);
        let forms = screen.captions();
        for form in forms {
            if text::measure(form, self.small, FontWeightHint::Bold) <= room {
                return form;
            }
        }
        forms.get(2).copied().unwrap_or("")
    }

    /// The height of one row of a word list.
    #[must_use]
    pub fn row_h(&self) -> f32 {
        (self.font * 2.4).max(14.0)
    }

    /// The search field, at the top of the search screen's content.
    #[must_use]
    pub fn search_box(&self) -> Rect {
        let h = (self.content.h * 0.18).min(self.font * 2.6);
        Rect::new(self.content.x, self.content.y, self.content.w, h.max(0.0))
    }

    /// The pane a word list is drawn in. On the search screen the field sits
    /// above it; on the other two the list has the whole content.
    #[must_use]
    pub fn list_pane(&self, screen: Screen) -> Rect {
        let top = if screen == Screen::Search {
            self.search_box().bottom() + self.pad
        } else {
            self.content.y + self.header_h()
        };
        Rect::new(
            self.content.x,
            top,
            self.content.w,
            (self.content.bottom() - top).max(0.0),
        )
    }

    /// The band above the history and favourites lists, holding their title
    /// and the button that empties them.
    #[must_use]
    pub fn header_h(&self) -> f32 {
        (self.content.h * 0.14).min(self.font * 2.2).max(0.0)
    }

    /// How many whole rows fit in `pane`.
    #[must_use]
    pub fn rows_in(&self, pane: Rect) -> usize {
        if pane.h <= 0.0 {
            return 0;
        }
        let n = (pane.h / self.row_h()).floor();
        if n <= 0.0 { 0 } else { n as usize }
    }

    /// The `slot`th visible row of `pane`, counted from its top edge.
    #[must_use]
    pub fn row(&self, pane: Rect, slot: usize) -> Rect {
        let h = self.row_h();
        Rect::new(pane.x, pane.y + slot as f32 * h, pane.w, h)
    }

    /// The bar at the top of the entry screen: back on the left, the star on
    /// the right.
    #[must_use]
    pub fn entry_bar(&self) -> Rect {
        Rect::new(
            self.content.x,
            self.content.y,
            self.content.w,
            self.header_h(),
        )
    }

    /// The scrolling body of an entry, below its bar.
    #[must_use]
    pub fn entry_pane(&self) -> Rect {
        let top = self.entry_bar().bottom() + self.pad * 0.5;
        Rect::new(
            self.content.x,
            top,
            self.content.w,
            (self.content.bottom() - top).max(0.0),
        )
    }

    /// A small button at the right-hand end of `band`, `slot` places in from
    /// the edge.
    #[must_use]
    pub fn trailing_button(&self, band: Rect, slot: usize, width: f32) -> Rect {
        let h = (band.h - self.pad * 0.5).clamp(0.0, self.font * 2.0);
        let gap = self.pad * 0.5;
        let right = band.right() - slot as f32 * (width + gap);
        Rect::new(
            (right - width).max(band.x),
            band.y + (band.h - h) / 2.0,
            width.min(band.w),
            h,
        )
    }
}

// ── Search ─────────────────────────────────────────────────────────────────

/// How well `entry` answers `query`, lower being better, or `None` when it
/// does not answer it at all.
///
/// The old search made five sequential passes over the whole dictionary, each
/// one asking `results.contains(&i)` for every entry it looked at — quadratic
/// work to produce an order that one pass and a stable sort produce exactly.
/// The rewrite also reaches two fields the old one never searched: antonyms,
/// and the `related` list, which was written, stored, and read by nothing.
///
/// `query` must already be lowercased and non-empty.
#[must_use]
pub fn rank(entry: &DictEntry, query: &str) -> Option<u8> {
    let word = entry.word.to_lowercase();
    if word == query {
        return Some(0);
    }
    if word.starts_with(query) {
        return Some(1);
    }
    if word.contains(query) {
        return Some(2);
    }
    let in_definitions = entry.definitions.iter().any(|d| {
        d.text.to_lowercase().contains(query)
            || d.example
                .as_deref()
                .is_some_and(|e| e.to_lowercase().contains(query))
    });
    if in_definitions {
        return Some(3);
    }
    let hits = |words: &[String]| words.iter().any(|w| w.to_lowercase().contains(query));
    if hits(&entry.synonyms) {
        return Some(4);
    }
    if hits(&entry.antonyms) {
        return Some(5);
    }
    if hits(&entry.related) {
        return Some(6);
    }
    None
}

/// Keep `sel` on screen by moving the window `top`, and nothing else.
///
/// The old program had no such thing, which is fault four and fault five in
/// one: `Down` walked the selection to `len - 1` while the renderer drew
/// `.take(visible)` rows from index zero, so on any list longer than the pane
/// the selection left the screen and `Enter` opened a word the reader could
/// not see.
pub fn scroll_into_view(sel: usize, top: &mut usize, visible: usize) {
    if visible == 0 {
        *top = 0;
        return;
    }
    if sel < *top {
        *top = sel;
    } else if sel >= top.saturating_add(visible) {
        *top = sel.saturating_sub(visible.saturating_sub(1));
    }
}

/// Apply `step` to a selection of `len` items showing `visible` at a time.
#[must_use]
pub fn stepped(sel: usize, step: Step, len: usize, visible: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let last = len.saturating_sub(1);
    let page = visible.max(1);
    match step {
        Step::Prev => sel.saturating_sub(1),
        Step::Next => sel.saturating_add(1).min(last),
        Step::PageUp => sel.saturating_sub(page),
        Step::PageDown => sel.saturating_add(page).min(last),
        Step::First => 0,
        Step::Last => last,
    }
}

// ── The entry, as blocks ───────────────────────────────────────────────────

/// One thing drawn in the scrolling body of an entry.
///
/// The same list serves both the drawing and the scroll clamp, which is the
/// whole point of building it. The old code guarded each section with
/// `dy + 20.0 < y + h` and wrote a `detail_scroll` that nothing ever read, so
/// on a long entry the etymology was dropped silently and no key brought it
/// back.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    /// A run of text already wrapped to one line.
    Line {
        text: String,
        size: f32,
        color: Color,
        weight: FontWeightHint,
        indent: f32,
        /// Blank space above this line.
        space: f32,
    },
    /// One row of word chips. Each carries the dictionary index it names, when
    /// the dictionary has it — that index is what makes the chip clickable.
    Chips {
        words: Vec<(String, Option<usize>)>,
        size: f32,
        color: Color,
        space: f32,
    },
}

impl Block {
    #[must_use]
    pub fn space(&self) -> f32 {
        match self {
            Self::Line { space, .. } | Self::Chips { space, .. } => *space,
        }
    }

    /// How tall this block is drawn, not counting the space above it.
    #[must_use]
    pub fn height(&self) -> f32 {
        match self {
            Self::Line { size, weight, .. } => text::line_height(*size, *weight),
            Self::Chips { size, .. } => text::line_height(*size, FontWeightHint::Regular) * 1.7,
        }
    }
}

/// The height of a whole block list, spaces included.
#[must_use]
pub fn blocks_height(blocks: &[Block]) -> f32 {
    blocks
        .iter()
        .fold(0.0, |acc, b| acc + b.space() + b.height())
}

// ── Model ──────────────────────────────────────────────────────────────────

pub struct Dictionary {
    entries: Vec<DictEntry>,
    query: String,
    /// Indices into `entries`, best match first.
    results: Vec<usize>,
    /// The selected row and the first visible row of each list screen, indexed
    /// by [`Screen::index`]. The entry and featured screens leave their slots
    /// alone; giving every screen its own pair is what stopped the favourites
    /// list from sharing — and losing to — the history list's position.
    sel: [usize; SCREENS],
    top: [usize; SCREENS],
    current: Option<usize>,
    /// Where `Back` goes from the entry screen.
    came_from: Screen,
    /// Words, most recent first.
    history: Vec<String>,
    favorites: Vec<String>,
    featured: usize,
    screen: Screen,
    /// Pixels the open entry is scrolled down by. A pixel offset rather than a
    /// row count because an entry is prose of several sizes, not rows.
    entry_scroll: f32,
    /// The wheel's unspent fraction of a row. A trackpad sends a stream of
    /// fifths of a notch, and a converter that rounded each event on its own
    /// would return zero every time and never scroll at all.
    wheel: guitk::wheel::Accumulator,
    status: String,
    size: (f32, f32),
}

/// The longest history the program keeps.
const HISTORY_LIMIT: usize = 100;

impl Dictionary {
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: build_dictionary(),
            query: String::new(),
            results: Vec::new(),
            sel: [0; SCREENS],
            top: [0; SCREENS],
            current: None,
            came_from: Screen::Search,
            history: Vec::new(),
            favorites: Vec::new(),
            featured: 0,
            screen: Screen::Search,
            entry_scroll: 0.0,
            wheel: guitk::wheel::Accumulator::default(),
            status: "Type a word, or part of one".to_string(),
            size: (WINDOW_WIDTH, WINDOW_HEIGHT),
        }
    }

    /// Remember the size the window is being drawn at, so the next click is
    /// read against the layout the reader actually saw.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.size = (width.max(1.0), height.max(1.0));
    }

    #[must_use]
    pub fn layout(&self) -> Layout {
        Layout::new(self.size.0, self.size.1)
    }

    #[must_use]
    pub fn screen(&self) -> Screen {
        self.screen
    }

    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    #[must_use]
    pub fn status(&self) -> &str {
        &self.status
    }

    #[must_use]
    pub fn current(&self) -> Option<usize> {
        self.current
    }

    #[must_use]
    pub fn featured(&self) -> usize {
        self.featured
    }

    #[must_use]
    pub fn entry_scroll(&self) -> f32 {
        self.entry_scroll
    }

    #[must_use]
    pub fn entries(&self) -> &[DictEntry] {
        &self.entries
    }

    #[must_use]
    pub fn history(&self) -> &[String] {
        &self.history
    }

    #[must_use]
    pub fn favorites(&self) -> &[String] {
        &self.favorites
    }

    #[must_use]
    pub fn entry(&self, index: usize) -> Option<&DictEntry> {
        self.entries.get(index)
    }

    /// The word `index` names, or the empty string.
    #[must_use]
    pub fn word(&self, index: usize) -> &str {
        self.entries.get(index).map_or("", |e| e.word.as_str())
    }

    #[must_use]
    pub fn find_word(&self, word: &str) -> Option<usize> {
        let lower = word.to_lowercase();
        self.entries
            .iter()
            .position(|e| e.word.to_lowercase() == lower)
    }

    /// Whether the open entry is one of the favourites.
    #[must_use]
    pub fn is_favorite(&self) -> bool {
        self.current
            .and_then(|i| self.entries.get(i))
            .is_some_and(|e| self.favorites.contains(&e.word))
    }

    /// The dictionary indices the given list screen shows, in order.
    ///
    /// One function for all three, so a row's meaning cannot depend on which
    /// screen forgot to handle a key.
    #[must_use]
    pub fn rows(&self, screen: Screen) -> Vec<usize> {
        match screen {
            Screen::Search => self.results.clone(),
            Screen::History => self
                .history
                .iter()
                .filter_map(|w| self.find_word(w))
                .collect(),
            Screen::Favorites => self
                .favorites
                .iter()
                .filter_map(|w| self.find_word(w))
                .collect(),
            Screen::Entry | Screen::Featured => Vec::new(),
        }
    }

    #[must_use]
    pub fn selected(&self, screen: Screen) -> usize {
        self.sel.get(screen.index()).copied().unwrap_or(0)
    }

    #[must_use]
    pub fn scroll_top(&self, screen: Screen) -> usize {
        self.top.get(screen.index()).copied().unwrap_or(0)
    }

    /// How many rows the current window shows for `screen`.
    #[must_use]
    pub fn visible_rows(&self, screen: Screen) -> usize {
        let l = self.layout();
        l.rows_in(l.list_pane(screen))
    }

    /// Whether `screen` is worth showing: the entry tab is dead with nothing
    /// open, and `CycleScreen` steps over it rather than landing on a blank.
    #[must_use]
    pub fn reachable(&self, screen: Screen) -> bool {
        screen != Screen::Entry || self.current.is_some()
    }

    // ── Search ─────────────────────────────────────────────────────────────

    fn search(&mut self) {
        self.results.clear();
        self.set_sel(Screen::Search, 0);
        let query = self.query.trim().to_lowercase();
        if query.is_empty() {
            self.status = "Type a word, or part of one".to_string();
            return;
        }
        let mut scored: Vec<(u8, usize)> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| rank(e, &query).map(|r| (r, i)))
            .collect();
        // A stable sort, so entries of equal rank stay in dictionary order.
        scored.sort_by_key(|&(r, _)| r);
        self.results = scored.into_iter().map(|(_, i)| i).collect();
        self.status = match self.results.len() {
            0 => format!("Nothing matches \"{}\"", self.query.trim()),
            1 => format!("One entry matches \"{}\"", self.query.trim()),
            n => format!("{n} entries match \"{}\"", self.query.trim()),
        };
    }

    fn set_sel(&mut self, screen: Screen, value: usize) {
        if let Some(slot) = self.sel.get_mut(screen.index()) {
            *slot = value;
        }
        let visible = self.visible_rows(screen);
        let mut top = self.scroll_top(screen);
        scroll_into_view(value, &mut top, visible);
        if let Some(slot) = self.top.get_mut(screen.index()) {
            *slot = top;
        }
    }

    // ── Doing things ───────────────────────────────────────────────────────

    /// The one place the state changes. Keys and clicks both come through
    /// here, so a control and its shortcut cannot mean different things.
    pub fn apply(&mut self, action: Action) {
        match action {
            Action::Go(screen) => self.go(screen),
            Action::CycleScreen(n) => self.cycle(n),
            Action::Open(index) => self.open(index),
            Action::OpenSelected => {
                let rows = self.rows(self.screen);
                if let Some(&index) = rows.get(self.selected(self.screen)) {
                    self.open(index);
                }
            }
            Action::Move(step) => self.move_selection(step),
            Action::Type(ch) => {
                self.query.push(ch);
                self.search();
                self.screen = Screen::Search;
            }
            Action::Backspace => {
                if self.query.pop().is_some() {
                    self.search();
                }
                self.screen = Screen::Search;
            }
            Action::ClearQuery => {
                if self.query.is_empty() {
                    self.status = "Type a word, or part of one".to_string();
                } else {
                    self.query.clear();
                    self.search();
                }
                self.screen = Screen::Search;
            }
            Action::ClearHistory => {
                self.history.clear();
                self.set_sel(Screen::History, 0);
                self.status = "History cleared".to_string();
            }
            Action::ToggleFavorite => self.toggle_favorite(),
            Action::Back => self.back(),
            Action::Scroll(dy) => self.scroll_entry(dy),
            Action::ScrollRows(n) => self.scroll_rows(n),
            Action::StepFeatured(n) => self.step_featured(n),
        }
    }

    fn go(&mut self, screen: Screen) {
        if !self.reachable(screen) {
            self.status = "Open a word first".to_string();
            return;
        }
        self.screen = screen;
        if screen.is_list() {
            // The window may have changed shape since this list was last seen.
            let sel = self.selected(screen);
            let len = self.rows(screen).len();
            self.set_sel(screen, sel.min(len.saturating_sub(1)));
        }
        self.status = self.screen_status();
    }

    fn cycle(&mut self, n: isize) {
        let mut index = self.screen.index();
        for _ in 0..SCREENS {
            let next = isize::try_from(index).unwrap_or(0).saturating_add(n);
            let wrapped = next.rem_euclid(SCREENS as isize);
            index = usize::try_from(wrapped).unwrap_or(0);
            if let Some(screen) = Screen::from_index(index)
                && self.reachable(screen)
            {
                self.go(screen);
                return;
            }
        }
    }

    fn open(&mut self, index: usize) {
        let Some(entry) = self.entries.get(index) else {
            return;
        };
        let word = entry.word.clone();
        if self.screen != Screen::Entry {
            self.came_from = self.screen;
        }
        self.current = Some(index);
        self.entry_scroll = 0.0;
        self.screen = Screen::Entry;
        self.history.retain(|w| w != &word);
        self.history.insert(0, word.clone());
        self.history.truncate(HISTORY_LIMIT);
        // The history just moved under the selection; put it back on the row
        // the reader was last looking at rather than wherever it now points.
        self.set_sel(Screen::History, 0);
        self.status = format!("{word} — {}", self.summary(index));
    }

    /// A one-line description of an entry, for the status bar.
    fn summary(&self, index: usize) -> String {
        self.entries.get(index).map_or_else(String::new, |e| {
            let senses = e.definitions.len();
            let parts: Vec<&str> = e.definitions.iter().map(|d| d.part_of_speech.short()).fold(
                Vec::new(),
                |mut acc, s| {
                    if !acc.contains(&s) {
                        acc.push(s);
                    }
                    acc
                },
            );
            if senses == 1 {
                format!("1 sense, {}", parts.join(" "))
            } else {
                format!("{senses} senses, {}", parts.join(" "))
            }
        })
    }

    fn back(&mut self) {
        let to = if self.came_from == Screen::Entry {
            Screen::Search
        } else {
            self.came_from
        };
        self.go(to);
    }

    fn toggle_favorite(&mut self) {
        let Some(word) = self
            .current
            .and_then(|i| self.entries.get(i))
            .map(|e| e.word.clone())
        else {
            self.status = "Open a word first".to_string();
            return;
        };
        if self.favorites.contains(&word) {
            self.favorites.retain(|w| w != &word);
            self.status = format!("Removed {word} from favourites");
        } else {
            self.favorites.push(word.clone());
            self.status = format!("Added {word} to favourites");
        }
        let len = self.rows(Screen::Favorites).len();
        let sel = self.selected(Screen::Favorites);
        self.set_sel(Screen::Favorites, sel.min(len.saturating_sub(1)));
    }

    fn move_selection(&mut self, step: Step) {
        if self.screen == Screen::Entry {
            let l = self.layout();
            let line = text::line_height(l.font, FontWeightHint::Regular);
            let page = l.entry_pane().h;
            let dy = match step {
                Step::Prev => -line,
                Step::Next => line,
                Step::PageUp => -page,
                Step::PageDown => page,
                Step::First => -f32::MAX,
                Step::Last => f32::MAX,
            };
            self.scroll_entry(dy);
            return;
        }
        if self.screen == Screen::Featured {
            let n = match step {
                Step::Prev | Step::PageUp | Step::First => -1,
                Step::Next | Step::PageDown | Step::Last => 1,
            };
            self.step_featured(n);
            return;
        }
        let len = self.rows(self.screen).len();
        if len == 0 {
            return;
        }
        let visible = self.visible_rows(self.screen);
        let next = stepped(self.selected(self.screen), step, len, visible);
        self.set_sel(self.screen, next);
    }

    /// Scroll the open entry, clamped to the height its blocks actually need.
    ///
    /// The clamp reads the same block list the drawing does, so an entry can
    /// never be scrolled past its own last line, and a long one can always be
    /// scrolled to it.
    fn scroll_entry(&mut self, dy: f32) {
        let max = self.entry_max_scroll();
        // `dy` may be `f32::MAX` for a jump to the end; adding first and
        // clamping second would give an infinity, so clamp the sum.
        let next = if dy >= f32::MAX {
            max
        } else if dy <= -f32::MAX {
            0.0
        } else {
            self.entry_scroll + dy
        };
        self.entry_scroll = next.clamp(0.0, max);
    }

    /// How far the open entry can be scrolled: zero when it already fits.
    #[must_use]
    pub fn entry_max_scroll(&self) -> f32 {
        let l = self.layout();
        let pane = l.entry_pane();
        let Some(index) = self.current else {
            return 0.0;
        };
        let blocks = self.entry_blocks(index, &l, pane.w);
        (blocks_height(&blocks) - pane.h).max(0.0)
    }

    /// Move the current list's window by `n` rows, clamped so it can never
    /// show empty space past the last row.
    fn scroll_rows(&mut self, n: isize) {
        let screen = self.screen;
        if !screen.is_list() {
            return;
        }
        let len = self.rows(screen).len();
        let visible = self.visible_rows(screen);
        let max = isize::try_from(len.saturating_sub(visible)).unwrap_or(isize::MAX);
        let top = isize::try_from(self.scroll_top(screen))
            .unwrap_or(0)
            .saturating_add(n)
            .clamp(0, max);
        if let Some(slot) = self.top.get_mut(screen.index()) {
            *slot = usize::try_from(top).unwrap_or(0);
        }
    }

    fn step_featured(&mut self, n: isize) {
        if self.entries.is_empty() {
            return;
        }
        let len = isize::try_from(self.entries.len()).unwrap_or(isize::MAX);
        let next = isize::try_from(self.featured)
            .unwrap_or(0)
            .saturating_add(n)
            .rem_euclid(len);
        self.featured = usize::try_from(next).unwrap_or(0);
        self.status = format!("Featured: {}", self.word(self.featured));
    }

    fn screen_status(&self) -> String {
        match self.screen {
            Screen::Search => {
                if self.query.trim().is_empty() {
                    "Type a word, or part of one".to_string()
                } else {
                    match self.results.len() {
                        0 => format!("Nothing matches \"{}\"", self.query.trim()),
                        1 => format!("One entry matches \"{}\"", self.query.trim()),
                        n => format!("{n} entries match \"{}\"", self.query.trim()),
                    }
                }
            }
            Screen::Entry => self
                .current
                .map(|i| format!("{} — {}", self.word(i), self.summary(i)))
                .unwrap_or_else(|| "Open a word first".to_string()),
            Screen::History => match self.history.len() {
                0 => "Nothing looked up yet".to_string(),
                1 => "One word looked up".to_string(),
                n => format!("{n} words looked up"),
            },
            Screen::Favorites => match self.favorites.len() {
                0 => "No favourites yet — open a word and press Ctrl+D".to_string(),
                1 => "One favourite".to_string(),
                n => format!("{n} favourites"),
            },
            Screen::Featured => format!("Featured: {}", self.word(self.featured)),
        }
    }
}

impl Default for Dictionary {
    fn default() -> Self {
        Self::new()
    }
}
// ── Building an entry's blocks ─────────────────────────────────────────────

/// The pill a cross-reference word is drawn in.
///
/// One function, used both by the packer that decides which words share a row
/// and by the painter that places them — so a chip's hit box cannot land
/// anywhere but under the chip.
#[must_use]
fn chip_w(word: &str, size: f32, pad: f32) -> f32 {
    text::measure(word, size, FontWeightHint::Regular) + pad * 2.0
}

impl Dictionary {
    /// Everything the entry screen draws for `index`, in order.
    #[must_use]
    pub fn entry_blocks(&self, index: usize, l: &Layout, width: f32) -> Vec<Block> {
        let mut out = Vec::new();
        let Some(entry) = self.entries.get(index) else {
            return out;
        };
        let body = (width - l.pad * 2.0).max(1.0);

        out.push(Block::Line {
            text: entry.word.clone(),
            size: l.big,
            color: MAUVE,
            weight: FontWeightHint::Bold,
            indent: 0.0,
            space: 0.0,
        });
        if !entry.pronunciation.is_empty() {
            out.push(Block::Line {
                text: entry.pronunciation.clone(),
                size: l.font,
                color: SUBTEXT0,
                weight: FontWeightHint::Regular,
                indent: 0.0,
                space: l.small * 0.2,
            });
        }

        for (i, def) in entry.definitions.iter().enumerate() {
            out.push(Block::Line {
                text: format!("{}. {}", i.saturating_add(1), def.part_of_speech.label()),
                size: l.small,
                color: def.part_of_speech.color(),
                weight: FontWeightHint::Bold,
                indent: 0.0,
                space: l.font * 0.8,
            });
            let indent = l.font;
            for (n, line) in text::wrap(
                &def.text,
                (body - indent).max(1.0),
                l.font,
                FontWeightHint::Regular,
            )
            .into_iter()
            .enumerate()
            {
                out.push(Block::Line {
                    text: line,
                    size: l.font,
                    color: TEXT_COLOR,
                    weight: FontWeightHint::Regular,
                    indent,
                    space: if n == 0 { l.small * 0.25 } else { 0.0 },
                });
            }
            if let Some(example) = &def.example {
                let quoted = format!("\u{201c}{example}\u{201d}");
                let indent = l.font * 1.8;
                for (n, line) in text::wrap(
                    &quoted,
                    (body - indent).max(1.0),
                    l.small,
                    FontWeightHint::Regular,
                )
                .into_iter()
                .enumerate()
                {
                    out.push(Block::Line {
                        text: line,
                        size: l.small,
                        color: OVERLAY0,
                        weight: FontWeightHint::Regular,
                        indent,
                        space: if n == 0 { l.small * 0.3 } else { 0.0 },
                    });
                }
            }
        }

        // The three cross-reference lists. `related` is here because it had
        // never been drawn anywhere: it was built by `build_dictionary`, stored
        // on every entry, and reached by nothing at all — which is precisely
        // what the blanket `#![allow(dead_code)]` on line 1 was hiding.
        for (title, words, color) in [
            ("Synonyms", &entry.synonyms, GREEN),
            ("Antonyms", &entry.antonyms, RED),
            ("See also", &entry.related, BLUE),
        ] {
            if words.is_empty() {
                continue;
            }
            out.push(Block::Line {
                text: title.to_string(),
                size: l.small,
                color: SUBTEXT1,
                weight: FontWeightHint::Bold,
                indent: 0.0,
                space: l.font * 0.9,
            });
            self.pack_chips(&mut out, words, body, l, color);
        }

        if !entry.etymology.is_empty() {
            out.push(Block::Line {
                text: "Origin".to_string(),
                size: l.small,
                color: SUBTEXT1,
                weight: FontWeightHint::Bold,
                indent: 0.0,
                space: l.font * 0.9,
            });
            for (n, line) in text::wrap(
                &entry.etymology,
                (body - l.font).max(1.0),
                l.small,
                FontWeightHint::Regular,
            )
            .into_iter()
            .enumerate()
            {
                out.push(Block::Line {
                    text: line,
                    size: l.small,
                    color: YELLOW,
                    weight: FontWeightHint::Regular,
                    indent: l.font,
                    space: if n == 0 { l.small * 0.3 } else { 0.0 },
                });
            }
        }
        out
    }

    /// Break `words` into rows of chips no wider than `body`.
    fn pack_chips(
        &self,
        out: &mut Vec<Block>,
        words: &[String],
        body: f32,
        l: &Layout,
        color: Color,
    ) {
        let gap = l.pad * 0.5;
        let mut row: Vec<(String, Option<usize>)> = Vec::new();
        let mut used = 0.0f32;
        for word in words {
            let w = chip_w(word, l.small, l.pad * 0.6);
            let needed = if row.is_empty() { w } else { w + gap };
            if !row.is_empty() && used + needed > body {
                out.push(Block::Chips {
                    words: std::mem::take(&mut row),
                    size: l.small,
                    color,
                    space: if out.last().is_some_and(|b| matches!(b, Block::Chips { .. })) {
                        gap
                    } else {
                        l.small * 0.35
                    },
                });
                used = 0.0;
            }
            used += if row.is_empty() { w } else { w + gap };
            row.push((word.clone(), self.find_word(word)));
        }
        if !row.is_empty() {
            let space = if out.last().is_some_and(|b| matches!(b, Block::Chips { .. })) {
                gap
            } else {
                l.small * 0.35
            };
            out.push(Block::Chips {
                words: row,
                size: l.small,
                color,
                space,
            });
        }
    }
}

// ── Drawing ────────────────────────────────────────────────────────────────

fn fill(f: &mut Frame, r: Rect, color: Color, radius: f32) {
    if r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    f.push(RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii: if radius > 0.0 {
            CornerRadii::all(radius)
        } else {
            CornerRadii::ZERO
        },
    });
}

fn label(
    f: &mut Frame,
    x: f32,
    y: f32,
    body: &str,
    size: f32,
    color: Color,
    weight: FontWeightHint,
    max_width: Option<f32>,
) {
    if body.is_empty() || size <= 0.0 || max_width.is_some_and(|w| w <= 0.0) {
        return;
    }
    f.push(RenderCommand::Text {
        x,
        y,
        text: body.to_string(),
        color,
        font_size: size,
        font_weight: weight,
        max_width,
        overflow: TextOverflow::Ellipsis,
    });
}

/// A label centred in a horizontal span, clamped so a string wider than the
/// span starts at the span's left edge instead of overhanging to its left.
fn centred_in(
    f: &mut Frame,
    left: f32,
    span: f32,
    cy: f32,
    body: &str,
    size: f32,
    color: Color,
    weight: FontWeightHint,
) {
    label(
        f,
        text::center_x(body, left + span / 2.0, size, weight).max(left),
        cy - text::line_height(size, weight) / 2.0,
        body,
        size,
        color,
        weight,
        Some(span.max(0.0)),
    );
}

/// One button: a filled pill with a centred caption, and its hit box.
fn button(f: &mut Frame, r: Rect, body: &str, size: f32, live: bool, target: Target) {
    if r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    fill(
        f,
        r,
        if live { SURFACE1 } else { CRUST },
        (r.h * 0.3).min(8.0),
    );
    centred_in(
        f,
        r.x,
        r.w,
        r.y + r.h / 2.0,
        body,
        size,
        if live { TEXT_COLOR } else { OVERLAY0 },
        FontWeightHint::Bold,
    );
    f.hit(target, r);
}

/// The thin bar down the right-hand edge of a pane that says how much of it
/// you are looking at. Not clickable: it reports, it does not drive.
fn scrollbar(f: &mut Frame, pane: Rect, fraction: f32, offset: f32) {
    if pane.h <= 0.0 || pane.w <= 6.0 || fraction >= 1.0 {
        return;
    }
    let w = (pane.w * 0.012).clamp(2.0, 5.0);
    let track = Rect::new(pane.right() - w, pane.y, w, pane.h);
    fill(f, track, CRUST, w / 2.0);
    let thumb_h = (pane.h * fraction.clamp(0.05, 1.0))
        .max(w * 3.0)
        .min(pane.h);
    let travel = (pane.h - thumb_h).max(0.0);
    fill(
        f,
        Rect::new(
            track.x,
            pane.y + travel * offset.clamp(0.0, 1.0),
            w,
            thumb_h,
        ),
        SURFACE1,
        w / 2.0,
    );
}

impl Dictionary {
    /// The whole window, and every hit box in it, in one pass.
    ///
    /// The old program had no hit test whatsoever: the tab bar, the result
    /// rows, the history and favourites rows, the favourite button and the
    /// "[Enter] View full entry" prompt were painted and inert. Recording the
    /// boxes here, in the pass that draws them, is what stops a control's
    /// clickable area from drifting away from the pixels naming it.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let l = Layout::new(width, height);
        let mut f = Frame::new(l.window.w, l.window.h);
        fill(&mut f, l.window, BASE, 0.0);

        if l.shows_tabs() {
            self.draw_tabs(&mut f, &l);
        }
        match self.screen {
            Screen::Search => self.draw_search(&mut f, &l),
            Screen::Entry => self.draw_entry(&mut f, &l),
            Screen::History | Screen::Favorites => self.draw_list_screen(&mut f, &l, self.screen),
            Screen::Featured => self.draw_featured(&mut f, &l),
        }
        if l.shows_status() {
            self.draw_status(&mut f, &l);
        }
        f
    }

    fn draw_tabs(&self, f: &mut Frame, l: &Layout) {
        fill(f, l.tabs, CRUST, 0.0);
        for (i, screen) in Screen::ALL.into_iter().enumerate() {
            let cell = l.tab(i);
            if cell.w <= 0.0 || cell.h <= 0.0 {
                continue;
            }
            let active = screen == self.screen;
            let live = self.reachable(screen);
            fill(
                f,
                cell,
                if active {
                    SURFACE1
                } else if live {
                    SURFACE0
                } else {
                    CRUST
                },
                (cell.h * 0.25).min(7.0),
            );
            let colour = if active {
                LAVENDER
            } else if live {
                SUBTEXT1
            } else {
                OVERLAY0
            };
            centred_in(
                f,
                cell.x,
                cell.w,
                cell.y + cell.h / 2.0,
                l.tab_caption(screen, cell),
                l.small,
                colour,
                FontWeightHint::Bold,
            );
            // Recorded even for the entry tab with nothing open: a click on it
            // gets an explanation in the status bar, which is more use than a
            // control that swallows the click in silence.
            f.hit(Target::Tab(i), cell);
        }
    }

    fn draw_search(&self, f: &mut Frame, l: &Layout) {
        let field = l.search_box();
        if field.h > 0.0 {
            fill(f, field, SURFACE0, (field.h * 0.22).min(8.0));
            f.hit(Target::SearchBox, field);
            let inner = l.pad;
            let clear_w = (l.small * 4.0).min(field.w * 0.25);
            let has_query = !self.query.is_empty();
            let text_span =
                (field.w - inner * 2.0 - if has_query { clear_w } else { 0.0 }).max(0.0);
            let baseline =
                field.y + field.h / 2.0 - text::line_height(l.font, FontWeightHint::Regular) / 2.0;
            if has_query {
                // The caret is drawn as part of the string rather than as a
                // separate rectangle: there is no cursor to move in this field,
                // so a caret anywhere but the end would be a lie.
                label(
                    f,
                    field.x + inner,
                    baseline,
                    &format!("{}\u{2502}", self.query),
                    l.font,
                    TEXT_COLOR,
                    FontWeightHint::Regular,
                    Some(text_span),
                );
                let clear = Rect::new(
                    field.right() - inner - clear_w,
                    field.y + field.h * 0.15,
                    clear_w,
                    field.h * 0.7,
                );
                button(f, clear, "Clear", l.small, true, Target::ClearQuery);
            } else {
                label(
                    f,
                    field.x + inner,
                    baseline,
                    "Search a word\u{2026}",
                    l.font,
                    OVERLAY0,
                    FontWeightHint::Regular,
                    Some(text_span),
                );
            }
        }
        self.draw_rows(f, l, Screen::Search, l.list_pane(Screen::Search));
    }

    fn draw_list_screen(&self, f: &mut Frame, l: &Layout, screen: Screen) {
        let band = Rect::new(l.content.x, l.content.y, l.content.w, l.header_h());
        if band.h > 0.0 {
            let title = if screen == Screen::History {
                "Recently looked up"
            } else {
                "Favourites"
            };
            label(
                f,
                band.x,
                band.y + band.h / 2.0 - text::line_height(l.font, FontWeightHint::Bold) / 2.0,
                title,
                l.font,
                LAVENDER,
                FontWeightHint::Bold,
                Some(band.w * 0.6),
            );
            if screen == Screen::History && !self.history.is_empty() {
                let w = (l.small * 5.0).min(band.w * 0.3);
                button(
                    f,
                    l.trailing_button(band, 0, w),
                    "Clear",
                    l.small,
                    true,
                    Target::ClearHistory,
                );
            }
        }
        self.draw_rows(f, l, screen, l.list_pane(screen));
    }

    /// The rows of whichever list `screen` shows.
    ///
    /// Clipped to the pane, which is what keeps a half-scrolled row from
    /// taking a click on the part of it that was never drawn: `Frame::clip`
    /// trims the recorded boxes as well as the ink.
    fn draw_rows(&self, f: &mut Frame, l: &Layout, screen: Screen, pane: Rect) {
        if pane.w <= 0.0 || pane.h <= 0.0 {
            return;
        }
        let rows = self.rows(screen);
        let visible = l.rows_in(pane);
        if rows.is_empty() {
            let empty = match screen {
                Screen::Search if self.query.trim().is_empty() => {
                    "Start typing to search the dictionary"
                }
                Screen::Search => "No entry matches that",
                Screen::History => "Nothing looked up yet",
                _ => "No favourites yet \u{2014} open a word and press Ctrl+D",
            };
            label(
                f,
                pane.x + l.pad,
                pane.y + l.pad,
                empty,
                l.font,
                OVERLAY0,
                FontWeightHint::Regular,
                Some((pane.w - l.pad * 2.0).max(0.0)),
            );
            return;
        }

        let top = self
            .scroll_top(screen)
            .min(rows.len().saturating_sub(visible.max(1)));
        let sel = self.selected(screen);
        // One row past the whole ones, when the pane has a strip of height
        // left over for it. A list that stops on an exact row boundary and
        // leaves a blank band below looks finished even when it is not; a row
        // sliced by the bottom edge says "there is more" without being read.
        // `rows_in` deliberately stays a count of *whole* rows, because that
        // is what the selection and the page keys must move within — a peek
        // row is for the eye, not for `Enter`.
        let leftover = pane.h - visible as f32 * l.row_h();
        let peek = usize::from(leftover > 1.0 && rows.len() > top.saturating_add(visible));
        // The clip is what makes the peek row honest: it trims the recorded
        // hit box along with the ink, so the half of the row that was never
        // drawn cannot be clicked either.
        f.clip(pane);
        for slot in 0..visible.saturating_add(peek) {
            let Some(&index) = rows.get(top.saturating_add(slot)) else {
                break;
            };
            let r = l.row(pane, slot);
            let chosen = top.saturating_add(slot) == sel;
            if chosen {
                fill(f, r, SURFACE0, (r.h * 0.2).min(7.0));
            }
            self.draw_row(f, l, r, index, chosen);
            f.hit(Target::Row(top.saturating_add(slot)), r);
        }
        f.unclip();

        if rows.len() > visible && visible > 0 {
            let fraction = visible as f32 / rows.len() as f32;
            let travel = rows.len().saturating_sub(visible).max(1) as f32;
            scrollbar(f, pane, fraction, top as f32 / travel);
        }
    }

    fn draw_row(&self, f: &mut Frame, l: &Layout, r: Rect, index: usize, chosen: bool) {
        let Some(entry) = self.entries.get(index) else {
            return;
        };
        let inner = l.pad;
        let star = if self.favorites.contains(&entry.word) {
            "\u{2605} "
        } else {
            ""
        };
        let head = format!("{star}{}", entry.word);
        let word_w = text::measure(&head, l.font, FontWeightHint::Bold).min(r.w * 0.45);
        label(
            f,
            r.x + inner,
            r.y + r.h * 0.5 - text::line_height(l.font, FontWeightHint::Bold),
            &head,
            l.font,
            if chosen { LAVENDER } else { TEXT_COLOR },
            FontWeightHint::Bold,
            Some(word_w),
        );
        // The parts of speech, in their own colours, after the word.
        let mut x = r.x + inner + word_w + inner * 0.6;
        for short in entry.definitions.iter().map(|d| d.part_of_speech).fold(
            Vec::new(),
            |mut acc: Vec<PartOfSpeech>, p| {
                if !acc.contains(&p) {
                    acc.push(p);
                }
                acc
            },
        ) {
            let w = text::measure(short.short(), l.small, FontWeightHint::Regular);
            if x + w > r.right() - inner {
                break;
            }
            label(
                f,
                x,
                r.y + r.h * 0.5 - text::line_height(l.small, FontWeightHint::Regular),
                short.short(),
                l.small,
                short.color(),
                FontWeightHint::Regular,
                Some(w),
            );
            x += w + inner * 0.5;
        }
        // The first sense, cut with an ellipsis rather than by a character
        // count: the renderer knows the font, and this code does not.
        if let Some(first) = entry.definitions.first() {
            label(
                f,
                r.x + inner,
                r.y + r.h * 0.5 + text::line_height(l.small, FontWeightHint::Regular) * 0.1,
                &first.text,
                l.small,
                SUBTEXT0,
                FontWeightHint::Regular,
                Some((r.w - inner * 2.0).max(0.0)),
            );
        }
    }

    fn draw_entry(&self, f: &mut Frame, l: &Layout) {
        let Some(index) = self.current else {
            label(
                f,
                l.content.x + l.pad,
                l.content.y + l.pad,
                "No word open \u{2014} search for one first",
                l.font,
                OVERLAY0,
                FontWeightHint::Regular,
                Some(l.content.w),
            );
            return;
        };
        let bar = l.entry_bar();
        if bar.h > 0.0 {
            let back_w = (l.small * 5.0).min(bar.w * 0.3);
            button(
                f,
                Rect::new(
                    bar.x,
                    bar.y + (bar.h - (bar.h - l.pad * 0.5).clamp(0.0, l.font * 2.0)) / 2.0,
                    back_w,
                    (bar.h - l.pad * 0.5).clamp(0.0, l.font * 2.0),
                ),
                "Back",
                l.small,
                true,
                Target::Back,
            );
            let fav_w = (l.small * 9.0).min(bar.w * 0.45);
            let caption = if self.is_favorite() {
                "\u{2605} Favourite"
            } else {
                "\u{2606} Favourite"
            };
            button(
                f,
                l.trailing_button(bar, 0, fav_w),
                caption,
                l.small,
                true,
                Target::Favorite,
            );
        }

        let pane = l.entry_pane();
        if pane.w <= 0.0 || pane.h <= 0.0 {
            return;
        }
        let blocks = self.entry_blocks(index, l, pane.w);
        let total = blocks_height(&blocks);
        f.clip(pane);
        let mut y = pane.y - self.entry_scroll;
        for block in &blocks {
            y += block.space();
            let h = block.height();
            if y + h >= pane.y && y <= pane.bottom() {
                draw_block(f, l, block, pane, y);
            }
            y += h;
        }
        f.unclip();

        if total > pane.h {
            let travel = (total - pane.h).max(1.0);
            scrollbar(f, pane, pane.h / total, self.entry_scroll / travel);
        }
    }

    fn draw_featured(&self, f: &mut Frame, l: &Layout) {
        let card = l.content;
        if card.w <= 0.0 || card.h <= 0.0 {
            return;
        }
        fill(f, card, CRUST, (card.h * 0.04).min(12.0));
        let inner = l.pad * 1.5;
        let mut y = card.y + inner;
        label(
            f,
            card.x + inner,
            y,
            "Featured word",
            l.small,
            TEAL,
            FontWeightHint::Bold,
            Some((card.w - inner * 2.0).max(0.0)),
        );
        y += text::line_height(l.small, FontWeightHint::Bold) + l.pad * 0.5;

        let Some(entry) = self.entries.get(self.featured) else {
            return;
        };
        label(
            f,
            card.x + inner,
            y,
            &entry.word,
            l.big,
            MAUVE,
            FontWeightHint::Bold,
            Some((card.w - inner * 2.0).max(0.0)),
        );
        y += text::line_height(l.big, FontWeightHint::Bold);
        label(
            f,
            card.x + inner,
            y,
            &entry.pronunciation,
            l.small,
            SUBTEXT0,
            FontWeightHint::Regular,
            Some((card.w - inner * 2.0).max(0.0)),
        );
        y += text::line_height(l.small, FontWeightHint::Regular) + l.pad;

        // The buttons are placed first so the definition can be given exactly
        // the room that is left, rather than drawn over them.
        let bh = (l.font * 2.0).min(card.h * 0.2);
        let bw = ((card.w - inner * 2.0) / 3.5).min(l.small * 9.0);
        let by = card.bottom() - inner - bh;
        let gap = l.pad * 0.6;
        button(
            f,
            Rect::new(card.x + inner, by, bw, bh),
            "Previous",
            l.small,
            self.entries.len() > 1,
            Target::PrevFeatured,
        );
        button(
            f,
            Rect::new(card.x + inner + bw + gap, by, bw, bh),
            "Next",
            l.small,
            self.entries.len() > 1,
            Target::NextFeatured,
        );
        button(
            f,
            Rect::new(card.right() - inner - bw, by, bw, bh),
            "Read entry",
            l.small,
            true,
            Target::OpenFeatured,
        );

        let room = (by - l.pad - y).max(0.0);
        if let Some(def) = entry.definitions.first() {
            let line = text::line_height(l.font, FontWeightHint::Regular);
            let fits = if line <= 0.0 {
                0
            } else {
                (room / line).floor().max(0.0) as usize
            };
            for row in text::wrap(
                &def.text,
                (card.w - inner * 2.0).max(1.0),
                l.font,
                FontWeightHint::Regular,
            )
            .into_iter()
            .take(fits)
            {
                label(
                    f,
                    card.x + inner,
                    y,
                    &row,
                    l.font,
                    TEXT_COLOR,
                    FontWeightHint::Regular,
                    Some((card.w - inner * 2.0).max(0.0)),
                );
                y += line;
            }
        }
    }

    fn draw_status(&self, f: &mut Frame, l: &Layout) {
        fill(f, l.status, CRUST, 0.0);
        let baseline = l.status.y + l.status.h / 2.0
            - text::line_height(l.small, FontWeightHint::Regular) / 2.0;
        let hint = self.hint();
        // Measured, not guessed at: the old bar drew its right-hand half at
        // `width - 250`, which is a negative x on any window under 250 across.
        let hint_w = text::measure(hint, l.small, FontWeightHint::Regular);
        let room = (l.status.w - l.pad * 2.0).max(0.0);
        let show_hint = hint_w > 0.0 && hint_w + l.pad * 4.0 <= room;
        label(
            f,
            l.status.x + l.pad,
            baseline,
            self.status(),
            l.small,
            SUBTEXT1,
            FontWeightHint::Regular,
            Some(if show_hint {
                (room - hint_w - l.pad * 2.0).max(0.0)
            } else {
                room
            }),
        );
        if show_hint {
            label(
                f,
                l.status.right() - l.pad - hint_w,
                baseline,
                hint,
                l.small,
                OVERLAY0,
                FontWeightHint::Regular,
                Some(hint_w),
            );
        }
    }

    /// The keys worth naming on the screen you are on.
    #[must_use]
    pub fn hint(&self) -> &'static str {
        match self.screen {
            Screen::Search => "Type to search  Enter open  Tab screens",
            Screen::Entry => "Esc back  Ctrl+D favourite  Up/Down scroll",
            Screen::History | Screen::Favorites => "Enter open  Esc search  Tab screens",
            Screen::Featured => "Left/Right change  Enter read",
        }
    }
}

fn draw_block(f: &mut Frame, l: &Layout, block: &Block, pane: Rect, y: f32) {
    match block {
        Block::Line {
            text: body,
            size,
            color,
            weight,
            indent,
            ..
        } => {
            label(
                f,
                pane.x + l.pad + indent,
                y,
                body,
                *size,
                *color,
                *weight,
                Some((pane.w - l.pad * 2.0 - indent).max(0.0)),
            );
        }
        Block::Chips {
            words, size, color, ..
        } => {
            let h = block.height();
            let gap = l.pad * 0.5;
            let inner = l.pad * 0.6;
            let mut x = pane.x + l.pad;
            for (word, link) in words {
                let w = chip_w(word, *size, inner);
                let chip = Rect::new(x, y, w, h);
                fill(f, chip, SURFACE0, (h * 0.35).min(9.0));
                centred_in(
                    f,
                    chip.x,
                    chip.w,
                    chip.y + chip.h / 2.0,
                    word,
                    *size,
                    if link.is_some() { *color } else { SUBTEXT0 },
                    FontWeightHint::Regular,
                );
                // Only a chip the dictionary can actually open is clickable, so
                // a chip that looks live leads somewhere.
                if let Some(index) = link {
                    f.hit(Target::Link(*index), chip);
                }
                x += w + gap;
            }
        }
    }
}

// ── Input ──────────────────────────────────────────────────────────────────

impl Dictionary {
    /// What a click at this point lands on, at the size last drawn.
    #[must_use]
    pub fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.size.0, self.size.1).hit_test(x, y)
    }

    fn handle_key(&mut self, ev: &KeyEvent) -> EventResult {
        // A release is not a second press.
        if !ev.pressed {
            return EventResult::Ignored;
        }
        let m = ev.modifiers;
        if m.alt || m.super_key {
            return EventResult::Ignored;
        }

        if m.ctrl {
            let action = match ev.key {
                Key::Num1 => Some(Action::Go(Screen::Search)),
                Key::Num2 => Some(Action::Go(Screen::Entry)),
                Key::Num3 => Some(Action::Go(Screen::History)),
                Key::Num4 => Some(Action::Go(Screen::Favorites)),
                Key::Num5 => Some(Action::Go(Screen::Featured)),
                Key::D => Some(Action::ToggleFavorite),
                Key::L | Key::K | Key::F => Some(Action::Go(Screen::Search)),
                Key::Backspace => Some(Action::ClearQuery),
                _ => None,
            };
            return match action {
                Some(a) => {
                    self.apply(a);
                    EventResult::Consumed
                }
                None => EventResult::Ignored,
            };
        }

        let action = match ev.key {
            Key::Tab => Some(Action::CycleScreen(if m.shift { -1 } else { 1 })),
            Key::Up => Some(Action::Move(Step::Prev)),
            Key::Down => Some(Action::Move(Step::Next)),
            Key::Left if self.screen == Screen::Featured => Some(Action::StepFeatured(-1)),
            Key::Right if self.screen == Screen::Featured => Some(Action::StepFeatured(1)),
            Key::PageUp => Some(Action::Move(Step::PageUp)),
            Key::PageDown => Some(Action::Move(Step::PageDown)),
            Key::Home => Some(Action::Move(Step::First)),
            Key::End => Some(Action::Move(Step::Last)),
            Key::Enter => Some(match self.screen {
                Screen::Featured => Action::Open(self.featured),
                Screen::Entry => Action::ToggleFavorite,
                _ => Action::OpenSelected,
            }),
            Key::Backspace => Some(match self.screen {
                Screen::Search => Action::Backspace,
                Screen::Entry => Action::Back,
                _ => Action::Go(Screen::Search),
            }),
            // Escape always leads somewhere. The old handler cleared
            // `search_active` while leaving `screen == Search`, and the typing
            // branch was gated on both — so the first Escape put the program in
            // a state where the keyboard did nothing at all and only a second
            // Escape got out of it.
            Key::Escape => Some(match self.screen {
                Screen::Search => Action::ClearQuery,
                Screen::Entry => Action::Back,
                _ => Action::Go(Screen::Search),
            }),
            _ => None,
        };

        if let Some(a) = action {
            self.apply(a);
            return EventResult::Consumed;
        }

        // Anything that types goes into the query, from any screen — which is
        // both what a reader expects and how the search field is reached
        // without a pointer. `typed()` drops the control characters, so Enter
        // and Tab cannot arrive here as text.
        if ev.types_text() {
            for ch in ev.typed() {
                self.apply(Action::Type(ch));
            }
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }

    fn handle_mouse(&mut self, ev: &MouseEvent) -> EventResult {
        if let MouseEventKind::Scroll { dy, .. } = ev.kind {
            return self.handle_scroll(dy);
        }
        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {
            return EventResult::Ignored;
        }
        let Some(target) = self.target_at(ev.x, ev.y) else {
            return EventResult::Ignored;
        };
        match target {
            Target::Tab(i) => {
                if let Some(screen) = Screen::from_index(i) {
                    self.apply(Action::Go(screen));
                }
            }
            Target::Row(i) => {
                let rows = self.rows(self.screen);
                if let Some(&index) = rows.get(i) {
                    // One click selects and opens: a two-step select-then-open
                    // is a keyboard idiom, and the row is big enough to hit.
                    self.set_sel(self.screen, i);
                    self.apply(Action::Open(index));
                }
            }
            Target::Link(index) => self.apply(Action::Open(index)),
            // The field is only ever drawn on the search screen, and typing
            // already reaches the query from every screen, so `Go(Search)`
            // was a hit box that could not do anything on any click that
            // could reach it — fault two in miniature, recorded by the very
            // rewrite that was meant to end it. A field with no caret cannot
            // be focused, so the one useful thing a click here can do is say
            // so, and say what to press instead.
            Target::SearchBox => {
                self.screen = Screen::Search;
                self.status = if self.query.is_empty() {
                    "There is no caret to place \u{2014} just type, from any screen".to_string()
                } else {
                    format!(
                        "\u{201c}{}\u{201d} \u{2014} keep typing to narrow it, Ctrl+K to clear",
                        self.query
                    )
                };
            }
            Target::ClearQuery => self.apply(Action::ClearQuery),
            Target::ClearHistory => self.apply(Action::ClearHistory),
            Target::Favorite => self.apply(Action::ToggleFavorite),
            Target::Back => self.apply(Action::Back),
            Target::PrevFeatured => self.apply(Action::StepFeatured(-1)),
            Target::NextFeatured => self.apply(Action::StepFeatured(1)),
            Target::OpenFeatured => self.apply(Action::Open(self.featured)),
        }
        EventResult::Consumed
    }

    /// The wheel, in the units the wheel actually arrives in.
    ///
    /// `dy` is a count of **notches** — a fraction of one from a trackpad — so
    /// a list turns it into whole rows through an accumulator that keeps the
    /// remainder, and the entry view, whose offset is genuinely continuous,
    /// scales it to pixels directly.
    fn handle_scroll(&mut self, dy: f32) -> EventResult {
        if self.screen == Screen::Entry {
            let line = text::line_height(self.layout().font, FontWeightHint::Regular);
            let pixels = guitk::wheel::pixels(dy, line);
            if pixels == 0.0 {
                return EventResult::Ignored;
            }
            self.apply(Action::Scroll(pixels));
            return EventResult::Consumed;
        }
        if !self.screen.is_list() {
            return EventResult::Ignored;
        }
        let rows = self.wheel.rows(dy);
        if rows == 0 {
            return EventResult::Ignored;
        }
        self.apply(Action::ScrollRows(rows));
        EventResult::Consumed
    }
}

/// The one body both the window and the test probe drive, so what a click does
/// in a test is what it does on a screen.
pub fn handle_event(app: &mut Dictionary, event: &Event) -> EventResult {
    match event {
        Event::Key(ev) => app.handle_key(ev),
        Event::Mouse(ev) => app.handle_mouse(ev),
        Event::Resize { width, height } => {
            app.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

impl App for Dictionary {
    fn title(&self) -> String {
        "Dictionary".to_string()
    }

    fn app_id(&self) -> String {
        "dictionary".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match handle_event(self, event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The size the frame is drawn at is the size the next click is read
        // against — that is the whole point of storing it here.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for Dictionary {
    type Target = Target;
    type Outcome = EventResult;
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        handle_event(
            self,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            }),
        )
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        handle_event(self, &Event::Key(key.clone()))
    }
}

fn main() -> ExitCode {
    let mut app = Dictionary::new();
    app::launch("dictionary", &mut app)
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::expect_used,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;
    use guitk::event::Modifiers;
    use guitk::probe;

    /// The window sizes every layout claim is checked at. The first three are
    /// smaller than the chrome would like, which is the case the old constant
    /// layout — fixed at 800x600 whatever the window did — never had to
    /// survive.
    const WINDOWS: &[(f32, f32)] = &[
        (120.0, 90.0),
        (200.0, 160.0),
        (320.0, 240.0),
        (400.0, 900.0),
        (860.0, 640.0),
        (900.0, 500.0),
        (1280.0, 720.0),
        (1920.0, 1080.0),
        (2560.0, 1440.0),
    ];

    /// A window short enough that a result list is longer than its pane.
    const SHORT: (f32, f32) = (700.0, 300.0);

    /// A window in which a full entry is taller than the pane that shows it.
    ///
    /// Narrow as well as short: the font scales with the window's *height*, so
    /// a merely short window shrinks the type along with the pane and the entry
    /// goes on fitting. It is the width that forces the wrapping.
    const CRAMPED: (f32, f32) = (240.0, 200.0);

    /// `a` and `b` to within a pixel.
    ///
    /// A clamp against a computed maximum lands on exactly that maximum, but
    /// asserting bit equality on a float that reached its value through a sum
    /// is a test that fails for the wrong reason the day a font metric moves.
    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    fn app() -> Dictionary {
        Dictionary::new()
    }

    fn sized(size: (f32, f32)) -> Dictionary {
        let mut d = app();
        d.resize(size.0, size.1);
        d
    }

    /// Everything a test can see of the state, in one string.
    ///
    /// Used to assert that a control *did* something: a recorded hit box that
    /// changes nothing is worse than no hit box at all, because it swallows the
    /// click instead of letting it fall through.
    fn describe(d: &Dictionary) -> String {
        format!(
            "{:?}|{}|{:?}|{}|{:.2}|{:?}|{:?}|{:?}|{}|{}|{}",
            d.screen(),
            d.query(),
            d.current(),
            d.featured(),
            d.entry_scroll(),
            d.history(),
            d.favorites(),
            d.rows(d.screen()),
            d.selected(d.screen()),
            d.scroll_top(d.screen()),
            d.status()
        )
    }

    /// Every rectangle the frame paints. A `Text` is reported as the zero-sized
    /// point it starts at, because its width is the renderer's business.
    ///
    /// Clips are honoured rather than ignored: a command emitted between a
    /// `PushClip` and its `PopClip` is cut down to the clip, and dropped
    /// entirely when the clip excludes it. Ignoring them would make this
    /// helper disagree with the screen — the list draws one row past the
    /// bottom of its pane on purpose, and a renderer that obeys the clip
    /// never puts a pixel of the overhang anywhere.
    fn painted(d: &Dictionary, w: f32, h: f32) -> Vec<Rect> {
        let mut clips: Vec<Rect> = Vec::new();
        let mut out = Vec::new();
        for c in d.frame(w, h).commands() {
            let r = match *c {
                RenderCommand::PushClip {
                    x,
                    y,
                    width,
                    height,
                } => {
                    let next = Rect::new(x, y, width, height);
                    clips.push(clips.last().map_or(next, |c| intersect(*c, next)));
                    continue;
                }
                RenderCommand::PopClip => {
                    clips.pop();
                    continue;
                }
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                }
                | RenderCommand::StrokeRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => Rect::new(x, y, width, height),
                RenderCommand::Text { x, y, .. } => Rect::new(x, y, 0.0, 0.0),
                _ => continue,
            };
            match clips.last() {
                // A zero-height text origin still has to sit *within* the
                // clip, so the test is `contains`, not a non-empty overlap.
                Some(&clip) => {
                    if r.x >= clip.x - 0.01
                        && r.y >= clip.y - 0.01
                        && r.right() <= clip.right() + 0.01
                        && r.bottom() <= clip.bottom() + 0.01
                    {
                        out.push(r);
                    } else if r.w > 0.0 && r.h > 0.0 {
                        let cut = intersect(r, clip);
                        if cut.w > 0.0 && cut.h > 0.0 {
                            out.push(cut);
                        }
                    }
                }
                None => out.push(r),
            }
        }
        out
    }

    fn intersect(a: Rect, b: Rect) -> Rect {
        let x = a.x.max(b.x);
        let y = a.y.max(b.y);
        Rect::new(
            x,
            y,
            (a.right().min(b.right()) - x).max(0.0),
            (a.bottom().min(b.bottom()) - y).max(0.0),
        )
    }

    fn texts(d: &Dictionary, size: (f32, f32)) -> Vec<String> {
        d.frame(size.0, size.1)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn shows(d: &Dictionary, size: (f32, f32), needle: &str) -> bool {
        texts(d, size).iter().any(|t| t.contains(needle))
    }

    fn bands(l: &Layout) -> [bool; 2] {
        [l.shows_tabs(), l.shows_status()]
    }

    /// Type `query` at whatever size the dictionary is currently being drawn
    /// at.
    ///
    /// Not [`probe::type_str`], which delivers every key at [`Probe::SIZE`] and
    /// would quietly undo a preceding [`sized`] — so a test that set up a small
    /// window would go on to assert about a large one.
    fn search_for(d: &mut Dictionary, query: &str) {
        let window = d.layout().window;
        let size = (window.w, window.h);
        for ch in query.chars() {
            d.key_at(&probe::typing(&ch.to_string()), size);
        }
    }

    fn open(d: &mut Dictionary, word: &str) {
        let i = d
            .find_word(word)
            .unwrap_or_else(|| panic!("the dictionary has no entry for {word}"));
        d.apply(Action::Open(i));
    }

    /// A dictionary with something on every screen: a query, a word open, a
    /// history and a favourite. Rebuilt rather than cloned, so a test that
    /// wants a hundred independent copies gets a hundred honest ones.
    fn furnished(screen: Screen) -> Dictionary {
        let mut d = app();
        open(&mut d, "cache");
        d.apply(Action::ToggleFavorite);
        open(&mut d, "kernel");
        search_for(&mut d, "e");
        d.apply(Action::Go(screen));
        d
    }

    // ── Layout ─────────────────────────────────────────────────────────────

    #[test]
    fn nothing_is_painted_outside_the_window() {
        for &(w, h) in WINDOWS {
            for screen in Screen::ALL {
                let mut d = furnished(screen);
                d.resize(w, h);
                for r in painted(&d, w, h) {
                    assert!(
                        r.x >= -0.5 && r.y >= -0.5 && r.right() <= w + 0.5 && r.bottom() <= h + 0.5,
                        "{screen:?} paints {r:?} outside {w}x{h}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_content_keeps_its_share_of_every_window() {
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            assert!(
                l.content.h >= h * CONTENT_SHARE - 0.5,
                "{w}x{h}: content is {} of a promised {}",
                l.content.h,
                h * CONTENT_SHARE
            );
            assert!(l.content.w > 0.0, "{w}x{h}: content has no width");
        }
    }

    #[test]
    fn a_band_is_dropped_whole_and_never_half_drawn() {
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            for band in [l.tabs, l.status] {
                assert!(
                    band == Rect::EMPTY || band.h >= 16.0,
                    "{w}x{h}: a band survives at {}px, too short to read",
                    band.h
                );
            }
        }
    }

    #[test]
    fn a_taller_window_never_loses_a_band_a_shorter_one_had() {
        let mut previous = [false, false];
        for h in 60_u16..1200 {
            let now = bands(&Layout::new(1000.0, f32::from(h)));
            for (i, (&was, &is)) in previous.iter().zip(now.iter()).enumerate() {
                assert!(
                    !was || is,
                    "band {i} was drawn at {}px tall and is gone at {h}px",
                    h - 1
                );
            }
            previous = now;
        }
    }

    #[test]
    fn the_bands_go_in_the_stated_order() {
        // The status bar first, the tab bar second — written out here rather
        // than read from `BAND_DROP_ORDER`, so reordering the constant fails
        // this test instead of quietly redefining what it checks.
        let mut order: Vec<usize> = Vec::new();
        let mut previous = [true, true];
        for h in (60_u16..1200).rev() {
            let now = bands(&Layout::new(1000.0, f32::from(h)));
            for (i, (&was, &is)) in previous.iter().zip(now.iter()).enumerate() {
                if was && !is {
                    order.push(i);
                }
            }
            previous = now;
        }
        assert_eq!(order, vec![1, 0], "the bands went in the wrong order");
    }

    #[test]
    fn a_bar_too_narrow_to_read_is_not_drawn() {
        // A tall, very narrow window has the height for both bars and the width
        // for neither.
        let l = Layout::new(100.0, 1000.0);
        assert!(!l.shows_tabs(), "five tabs drawn across 100px");
        assert!(!l.shows_status(), "a status line drawn across 100px");
        assert_eq!(l.tab(0), Rect::EMPTY, "a tab cell exists with no tab bar");
    }

    #[test]
    fn every_tab_caption_fits_its_cell() {
        // Fault six in miniature: five fixed 110px cells wanted 574px and ran
        // straight off the edge of anything narrower.
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            if !l.shows_tabs() {
                continue;
            }
            for (i, screen) in Screen::ALL.into_iter().enumerate() {
                let cell = l.tab(i);
                let caption = l.tab_caption(screen, cell);
                let width = text::measure(caption, l.small, FontWeightHint::Bold);
                assert!(
                    width <= cell.w + 0.5,
                    "{w}x{h}: {screen:?} caption {caption:?} is {width}px in a {}px cell",
                    cell.w
                );
            }
            for i in 1..SCREENS {
                assert!(
                    l.tab(i).x >= l.tab(i - 1).right() - 0.5,
                    "{w}x{h}: tab {i} overlaps tab {}",
                    i - 1
                );
            }
        }
    }

    #[test]
    fn the_status_bar_stays_inside_the_window() {
        // The old bar drew its right-hand hint at `width - 250`, which is a
        // negative x on any window under 250 across.
        for &(w, h) in WINDOWS {
            let mut d = sized((w, h));
            search_for(&mut d, "concurrency");
            for r in painted(&d, w, h) {
                assert!(r.x >= -0.5, "{w}x{h}: something is painted at x={}", r.x);
            }
        }
    }

    #[test]
    fn the_dictionary_is_still_usable_in_a_window_too_small_for_the_chrome() {
        let size = (120.0, 90.0);
        let l = Layout::new(size.0, size.1);
        assert!(
            !l.shows_tabs() && !l.shows_status(),
            "the chrome survived 120x90"
        );
        let mut d = sized(size);
        search_for(&mut d, "kernel");
        assert_eq!(
            d.rows(Screen::Search).first().copied(),
            d.find_word("kernel")
        );
        d.key_at(&probe::press(Key::Enter), size);
        assert_eq!(d.screen(), Screen::Entry);
        assert!(shows(&d, size, "kernel"), "no word drawn at 120x90");
    }

    // ── Pointer ────────────────────────────────────────────────────────────

    #[test]
    fn every_tab_can_be_clicked() {
        // Fault one: the tab bar was painted and inert — there was not one hit
        // box anywhere in the program.
        for (i, screen) in Screen::ALL.into_iter().enumerate() {
            let mut d = furnished(Screen::Search);
            probe::click(&mut d, Target::Tab(i));
            assert_eq!(d.screen(), screen, "tab {i} went to the wrong screen");
        }
    }

    #[test]
    fn every_control_the_program_draws_answers_a_click() {
        for screen in Screen::ALL {
            let reference = furnished(screen);
            let targets: Vec<Target> = reference
                .frame(WINDOW_WIDTH, WINDOW_HEIGHT)
                .hits()
                .iter()
                .map(|(t, _)| *t)
                .collect();
            assert!(
                !targets.is_empty(),
                "{screen:?} records no hit boxes at all"
            );
            for target in targets {
                // A tab whose whole job is "go to this screen", clicked while
                // you are already on that screen, is entitled to do nothing.
                // Every other control must answer — including the search
                // field, which is drawn only on the screen it would take you
                // to and so has to justify its hit box some other way.
                if target == Target::Tab(screen.index()) {
                    continue;
                }
                let mut d = furnished(screen);
                let before = describe(&d);
                probe::click(&mut d, target);
                assert_ne!(
                    describe(&d),
                    before,
                    "{screen:?}: clicking {target:?} changed nothing"
                );
            }
        }
    }

    #[test]
    fn clicking_a_result_row_opens_the_word_that_row_shows() {
        let mut d = app();
        search_for(&mut d, "e");
        let rows = d.rows(Screen::Search);
        assert!(rows.len() > 1, "not enough results to tell the rows apart");
        let wanted = d.word(rows[1]).to_string();
        probe::click(&mut d, Target::Row(1));
        assert_eq!(d.screen(), Screen::Entry);
        assert_eq!(d.word(d.current().unwrap()), wanted);
    }

    #[test]
    fn a_row_is_opened_by_a_click_on_the_pixels_that_draw_it() {
        // Not through the hit box's name, but through a point inside the
        // rectangle the row's own ink occupies.
        let mut d = app();
        search_for(&mut d, "e");
        let row = probe::rect_of(&d, Target::Row(2)).expect("row 2 has no hit box");
        let wanted = d.word(d.rows(Screen::Search)[2]).to_string();
        d.click_at(
            row.x + row.w * 0.5,
            row.y + row.h * 0.5,
            MouseButton::Left,
            Dictionary::SIZE,
        );
        assert_eq!(d.word(d.current().unwrap()), wanted);
    }

    #[test]
    fn the_search_field_answers_a_click_on_its_own_pixels() {
        // Deliberately not through `probe::rect_of`: that reads the recorded
        // hit box, so a field that recorded none would quietly vanish from
        // the test rather than fail it — which is exactly how the field's
        // dead `Go(Search)` survived the first mutation run. The rectangle
        // comes from the layout, the same one the drawing pass fills, so the
        // claim is "the pixels that look like a field behave like one".
        for (query, expected) in [("", "no caret"), ("ker", "Ctrl+K")] {
            let mut d = app();
            search_for(&mut d, query);
            let field = d.layout().search_box();
            assert!(field.w > 0.0 && field.h > 0.0, "the field is not drawn");
            let (x, y) = field.centre();
            let before = describe(&d);
            d.click_at(x, y, MouseButton::Left, Dictionary::SIZE);
            assert_ne!(
                describe(&d),
                before,
                "a click on the search field did nothing"
            );
            assert!(
                d.status().contains(expected),
                "the field answered {:?}, which does not mention {expected:?}",
                d.status()
            );
        }
    }

    #[test]
    fn a_click_on_nothing_is_ignored() {
        let mut d = app();
        let before = describe(&d);
        assert_eq!(probe::click_background(&mut d), EventResult::Ignored);
        assert_eq!(
            describe(&d),
            before,
            "a click on the background did something"
        );
    }

    #[test]
    fn the_favourite_button_is_clickable_and_reversible() {
        let mut d = app();
        open(&mut d, "paradigm");
        assert!(!d.is_favorite());
        probe::click(&mut d, Target::Favorite);
        assert!(d.is_favorite(), "the star did not take");
        assert_eq!(d.favorites(), ["paradigm"]);
        probe::click(&mut d, Target::Favorite);
        assert!(!d.is_favorite(), "the star did not come off again");
        assert!(d.favorites().is_empty());
    }

    #[test]
    fn a_cross_reference_chip_opens_the_word_it_names() {
        let mut d = app();
        open(&mut d, "algorithm");
        let l = d.layout();
        let blocks = d.entry_blocks(d.current().unwrap(), &l, l.entry_pane().w);
        let (word, index) = blocks
            .iter()
            .find_map(|b| match b {
                Block::Chips { words, .. } => {
                    words.iter().find_map(|(w, i)| i.map(|i| (w.clone(), i)))
                }
                Block::Line { .. } => None,
            })
            .expect("algorithm has no chip that names another entry");
        probe::click(&mut d, Target::Link(index));
        assert_eq!(
            d.word(d.current().unwrap()).to_lowercase(),
            word.to_lowercase()
        );
    }

    #[test]
    fn a_chip_naming_a_word_the_dictionary_lacks_is_not_clickable() {
        let d = app();
        let l = d.layout();
        let mut dead = 0;
        for i in 0..d.entries().len() {
            for block in d.entry_blocks(i, &l, l.entry_pane().w) {
                if let Block::Chips { words, .. } = block {
                    dead += words.iter().filter(|(_, link)| link.is_none()).count();
                }
            }
        }
        assert!(
            dead > 0,
            "every cross-reference in the dictionary has an entry — \
             this test needs one that does not"
        );
        // And every recorded link points at a real entry.
        let mut d = app();
        open(&mut d, "algorithm");
        for (target, _) in d.frame(WINDOW_WIDTH, WINDOW_HEIGHT).hits() {
            if let Target::Link(i) = *target {
                assert!(
                    d.entry(i).is_some(),
                    "a chip links to entry {i}, which does not exist"
                );
            }
        }
    }

    #[test]
    fn the_clear_button_empties_the_query_and_is_only_offered_when_there_is_one() {
        let mut d = app();
        assert!(
            probe::rect_of(&d, Target::ClearQuery).is_none(),
            "an empty field offers a Clear button"
        );
        search_for(&mut d, "kernel");
        assert_eq!(d.query(), "kernel");
        probe::click(&mut d, Target::ClearQuery);
        assert_eq!(d.query(), "");
        assert!(
            d.rows(Screen::Search).is_empty(),
            "results outlived the query"
        );
    }

    #[test]
    fn back_returns_to_the_screen_the_entry_was_opened_from() {
        for from in [
            Screen::Search,
            Screen::History,
            Screen::Favorites,
            Screen::Featured,
        ] {
            let mut d = furnished(from);
            open(&mut d, "iterate");
            assert_eq!(d.screen(), Screen::Entry);
            probe::click(&mut d, Target::Back);
            assert_eq!(d.screen(), from, "Back from an entry opened on {from:?}");
        }
    }

    #[test]
    fn the_featured_buttons_step_and_open() {
        let mut d = app();
        d.apply(Action::Go(Screen::Featured));
        let first = d.featured();
        probe::click(&mut d, Target::NextFeatured);
        assert_eq!(d.featured(), first + 1);
        probe::click(&mut d, Target::PrevFeatured);
        assert_eq!(d.featured(), first);
        probe::click(&mut d, Target::OpenFeatured);
        assert_eq!(d.screen(), Screen::Entry);
        assert_eq!(d.current(), Some(first));
    }

    #[test]
    fn the_history_can_be_emptied_from_its_own_screen() {
        let mut d = app();
        open(&mut d, "kernel");
        open(&mut d, "cache");
        d.apply(Action::Go(Screen::History));
        assert_eq!(d.history().len(), 2);
        probe::click(&mut d, Target::ClearHistory);
        assert!(d.history().is_empty());
        assert!(
            probe::rect_of(&d, Target::ClearHistory).is_none(),
            "an empty history still offers a Clear button"
        );
    }

    #[test]
    fn a_click_is_read_against_the_size_last_drawn() {
        // The old program tested clicks against a layout fixed at 800x600 no
        // matter what size the window was.
        let big = (1600.0, 1000.0);
        let small = (400.0, 500.0);
        let mut d = sized(big);
        let tab = probe::rect_of_sized(&d, Target::Tab(4), big).expect("no tab at the big size");
        d.resize(small.0, small.1);
        assert_ne!(
            d.target_at(tab.x + tab.w / 2.0, tab.y + tab.h / 2.0),
            Some(Target::Tab(4)),
            "a point that was tab 4 at {big:?} is still tab 4 at {small:?}"
        );
        probe::click_sized(&mut d, Target::Tab(4), MouseButton::Left, small);
        assert_eq!(d.screen(), Screen::Featured);
    }

    // ── Keyboard ───────────────────────────────────────────────────────────

    #[test]
    fn a_release_does_nothing() {
        let mut d = app();
        let before = describe(&d);
        let mut ev = probe::typing("k");
        ev.pressed = false;
        assert_eq!(probe::key(&mut d, &ev), EventResult::Ignored);
        assert_eq!(describe(&d), before, "a key release changed the state");
    }

    #[test]
    fn typing_searches_from_whatever_screen_you_are_on() {
        for screen in [Screen::History, Screen::Favorites, Screen::Featured] {
            let mut d = app();
            d.apply(Action::Go(screen));
            search_for(&mut d, "kernel");
            assert_eq!(
                d.screen(),
                Screen::Search,
                "typing on {screen:?} stayed put"
            );
            assert_eq!(d.query(), "kernel");
            assert_eq!(
                d.rows(Screen::Search).first().copied(),
                d.find_word("kernel")
            );
        }
    }

    #[test]
    fn the_slash_key_types_rather_than_being_a_shortcut_that_cannot_fire() {
        // Fault two: the bar advertised `[/] Search` while the handler read
        // `"/" | "f" if ctrl` — and a guard on a `|` pattern applies to both
        // alternatives, so a plain slash fell through to `_ => {}`.
        let mut d = app();
        search_for(&mut d, "/");
        assert_eq!(d.query(), "/", "a slash did not reach the query");
        for screen in Screen::ALL {
            let d = furnished(screen);
            assert!(
                !d.hint().contains("[/]"),
                "{screen:?} still advertises a slash shortcut: {:?}",
                d.hint()
            );
        }
    }

    #[test]
    fn escape_always_leads_somewhere() {
        // Fault nine: the first Escape cleared `search_active` while leaving
        // `screen == Search`, and the typing branch was gated on both — so the
        // keyboard went dead until Escape was pressed a second time.
        let mut d = app();
        open(&mut d, "kernel");
        search_for(&mut d, "abc");
        for round in 0..4 {
            probe::key(&mut d, &probe::press(Key::Escape));
            let before = d.query().to_string();
            search_for(&mut d, "z");
            assert_eq!(
                d.query(),
                format!("{before}z"),
                "the keyboard went dead after {} escapes",
                round + 1
            );
            probe::key(&mut d, &probe::press(Key::Backspace));
        }
    }

    #[test]
    fn tab_cycles_the_screens_and_skips_the_tab_with_nothing_behind_it() {
        let mut d = app();
        assert!(d.current().is_none());
        let mut seen = Vec::new();
        for _ in 0..SCREENS {
            probe::key(&mut d, &probe::press(Key::Tab));
            seen.push(d.screen());
        }
        assert!(
            !seen.contains(&Screen::Entry),
            "Tab landed on the empty entry screen: {seen:?}"
        );
        open(&mut d, "cache");
        let mut seen = Vec::new();
        for _ in 0..SCREENS {
            probe::key(&mut d, &probe::press(Key::Tab));
            seen.push(d.screen());
        }
        assert!(
            seen.contains(&Screen::Entry),
            "Tab skipped a live entry: {seen:?}"
        );
    }

    #[test]
    fn shift_tab_cycles_the_other_way() {
        let mut d = app();
        open(&mut d, "cache");
        d.apply(Action::Go(Screen::Search));
        probe::key(&mut d, &probe::press(Key::Tab));
        let forward = d.screen();
        probe::key(&mut d, &probe::shift(Key::Tab));
        assert_eq!(d.screen(), Screen::Search, "Shift+Tab did not undo Tab");
        probe::key(&mut d, &probe::shift(Key::Tab));
        assert_ne!(d.screen(), forward, "Shift+Tab went the same way as Tab");
    }

    #[test]
    fn ctrl_and_a_digit_jump_straight_to_a_screen() {
        let mut d = app();
        open(&mut d, "cache");
        for (key, screen) in [
            (Key::Num1, Screen::Search),
            (Key::Num2, Screen::Entry),
            (Key::Num3, Screen::History),
            (Key::Num4, Screen::Favorites),
            (Key::Num5, Screen::Featured),
        ] {
            probe::key(&mut d, &probe::ctrl(key));
            assert_eq!(d.screen(), screen, "Ctrl+{key:?} went to the wrong screen");
        }
    }

    #[test]
    fn a_plain_digit_types_rather_than_jumping() {
        // Which is exactly why the jumps need Ctrl: a search field has to be
        // able to type a digit.
        let mut d = app();
        probe::type_str(&mut d, "1");
        assert_eq!(d.query(), "1");
        assert_eq!(d.screen(), Screen::Search);
    }

    #[test]
    fn ctrl_d_marks_the_open_entry_a_favourite() {
        let mut d = app();
        open(&mut d, "verbose");
        probe::key(&mut d, &probe::ctrl(Key::D));
        assert_eq!(d.favorites(), ["verbose"]);
        probe::key(&mut d, &probe::ctrl(Key::D));
        assert!(d.favorites().is_empty());
    }

    #[test]
    fn a_modifier_the_program_does_not_use_is_ignored() {
        for modifiers in [Modifiers::alt(), Modifiers::super_key()] {
            let mut d = app();
            let before = describe(&d);
            let ev = probe::press_with(Key::Down, modifiers);
            assert_eq!(probe::key(&mut d, &ev), EventResult::Ignored);
            assert_eq!(describe(&d), before, "{modifiers:?} was acted on");
        }
    }

    // ── Scrolling a list ───────────────────────────────────────────────────

    #[test]
    fn the_selection_never_leaves_the_rows_on_screen() {
        // Fault four: Down walked the selection to `len - 1` while the renderer
        // drew the first `visible` rows from index zero and nothing scrolled,
        // so on any long list the selection left the screen and Enter opened a
        // word the reader could not see.
        let mut d = sized(SHORT);
        search_for(&mut d, "e");
        let len = d.rows(Screen::Search).len();
        let visible = d.visible_rows(Screen::Search);
        assert!(
            len > visible,
            "this test needs more results than fit: {len} in {visible}"
        );
        for _ in 0..len + 4 {
            d.key_at(&probe::press(Key::Down), SHORT);
            let sel = d.selected(Screen::Search);
            let top = d.scroll_top(Screen::Search);
            assert!(
                sel >= top && sel < top + visible,
                "selection {sel} is outside the window [{top}, {})",
                top + visible
            );
            assert!(
                probe::is_visible_sized(&d, Target::Row(sel), SHORT),
                "row {sel} is selected and not on screen"
            );
        }
        assert_eq!(
            d.selected(Screen::Search),
            len - 1,
            "Down stopped short of the end"
        );
    }

    #[test]
    fn enter_opens_the_row_that_is_selected_after_scrolling() {
        let mut d = sized(SHORT);
        search_for(&mut d, "e");
        for _ in 0..d.visible_rows(Screen::Search) + 2 {
            d.key_at(&probe::press(Key::Down), SHORT);
        }
        let sel = d.selected(Screen::Search);
        assert!(sel > 0, "the list did not move");
        let wanted = d.word(d.rows(Screen::Search)[sel]).to_string();
        d.key_at(&probe::press(Key::Enter), SHORT);
        assert_eq!(d.word(d.current().unwrap()), wanted);
    }

    #[test]
    fn the_favourites_list_has_keys_of_its_own() {
        // Fault five: only the history screen handled Up and Down, so the
        // favourites list was frozen on its first row and Enter always opened
        // that one, whatever was highlighted.
        let mut d = app();
        for word in ["kernel", "cache", "verbose", "paradigm"] {
            open(&mut d, word);
            d.apply(Action::ToggleFavorite);
        }
        d.apply(Action::Go(Screen::Favorites));
        assert_eq!(d.rows(Screen::Favorites).len(), 4);
        probe::key(&mut d, &probe::press(Key::Down));
        probe::key(&mut d, &probe::press(Key::Down));
        assert_eq!(
            d.selected(Screen::Favorites),
            2,
            "Down did nothing on the favourites"
        );
        let wanted = d.word(d.rows(Screen::Favorites)[2]).to_string();
        probe::key(&mut d, &probe::press(Key::Enter));
        assert_eq!(d.word(d.current().unwrap()), wanted);
    }

    #[test]
    fn the_history_and_favourites_keep_separate_positions() {
        let mut d = app();
        for word in ["kernel", "cache", "verbose", "paradigm", "iterate"] {
            open(&mut d, word);
            d.apply(Action::ToggleFavorite);
        }
        d.apply(Action::Go(Screen::History));
        probe::key(&mut d, &probe::press(Key::Down));
        probe::key(&mut d, &probe::press(Key::Down));
        assert_eq!(d.selected(Screen::History), 2);
        d.apply(Action::Go(Screen::Favorites));
        assert_eq!(
            d.selected(Screen::Favorites),
            0,
            "the favourites inherited the history's position"
        );
        probe::key(&mut d, &probe::press(Key::Down));
        assert_eq!(d.selected(Screen::Favorites), 1);
        d.apply(Action::Go(Screen::History));
        assert_eq!(d.selected(Screen::History), 2, "the history lost its place");
    }

    #[test]
    fn home_and_end_reach_both_ends_of_a_list() {
        let mut d = sized(SHORT);
        search_for(&mut d, "e");
        let len = d.rows(Screen::Search).len();
        d.key_at(&probe::press(Key::End), SHORT);
        assert_eq!(d.selected(Screen::Search), len - 1);
        assert!(probe::is_visible_sized(&d, Target::Row(len - 1), SHORT));
        d.key_at(&probe::press(Key::Home), SHORT);
        assert_eq!(d.selected(Screen::Search), 0);
        assert_eq!(d.scroll_top(Screen::Search), 0);
        assert!(probe::is_visible_sized(&d, Target::Row(0), SHORT));
    }

    #[test]
    fn page_down_moves_further_than_down() {
        let mut d = sized(SHORT);
        search_for(&mut d, "e");
        d.key_at(&probe::press(Key::Down), SHORT);
        let one = d.selected(Screen::Search);
        d.key_at(&probe::press(Key::Home), SHORT);
        d.key_at(&probe::press(Key::PageDown), SHORT);
        assert!(
            d.selected(Screen::Search) > one,
            "PageDown moved no further than Down"
        );
        d.key_at(&probe::press(Key::PageUp), SHORT);
        assert_eq!(
            d.selected(Screen::Search),
            0,
            "PageUp did not undo PageDown"
        );
    }

    fn wheel(d: &mut Dictionary, dy: f32, size: (f32, f32)) -> EventResult {
        d.resize(size.0, size.1);
        handle_event(
            d,
            &Event::Mouse(MouseEvent {
                x: size.0 / 2.0,
                y: size.1 / 2.0,
                kind: MouseEventKind::Scroll { dx: 0.0, dy },
            }),
        )
    }

    #[test]
    fn the_wheel_scrolls_a_list_in_notches_not_pixels() {
        let mut d = sized(SHORT);
        search_for(&mut d, "e");
        assert_eq!(d.scroll_top(Screen::Search), 0);
        // A trackpad sends a stream of fifths of a notch. A converter that
        // rounded each event on its own would return zero forever and the list
        // would never move at all.
        for _ in 0..4 {
            wheel(&mut d, -0.2, SHORT);
        }
        assert!(
            d.scroll_top(Screen::Search) > 0,
            "four fifths of a notch scrolled nothing"
        );
        // And the selection stayed put: a wheel moves the window, not the
        // choice.
        assert_eq!(d.selected(Screen::Search), 0);
    }

    #[test]
    fn a_list_cannot_be_wheeled_past_either_of_its_ends() {
        let mut d = sized(SHORT);
        search_for(&mut d, "e");
        let len = d.rows(Screen::Search).len();
        let visible = d.visible_rows(Screen::Search);
        for _ in 0..50 {
            wheel(&mut d, -3.0, SHORT);
        }
        assert_eq!(d.scroll_top(Screen::Search), len - visible);
        for _ in 0..50 {
            wheel(&mut d, 3.0, SHORT);
        }
        assert_eq!(d.scroll_top(Screen::Search), 0);
    }

    #[test]
    fn a_half_scrolled_row_is_not_clickable_where_it_was_never_drawn() {
        // `Frame::clip` trims the recorded boxes as well as the ink, so no row
        // may keep a hit box sticking out of the pane it was drawn in.
        //
        // The list draws one row past the whole ones whenever the pane has a
        // strip left over for it, so that a list continuing below the fold
        // looks like it does. That peek row is what this test is really
        // about: without one the containment assertion is vacuous — every
        // whole row fits by construction — so the loop also counts the rows
        // the clip cut short and insists at least one window produced one.
        let mut trimmed = 0;
        for &size in WINDOWS {
            let mut d = sized(size);
            search_for(&mut d, "e");
            wheel(&mut d, -1.0, size);
            let l = d.layout();
            let pane = l.list_pane(Screen::Search);
            for (target, rect) in d.frame(size.0, size.1).hits() {
                if matches!(target, Target::Row(_)) {
                    assert!(
                        rect.y >= pane.y - 0.5 && rect.bottom() <= pane.bottom() + 0.5,
                        "{size:?}: {target:?} at {rect:?} leaves the pane {pane:?}"
                    );
                    if rect.h < l.row_h() - 0.5 {
                        trimmed += 1;
                    }
                }
            }
        }
        assert!(
            trimmed > 0,
            "no window drew a row cut short by the bottom of its pane, so \
             nothing here tested the clip"
        );
    }

    // ── Scrolling an entry ─────────────────────────────────────────────────

    #[test]
    fn a_long_entry_can_be_scrolled_to_its_last_line() {
        // Fault six: `dy + 20.0 < y + h` guards dropped the etymology, and
        // `detail_scroll` was written by Up/Down and read by nothing, so
        // nothing brought it back.
        let mut d = sized(CRAMPED);
        open(&mut d, "algorithm");
        assert!(
            d.entry_max_scroll() > 0.0,
            "algorithm fits 240x200 — this test needs a smaller window"
        );
        assert!(
            !shows(&d, CRAMPED, "Origin"),
            "the origin is visible without scrolling"
        );
        d.key_at(&probe::press(Key::End), CRAMPED);
        assert!(shows(&d, CRAMPED, "Origin"), "End did not reach the origin");
    }

    #[test]
    fn an_entry_cannot_be_scrolled_past_its_own_last_line() {
        let mut d = sized(CRAMPED);
        open(&mut d, "algorithm");
        let max = d.entry_max_scroll();
        for _ in 0..200 {
            d.key_at(&probe::press(Key::Down), CRAMPED);
        }
        assert!(
            near(d.entry_scroll(), max),
            "scrolled to {} of a possible {max}",
            d.entry_scroll()
        );
        for _ in 0..400 {
            d.key_at(&probe::press(Key::Up), CRAMPED);
        }
        assert!(
            near(d.entry_scroll(), 0.0),
            "scrolled above the first line, to {}",
            d.entry_scroll()
        );
    }

    #[test]
    fn an_entry_that_fits_does_not_scroll_at_all() {
        let big = (1600.0, 1200.0);
        let mut d = sized(big);
        open(&mut d, "they");
        assert!(
            near(d.entry_max_scroll(), 0.0),
            "a short entry wants {} pixels of scrolling at 1600x1200",
            d.entry_max_scroll()
        );
        d.key_at(&probe::press(Key::End), big);
        assert!(
            near(d.entry_scroll(), 0.0),
            "End scrolled an entry that fits"
        );
    }

    #[test]
    fn opening_a_second_entry_starts_it_at_the_top() {
        let mut d = sized(CRAMPED);
        open(&mut d, "algorithm");
        d.key_at(&probe::press(Key::End), CRAMPED);
        assert!(d.entry_scroll() > 0.0);
        open(&mut d, "kernel");
        assert!(
            near(d.entry_scroll(), 0.0),
            "the new entry inherited a scroll offset of {}",
            d.entry_scroll()
        );
    }

    #[test]
    fn the_wheel_scrolls_an_open_entry() {
        let mut d = sized(CRAMPED);
        open(&mut d, "algorithm");
        assert_eq!(wheel(&mut d, -1.0, CRAMPED), EventResult::Consumed);
        assert!(d.entry_scroll() > 0.0, "a notch down scrolled nothing");
        for _ in 0..50 {
            wheel(&mut d, 1.0, CRAMPED);
        }
        assert!(
            near(d.entry_scroll(), 0.0),
            "the wheel could not get back to the top: {}",
            d.entry_scroll()
        );
    }

    #[test]
    fn the_blocks_of_an_entry_account_for_all_of_its_height() {
        let l = Layout::new(CRAMPED.0, CRAMPED.1);
        let d = sized(CRAMPED);
        for i in 0..d.entries().len() {
            let blocks = d.entry_blocks(i, &l, l.entry_pane().w);
            let summed: f32 = blocks.iter().map(|b| b.space() + b.height()).sum();
            assert!(
                near(blocks_height(&blocks), summed),
                "{}: the total disagrees with its parts",
                d.word(i)
            );
            assert!(
                blocks_height(&blocks) > 0.0,
                "{}: an entry of no height",
                d.word(i)
            );
        }
    }

    #[test]
    fn every_part_of_an_entry_reaches_its_blocks() {
        let size = (1200.0, 900.0);
        let l = Layout::new(size.0, size.1);
        let d = sized(size);
        for (i, entry) in d.entries().iter().enumerate() {
            let blocks = d.entry_blocks(i, &l, l.entry_pane().w);
            let drawn: String = blocks
                .iter()
                .map(|b| match b {
                    Block::Line { text, .. } => text.clone(),
                    Block::Chips { words, .. } => words
                        .iter()
                        .map(|(w, _)| w.as_str())
                        .collect::<Vec<_>>()
                        .join(" "),
                })
                .collect::<Vec<_>>()
                .join(" ");
            assert!(
                drawn.contains(&entry.word),
                "{}: the word itself is missing",
                entry.word
            );
            for word in entry
                .synonyms
                .iter()
                .chain(&entry.antonyms)
                .chain(&entry.related)
            {
                assert!(
                    drawn.contains(word),
                    "{}: the cross-reference {word} is missing",
                    entry.word
                );
            }
            assert!(
                entry.etymology.is_empty() || drawn.contains("Origin"),
                "{}: an etymology with no heading",
                entry.word
            );
        }
    }

    // ── Search ─────────────────────────────────────────────────────────────

    #[test]
    fn an_empty_query_matches_nothing() {
        let mut d = app();
        assert!(d.rows(Screen::Search).is_empty());
        search_for(&mut d, "   ");
        assert!(
            d.rows(Screen::Search).is_empty(),
            "whitespace matched something"
        );
    }

    #[test]
    fn an_exact_match_comes_first() {
        let mut d = app();
        search_for(&mut d, "cache");
        assert_eq!(d.word(d.rows(Screen::Search)[0]), "cache");
    }

    #[test]
    fn a_prefix_beats_a_substring_which_beats_a_definition() {
        let entries = build_dictionary();
        let cache = entries.iter().find(|e| e.word == "cache").unwrap();
        assert_eq!(rank(cache, "cache"), Some(0));
        assert_eq!(rank(cache, "cac"), Some(1));
        let iterate = entries.iter().find(|e| e.word == "iterate").unwrap();
        assert_eq!(rank(iterate, "terat"), Some(2), "a substring should rank 2");
        let by_definition = rank(iterate, &iterate.definitions[0].text.to_lowercase());
        assert!(
            by_definition > Some(2),
            "a definition hit outranked a substring: {by_definition:?}"
        );
    }

    #[test]
    fn the_search_reaches_synonyms_antonyms_and_related_words() {
        // The old search read neither antonyms nor `related` — the latter was
        // built, stored on every entry, and looked at by nothing at all.
        let entries = build_dictionary();
        let mut found = [0_usize; 3];
        for e in &entries {
            for (slot, list) in [&e.synonyms, &e.antonyms, &e.related]
                .into_iter()
                .enumerate()
            {
                let wanted = u8::try_from(slot + 4).unwrap_or(u8::MAX);
                for w in list {
                    if rank(e, &w.to_lowercase()) == Some(wanted) {
                        found[slot] += 1;
                    }
                }
            }
        }
        assert!(found[0] > 0, "nothing is findable by its synonyms");
        assert!(found[1] > 0, "nothing is findable by its antonyms");
        assert!(found[2] > 0, "nothing is findable by its related words");
    }

    #[test]
    fn a_word_reachable_only_through_a_related_list_is_found() {
        let mut d = app();
        let entries = build_dictionary();
        // A related word that is neither part of its entry's own spelling nor
        // of any definition, so the only route to it is the `related` list.
        let probe_word = entries
            .iter()
            .flat_map(|e| e.related.iter().map(move |w| (e, w)))
            .find(|(e, w)| rank(e, &w.to_lowercase()) == Some(6))
            .map(|(e, w)| (e.word.clone(), w.to_lowercase()))
            .expect("no entry is reachable only through its related list");
        search_for(&mut d, &probe_word.1);
        let words: Vec<&str> = d.rows(Screen::Search).iter().map(|&i| d.word(i)).collect();
        assert!(
            words.contains(&probe_word.0.as_str()),
            "{} is not among the results for {}",
            probe_word.0,
            probe_word.1
        );
    }

    #[test]
    fn the_results_are_ordered_by_rank_and_then_by_the_word_list() {
        let mut d = app();
        search_for(&mut d, "e");
        let rows = d.rows(Screen::Search);
        let ranks: Vec<u8> = rows
            .iter()
            .map(|&i| rank(d.entry(i).unwrap(), "e").unwrap())
            .collect();
        assert!(
            ranks.windows(2).all(|w| w[0] <= w[1]),
            "out of rank order: {ranks:?}"
        );
        // Equal ranks keep dictionary order: a stable sort, not a scramble.
        for w in rows.windows(2) {
            let (a, b) = (w[0], w[1]);
            if rank(d.entry(a).unwrap(), "e") == rank(d.entry(b).unwrap(), "e") {
                assert!(a < b, "entries of equal rank came out in the wrong order");
            }
        }
    }

    #[test]
    fn the_search_ignores_case_and_surrounding_space() {
        let mut a = app();
        search_for(&mut a, "KERNEL");
        let mut b = app();
        search_for(&mut b, "  kernel  ");
        assert_eq!(a.rows(Screen::Search), b.rows(Screen::Search));
        assert_eq!(a.word(a.rows(Screen::Search)[0]), "kernel");
    }

    #[test]
    fn every_entry_can_be_found_by_its_own_name() {
        let reference = app();
        for i in 0..reference.entries().len() {
            let word = reference.word(i).to_string();
            let mut d = app();
            search_for(&mut d, &word);
            assert_eq!(
                d.rows(Screen::Search).first().copied(),
                Some(i),
                "{word} is not the first result for itself"
            );
        }
    }

    #[test]
    fn backspace_narrows_the_query_and_widens_the_results() {
        let mut d = app();
        search_for(&mut d, "cach");
        let narrow = d.rows(Screen::Search).len();
        probe::key(&mut d, &probe::press(Key::Backspace));
        assert_eq!(d.query(), "cac");
        probe::key(&mut d, &probe::press(Key::Backspace));
        probe::key(&mut d, &probe::press(Key::Backspace));
        assert_eq!(d.query(), "c");
        assert!(
            d.rows(Screen::Search).len() > narrow,
            "a shorter query matched no more than a longer one"
        );
    }

    #[test]
    fn ctrl_backspace_empties_the_query_in_one_go() {
        let mut d = app();
        search_for(&mut d, "concurrency");
        probe::key(&mut d, &probe::ctrl(Key::Backspace));
        assert_eq!(d.query(), "");
    }

    // ── The featured word ──────────────────────────────────────────────────

    #[test]
    fn the_featured_word_can_be_stepped_through_the_whole_dictionary() {
        // Fault eight: the "word of the day" was permanently entry zero. No
        // wall clock is reachable from an app, so it is a *featured* word the
        // reader steps through rather than a date the program cannot know.
        let mut d = app();
        d.apply(Action::Go(Screen::Featured));
        let mut seen = vec![d.featured()];
        for _ in 1..d.entries().len() {
            probe::key(&mut d, &probe::press(Key::Right));
            seen.push(d.featured());
        }
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(
            seen.len(),
            d.entries().len(),
            "Right did not reach every entry"
        );
    }

    #[test]
    fn stepping_the_featured_word_wraps_both_ways() {
        let mut d = app();
        d.apply(Action::Go(Screen::Featured));
        let last = d.entries().len() - 1;
        probe::key(&mut d, &probe::press(Key::Left));
        assert_eq!(d.featured(), last, "Left from the first did not wrap");
        probe::key(&mut d, &probe::press(Key::Right));
        assert_eq!(d.featured(), 0, "Right from the last did not wrap");
    }

    #[test]
    fn the_arrows_only_step_the_featured_word_on_the_featured_screen() {
        let mut d = app();
        search_for(&mut d, "e");
        let before = d.featured();
        probe::key(&mut d, &probe::press(Key::Right));
        assert_eq!(
            d.featured(),
            before,
            "Right stepped the featured word from the search screen"
        );
    }

    // ── History and favourites ─────────────────────────────────────────────

    #[test]
    fn looking_a_word_up_puts_it_at_the_top_of_the_history() {
        let mut d = app();
        open(&mut d, "kernel");
        open(&mut d, "cache");
        assert_eq!(d.history(), ["cache", "kernel"]);
    }

    #[test]
    fn looking_the_same_word_up_twice_leaves_one_entry() {
        let mut d = app();
        open(&mut d, "kernel");
        open(&mut d, "cache");
        open(&mut d, "kernel");
        assert_eq!(d.history(), ["kernel", "cache"]);
    }

    #[test]
    fn the_history_never_outgrows_its_limit() {
        let mut d = app();
        let n = d.entries().len();
        for round in 0..(HISTORY_LIMIT / n + 2) {
            for i in 0..n {
                d.apply(Action::Open(i));
            }
            assert!(
                d.history().len() <= HISTORY_LIMIT,
                "round {round}: the history reached {}",
                d.history().len()
            );
        }
        // With fewer distinct words than the limit it never exceeds the count.
        assert_eq!(d.history().len(), n);
    }

    #[test]
    fn a_favourite_survives_being_looked_at_from_another_screen() {
        let mut d = app();
        open(&mut d, "resilient");
        d.apply(Action::ToggleFavorite);
        d.apply(Action::Go(Screen::Favorites));
        probe::key(&mut d, &probe::press(Key::Enter));
        assert_eq!(d.word(d.current().unwrap()), "resilient");
        assert!(d.is_favorite(), "the favourite came off on being opened");
    }

    #[test]
    fn the_entry_tab_is_refused_until_something_is_open_and_says_so() {
        let mut d = app();
        probe::click(&mut d, Target::Tab(Screen::Entry.index()));
        assert_eq!(
            d.screen(),
            Screen::Search,
            "landed on an empty entry screen"
        );
        assert!(
            d.status().contains("Open a word"),
            "the click was swallowed in silence: {:?}",
            d.status()
        );
        open(&mut d, "nuance");
        d.apply(Action::Go(Screen::Search));
        probe::click(&mut d, Target::Tab(Screen::Entry.index()));
        assert_eq!(d.screen(), Screen::Entry);
    }

    // ── The word list itself ───────────────────────────────────────────────

    #[test]
    fn every_entry_is_complete() {
        for e in build_dictionary() {
            assert!(!e.word.is_empty(), "an entry with no word");
            assert!(!e.pronunciation.is_empty(), "{}: no pronunciation", e.word);
            assert!(!e.definitions.is_empty(), "{}: no definitions", e.word);
            assert!(!e.etymology.is_empty(), "{}: no etymology", e.word);
            for d in &e.definitions {
                assert!(!d.text.is_empty(), "{}: an empty definition", e.word);
                assert!(
                    d.example.as_ref().is_none_or(|x| !x.is_empty()),
                    "{}: an empty example",
                    e.word
                );
            }
        }
    }

    #[test]
    fn the_word_list_has_no_duplicates() {
        let mut words: Vec<String> = build_dictionary()
            .iter()
            .map(|e| e.word.to_lowercase())
            .collect();
        let before = words.len();
        words.sort();
        words.dedup();
        assert_eq!(words.len(), before, "the word list repeats itself");
    }

    #[test]
    fn every_part_of_speech_the_program_can_label_is_actually_used() {
        // A part of speech no entry carries is a label with nothing behind it:
        // its colour is never seen and its abbreviation never reached. The old
        // list used three of the ten, and a blanket `#![allow(dead_code)]` on
        // line one is what kept that quiet.
        let entries = build_dictionary();
        for part in [
            PartOfSpeech::Noun,
            PartOfSpeech::Verb,
            PartOfSpeech::Adjective,
            PartOfSpeech::Adverb,
            PartOfSpeech::Pronoun,
            PartOfSpeech::Preposition,
            PartOfSpeech::Conjunction,
            PartOfSpeech::Interjection,
            PartOfSpeech::Determiner,
            PartOfSpeech::Abbreviation,
        ] {
            assert!(
                entries
                    .iter()
                    .any(|e| e.definitions.iter().any(|d| d.part_of_speech == part)),
                "no entry is a {}",
                part.label()
            );
            assert!(!part.label().is_empty());
            assert!(!part.short().is_empty());
        }
    }

    #[test]
    fn a_cross_reference_that_names_an_entry_arrives_as_a_link() {
        let d = app();
        let l = d.layout();
        let mut live = 0;
        for i in 0..d.entries().len() {
            for block in d.entry_blocks(i, &l, l.entry_pane().w) {
                if let Block::Chips { words, .. } = block {
                    for (word, link) in words {
                        assert_eq!(
                            link,
                            d.find_word(&word),
                            "{word} on {}: the link disagrees with the dictionary",
                            d.word(i)
                        );
                        if link.is_some() {
                            live += 1;
                        }
                    }
                }
            }
        }
        assert!(live > 0, "not one cross-reference leads anywhere");
    }

    #[test]
    fn every_entry_leads_somewhere_else_in_the_dictionary() {
        // Wiring the chips up found that the data behind them was empty: of
        // 265 cross-references in the original word list, exactly one named a
        // word the dictionary had, so a feature whose whole point is getting
        // from one entry to another got you nowhere from twenty-nine of the
        // thirty. The `related` lists now name siblings on purpose.
        let d = app();
        let l = d.layout();
        for i in 0..d.entries().len() {
            let live: Vec<usize> = d
                .entry_blocks(i, &l, l.entry_pane().w)
                .into_iter()
                .flat_map(|b| match b {
                    Block::Chips { words, .. } => {
                        words.into_iter().filter_map(|(_, link)| link).collect()
                    }
                    Block::Line { .. } => Vec::new(),
                })
                .collect();
            assert!(
                !live.is_empty(),
                "{}: not one cross-reference leads to another entry",
                d.word(i)
            );
            assert!(
                live.iter().any(|&j| j != i),
                "{}: every cross-reference leads back to itself",
                d.word(i)
            );
        }
    }

    // ── What the window says ───────────────────────────────────────────────

    #[test]
    fn the_status_bar_says_something_on_every_screen_and_it_is_drawn() {
        for screen in Screen::ALL {
            let d = furnished(screen);
            assert!(
                !d.status().is_empty(),
                "{screen:?} has an empty status line"
            );
            assert!(!d.hint().is_empty(), "{screen:?} has no hint");
            assert!(
                shows(&d, Dictionary::SIZE, d.status()),
                "{screen:?}: the status line is not drawn"
            );
        }
    }

    #[test]
    fn a_resize_event_reaches_the_layout() {
        let mut d = app();
        handle_event(
            &mut d,
            &Event::Resize {
                width: 640,
                height: 480,
            },
        );
        assert_eq!(d.layout().window, Rect::new(0.0, 0.0, 640.0, 480.0));
    }
}
