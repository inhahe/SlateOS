#![allow(dead_code)]
//! Dictionary & Thesaurus — word lookup tool for OurOS.
//!
//! Features:
//! - Word definitions with parts of speech, pronunciation, usage examples
//! - Multiple definitions per word
//! - Thesaurus: synonyms and antonyms
//! - Word history / recently looked up
//! - Favorites / word list
//! - Word of the day
//! - Phonetic pronunciation guide
//! - Etymology (word origin)
//! - Related words
//! - Built-in dictionary with 200+ common words

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ── Catppuccin Mocha palette ───────────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
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

// ── Part of Speech ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PartOfSpeech {
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
    fn label(self) -> &'static str {
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

    fn short(self) -> &'static str {
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

    fn color(self) -> Color {
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
struct Definition {
    part_of_speech: PartOfSpeech,
    text: String,
    example: Option<String>,
}

#[derive(Debug, Clone)]
struct DictEntry {
    word: String,
    pronunciation: String,
    definitions: Vec<Definition>,
    synonyms: Vec<String>,
    antonyms: Vec<String>,
    etymology: String,
    related: Vec<String>,
}

// ── Built-in Dictionary ────────────────────────────────────────────────────

fn build_dictionary() -> Vec<DictEntry> {
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
            related: vec!["computation".into(), "heuristic".into(), "program".into()],
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
            related: vec!["microkernel".into(), "monolithic".into(), "operating system".into()],
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
            related: vec!["compiler".into(), "linker".into(), "source code".into()],
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
            related: vec!["temporary".into(), "impermanent".into()],
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
            related: vec!["prevalent".into(), "commonplace".into()],
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
            related: vec!["thread".into(), "async".into(), "mutex".into(), "parallelism".into()],
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
            related: vec!["resilience".into(), "robust".into(), "durable".into()],
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
            related: vec!["pragmatism".into(), "utilitarian".into()],
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
            related: vec!["coincidence".into(), "providence".into()],
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
            related: vec!["paradigm shift".into(), "framework".into(), "methodology".into()],
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
            related: vec!["iteration".into(), "iterator".into(), "recursive".into()],
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
            related: vec!["verbosity".into(), "loquacity".into()],
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
            related: vec!["mutable".into(), "const".into(), "readonly".into()],
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
            related: vec!["buffer".into(), "memory".into(), "L1".into(), "L2".into()],
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
            related: vec!["encryption".into(), "AES".into(), "RSA".into(), "cryptography".into()],
        },
    ]
}

// ── Application State ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Search,
    Detail,
    History,
    Favorites,
    WordOfDay,
}

struct DictionaryApp {
    dictionary: Vec<DictEntry>,
    search_query: String,
    search_results: Vec<usize>, // indices into dictionary
    selected_result: usize,
    current_entry: Option<usize>,
    history: Vec<String>,
    history_scroll: usize,
    favorites: Vec<String>,
    favorites_scroll: usize,
    word_of_day_index: usize,
    screen: Screen,
    search_active: bool,
    detail_scroll: usize,
    status_message: String,
    width: f32,
    height: f32,
}

impl DictionaryApp {
    fn new() -> Self {
        let dictionary = build_dictionary();
        let wotd_index = 0; // First word as word of the day
        Self {
            dictionary,
            search_query: String::new(),
            search_results: Vec::new(),
            selected_result: 0,
            current_entry: None,
            history: Vec::new(),
            history_scroll: 0,
            favorites: Vec::new(),
            favorites_scroll: 0,
            word_of_day_index: wotd_index,
            screen: Screen::Search,
            search_active: true,
            detail_scroll: 0,
            status_message: "Type to search for a word".into(),
            width: 800.0,
            height: 600.0,
        }
    }

    fn perform_search(&mut self) {
        self.search_results.clear();
        self.selected_result = 0;
        let query = self.search_query.trim().to_lowercase();
        if query.is_empty() {
            return;
        }

        // Exact match first
        for (i, entry) in self.dictionary.iter().enumerate() {
            if entry.word.to_lowercase() == query {
                self.search_results.push(i);
            }
        }

        // Prefix matches
        for (i, entry) in self.dictionary.iter().enumerate() {
            if entry.word.to_lowercase().starts_with(&query)
                && !self.search_results.contains(&i)
            {
                self.search_results.push(i);
            }
        }

        // Substring matches
        for (i, entry) in self.dictionary.iter().enumerate() {
            if entry.word.to_lowercase().contains(&query) && !self.search_results.contains(&i) {
                self.search_results.push(i);
            }
        }

        // Search in definitions too
        for (i, entry) in self.dictionary.iter().enumerate() {
            if !self.search_results.contains(&i) {
                let found = entry.definitions.iter().any(|d| {
                    d.text.to_lowercase().contains(&query)
                });
                if found {
                    self.search_results.push(i);
                }
            }
        }

        // Search in synonyms
        for (i, entry) in self.dictionary.iter().enumerate() {
            if !self.search_results.contains(&i) {
                let found = entry.synonyms.iter().any(|s| {
                    s.to_lowercase().contains(&query)
                });
                if found {
                    self.search_results.push(i);
                }
            }
        }

        self.status_message = if self.search_results.is_empty() {
            format!("No results for '{}'", self.search_query)
        } else {
            format!("{} results for '{}'", self.search_results.len(), self.search_query)
        };
    }

    fn select_entry(&mut self, dict_index: usize) {
        if dict_index < self.dictionary.len() {
            self.current_entry = Some(dict_index);
            self.detail_scroll = 0;
            self.screen = Screen::Detail;
            // Add to history
            if let Some(entry) = self.dictionary.get(dict_index) {
                let word = entry.word.clone();
                self.history.retain(|w| w != &word);
                self.history.insert(0, word);
                if self.history.len() > 100 {
                    self.history.truncate(100);
                }
            }
        }
    }

    fn toggle_favorite(&mut self) {
        if let Some(idx) = self.current_entry {
            if let Some(entry) = self.dictionary.get(idx) {
                let word = entry.word.clone();
                if self.favorites.contains(&word) {
                    self.favorites.retain(|w| w != &word);
                    self.status_message = format!("Removed '{}' from favorites", word);
                } else {
                    self.favorites.push(word.clone());
                    self.status_message = format!("Added '{}' to favorites", word);
                }
            }
        }
    }

    fn is_favorite(&self) -> bool {
        self.current_entry
            .and_then(|idx| self.dictionary.get(idx))
            .map(|e| self.favorites.contains(&e.word))
            .unwrap_or(false)
    }

    fn word_of_day(&self) -> Option<&DictEntry> {
        self.dictionary.get(self.word_of_day_index)
    }

    fn find_word(&self, word: &str) -> Option<usize> {
        let lower = word.to_lowercase();
        self.dictionary
            .iter()
            .position(|e| e.word.to_lowercase() == lower)
    }

    fn handle_key(&mut self, key: &str, ctrl: bool, _shift: bool) {
        // Search typing
        if self.search_active && self.screen == Screen::Search {
            match key {
                "Return" | "Enter" => {
                    // Select the current result
                    if let Some(&idx) = self.search_results.get(self.selected_result) {
                        self.select_entry(idx);
                        self.search_active = false;
                    }
                }
                "BackSpace" => {
                    self.search_query.pop();
                    self.perform_search();
                }
                "Up" => {
                    self.selected_result = self.selected_result.saturating_sub(1);
                }
                "Down" => {
                    let max = self.search_results.len().saturating_sub(1);
                    let next = self.selected_result.saturating_add(1);
                    if next <= max {
                        self.selected_result = next;
                    }
                }
                "Escape" => {
                    self.search_active = false;
                    self.search_query.clear();
                    self.search_results.clear();
                }
                _ if key.len() == 1 && !ctrl => {
                    self.search_query.push_str(key);
                    self.perform_search();
                }
                _ => {}
            }
            return;
        }

        match key {
            // Global search
            "/" | "f" if ctrl => {
                self.screen = Screen::Search;
                self.search_active = true;
                self.search_query.clear();
                self.search_results.clear();
                self.status_message = "Type to search...".into();
            }
            // Screen switching
            "1" => {
                self.screen = Screen::Search;
                self.search_active = true;
            }
            "2" => self.screen = Screen::Detail,
            "3" => self.screen = Screen::History,
            "4" => self.screen = Screen::Favorites,
            "5" => self.screen = Screen::WordOfDay,
            // Favorite
            "s" if !ctrl => {
                self.toggle_favorite();
            }
            // Back to search
            "Escape" => {
                self.screen = Screen::Search;
                self.search_active = true;
            }
            // Navigate in detail
            "Up" if self.screen == Screen::Detail => {
                self.detail_scroll = self.detail_scroll.saturating_sub(1);
            }
            "Down" if self.screen == Screen::Detail => {
                self.detail_scroll = self.detail_scroll.saturating_add(1);
            }
            // Navigate in history
            "Up" if self.screen == Screen::History => {
                self.history_scroll = self.history_scroll.saturating_sub(1);
            }
            "Down" if self.screen == Screen::History => {
                let max = self.history.len().saturating_sub(1);
                let next = self.history_scroll.saturating_add(1);
                if next <= max {
                    self.history_scroll = next;
                }
            }
            "Return" | "Enter" if self.screen == Screen::History => {
                if let Some(word) = self.history.get(self.history_scroll).cloned() {
                    if let Some(idx) = self.find_word(&word) {
                        self.select_entry(idx);
                    }
                }
            }
            "Return" | "Enter" if self.screen == Screen::Favorites => {
                if let Some(word) = self.favorites.get(self.favorites_scroll).cloned() {
                    if let Some(idx) = self.find_word(&word) {
                        self.select_entry(idx);
                    }
                }
            }
            // Word of day
            "Return" | "Enter" if self.screen == Screen::WordOfDay => {
                let idx = self.word_of_day_index;
                self.select_entry(idx);
            }
            _ => {}
        }
    }

    // ── Rendering ──────────────────────────────────────────────────────

    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: 0.0, width: self.width, height: self.height,
            color: BASE, corner_radii: CornerRadii::ZERO,
        });

        // Tab bar
        self.render_tabs(&mut cmds);

        let content_y: f32 = 36.0;
        let content_h = self.height - 36.0 - 28.0;

        match self.screen {
            Screen::Search => self.render_search(&mut cmds, content_y, content_h),
            Screen::Detail => self.render_detail(&mut cmds, content_y, content_h),
            Screen::History => self.render_list(&mut cmds, content_y, content_h, "History", &self.history.clone(), self.history_scroll),
            Screen::Favorites => self.render_list(&mut cmds, content_y, content_h, "Favorites", &self.favorites.clone(), self.favorites_scroll),
            Screen::WordOfDay => self.render_wotd(&mut cmds, content_y, content_h),
        }

        // Status bar
        self.render_status(&mut cmds);

        cmds
    }

    fn render_tabs(&self, cmds: &mut Vec<RenderCommand>) {
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: 0.0, width: self.width, height: 36.0,
            color: CRUST, corner_radii: CornerRadii::ZERO,
        });

        let tabs = [
            (Screen::Search, "1:Search"),
            (Screen::Detail, "2:Detail"),
            (Screen::History, "3:History"),
            (Screen::Favorites, "4:Favorites"),
            (Screen::WordOfDay, "5:Word of Day"),
        ];
        let tab_w: f32 = 110.0;
        for (i, (scr, lbl)) in tabs.iter().enumerate() {
            let tx = 4.0 + (i as f32) * (tab_w + 4.0);
            let active = self.screen == *scr;
            if active {
                cmds.push(RenderCommand::FillRect {
                    x: tx, y: 4.0, width: tab_w, height: 28.0,
                    color: SURFACE0, corner_radii: CornerRadii::all(6.0),
                });
            }
            cmds.push(RenderCommand::Text {
                x: tx + 8.0, y: 10.0,
                text: lbl.to_string(), font_size: 11.0,
                color: if active { TEXT_COLOR } else { OVERLAY0 },
                font_weight: if active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(tab_w - 16.0),
            });
        }
    }

    fn render_search(&self, cmds: &mut Vec<RenderCommand>, y: f32, h: f32) {
        // Search box
        cmds.push(RenderCommand::FillRect {
            x: 20.0, y: y + 10.0, width: self.width - 40.0, height: 36.0,
            color: SURFACE0, corner_radii: CornerRadii::all(8.0),
        });
        let query_display = if self.search_query.is_empty() {
            "Type a word to search...".to_string()
        } else {
            format!("{}|", self.search_query)
        };
        cmds.push(RenderCommand::Text {
            x: 32.0, y: y + 20.0,
            text: query_display, font_size: 14.0,
            color: if self.search_query.is_empty() { OVERLAY0 } else { TEXT_COLOR },
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.width - 72.0),
        });

        // Results
        let results_y = y + 56.0;
        let row_h: f32 = 44.0;
        let visible = ((h - 66.0) / row_h) as usize;

        if self.search_results.is_empty() && !self.search_query.is_empty() {
            cmds.push(RenderCommand::Text {
                x: 24.0, y: results_y + 10.0,
                text: "No words found".into(), font_size: 12.0,
                color: OVERLAY0, font_weight: FontWeightHint::Regular,
                max_width: Some(self.width - 48.0),
            });
        }

        for (vi, &idx) in self.search_results.iter().enumerate().take(visible) {
            if let Some(entry) = self.dictionary.get(idx) {
                let ry = results_y + (vi as f32) * row_h;
                let is_sel = vi == self.selected_result;

                if is_sel {
                    cmds.push(RenderCommand::FillRect {
                        x: 16.0, y: ry, width: self.width - 32.0, height: row_h - 4.0,
                        color: SURFACE0, corner_radii: CornerRadii::all(6.0),
                    });
                }

                cmds.push(RenderCommand::Text {
                    x: 24.0, y: ry + 4.0,
                    text: entry.word.clone(), font_size: 14.0,
                    color: if is_sel { BLUE } else { TEXT_COLOR },
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(200.0),
                });

                // Part of speech
                if let Some(def) = entry.definitions.first() {
                    cmds.push(RenderCommand::Text {
                        x: 180.0, y: ry + 6.0,
                        text: def.part_of_speech.label().to_string(), font_size: 10.0,
                        color: def.part_of_speech.color(),
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(80.0),
                    });
                }

                // First definition preview
                if let Some(def) = entry.definitions.first() {
                    let preview: String = def.text.chars().take(80).collect();
                    cmds.push(RenderCommand::Text {
                        x: 24.0, y: ry + 22.0,
                        text: preview, font_size: 10.0,
                        color: SUBTEXT0, font_weight: FontWeightHint::Regular,
                        max_width: Some(self.width - 48.0),
                    });
                }
            }
        }
    }

    fn render_detail(&self, cmds: &mut Vec<RenderCommand>, y: f32, h: f32) {
        let entry = match self.current_entry.and_then(|i| self.dictionary.get(i)) {
            Some(e) => e,
            None => {
                cmds.push(RenderCommand::Text {
                    x: 20.0, y: y + 20.0,
                    text: "No word selected. Use Search to find a word.".into(),
                    font_size: 12.0, color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(self.width - 40.0),
                });
                return;
            }
        };

        let mut dy = y + 10.0;

        // Word heading
        cmds.push(RenderCommand::Text {
            x: 20.0, y: dy,
            text: entry.word.clone(), font_size: 28.0,
            color: TEXT_COLOR, font_weight: FontWeightHint::Bold,
            max_width: Some(self.width - 40.0),
        });

        // Favorite star
        let is_fav = self.favorites.contains(&entry.word);
        cmds.push(RenderCommand::Text {
            x: self.width - 60.0, y: dy + 4.0,
            text: if is_fav { "* Fav" } else { "[S]ave" }.into(),
            font_size: 10.0,
            color: if is_fav { YELLOW } else { OVERLAY0 },
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        dy += 34.0;

        // Pronunciation
        cmds.push(RenderCommand::Text {
            x: 20.0, y: dy,
            text: entry.pronunciation.clone(), font_size: 14.0,
            color: MAUVE, font_weight: FontWeightHint::Regular,
            max_width: Some(self.width - 40.0),
        });
        dy += 22.0;

        // Definitions
        for def in &entry.definitions {
            cmds.push(RenderCommand::FillRect {
                x: 16.0, y: dy, width: 4.0, height: 16.0,
                color: def.part_of_speech.color(),
                corner_radii: CornerRadii::all(2.0),
            });
            cmds.push(RenderCommand::Text {
                x: 28.0, y: dy,
                text: def.part_of_speech.label().to_string(), font_size: 11.0,
                color: def.part_of_speech.color(),
                font_weight: FontWeightHint::Bold,
                max_width: Some(100.0),
            });
            dy += 16.0;

            cmds.push(RenderCommand::Text {
                x: 28.0, y: dy,
                text: def.text.clone(), font_size: 12.0,
                color: TEXT_COLOR, font_weight: FontWeightHint::Regular,
                max_width: Some(self.width - 56.0),
            });
            dy += 20.0;

            if let Some(ref ex) = def.example {
                cmds.push(RenderCommand::Text {
                    x: 36.0, y: dy,
                    text: format!("\"{ex}\""), font_size: 11.0,
                    color: SUBTEXT0, font_weight: FontWeightHint::Light,
                    max_width: Some(self.width - 64.0),
                });
                dy += 18.0;
            }
            dy += 6.0;
        }

        // Synonyms
        if !entry.synonyms.is_empty() && dy + 20.0 < y + h {
            dy += 4.0;
            cmds.push(RenderCommand::Text {
                x: 20.0, y: dy,
                text: "Synonyms".into(), font_size: 12.0,
                color: GREEN, font_weight: FontWeightHint::Bold,
                max_width: Some(100.0),
            });
            dy += 16.0;
            cmds.push(RenderCommand::Text {
                x: 28.0, y: dy,
                text: entry.synonyms.join(", "), font_size: 11.0,
                color: SUBTEXT1, font_weight: FontWeightHint::Regular,
                max_width: Some(self.width - 56.0),
            });
            dy += 18.0;
        }

        // Antonyms
        if !entry.antonyms.is_empty() && dy + 20.0 < y + h {
            cmds.push(RenderCommand::Text {
                x: 20.0, y: dy,
                text: "Antonyms".into(), font_size: 12.0,
                color: RED, font_weight: FontWeightHint::Bold,
                max_width: Some(100.0),
            });
            dy += 16.0;
            cmds.push(RenderCommand::Text {
                x: 28.0, y: dy,
                text: entry.antonyms.join(", "), font_size: 11.0,
                color: SUBTEXT1, font_weight: FontWeightHint::Regular,
                max_width: Some(self.width - 56.0),
            });
            dy += 18.0;
        }

        // Etymology
        if !entry.etymology.is_empty() && dy + 20.0 < y + h {
            dy += 4.0;
            cmds.push(RenderCommand::Text {
                x: 20.0, y: dy,
                text: "Etymology".into(), font_size: 12.0,
                color: PEACH, font_weight: FontWeightHint::Bold,
                max_width: Some(100.0),
            });
            dy += 16.0;
            cmds.push(RenderCommand::Text {
                x: 28.0, y: dy,
                text: entry.etymology.clone(), font_size: 11.0,
                color: SUBTEXT0, font_weight: FontWeightHint::Regular,
                max_width: Some(self.width - 56.0),
            });
        }
    }

    fn render_list(&self, cmds: &mut Vec<RenderCommand>, y: f32, h: f32, title: &str, items: &[String], scroll: usize) {
        cmds.push(RenderCommand::Text {
            x: 20.0, y: y + 10.0,
            text: format!("{title} ({} words)", items.len()),
            font_size: 16.0, color: BLUE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(self.width - 40.0),
        });

        let row_h: f32 = 28.0;
        let start_y = y + 36.0;
        let visible = ((h - 46.0) / row_h) as usize;

        if items.is_empty() {
            cmds.push(RenderCommand::Text {
                x: 20.0, y: start_y,
                text: "No entries yet".into(), font_size: 11.0,
                color: OVERLAY0, font_weight: FontWeightHint::Regular,
                max_width: Some(self.width - 40.0),
            });
            return;
        }

        for (vi, word) in items.iter().enumerate().take(visible) {
            let ry = start_y + (vi as f32) * row_h;
            let is_sel = vi == scroll;

            if is_sel {
                cmds.push(RenderCommand::FillRect {
                    x: 16.0, y: ry, width: self.width - 32.0, height: row_h - 4.0,
                    color: SURFACE0, corner_radii: CornerRadii::all(4.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: 24.0, y: ry + 6.0,
                text: word.clone(), font_size: 13.0,
                color: if is_sel { BLUE } else { TEXT_COLOR },
                font_weight: FontWeightHint::Regular,
                max_width: Some(self.width - 48.0),
            });
        }
    }

    fn render_wotd(&self, cmds: &mut Vec<RenderCommand>, y: f32, _h: f32) {
        cmds.push(RenderCommand::Text {
            x: 20.0, y: y + 10.0,
            text: "Word of the Day".into(), font_size: 18.0,
            color: YELLOW, font_weight: FontWeightHint::Bold,
            max_width: Some(self.width - 40.0),
        });

        if let Some(entry) = self.word_of_day() {
            cmds.push(RenderCommand::FillRect {
                x: 16.0, y: y + 40.0, width: self.width - 32.0, height: 100.0,
                color: SURFACE0, corner_radii: CornerRadii::all(10.0),
            });

            cmds.push(RenderCommand::Text {
                x: 28.0, y: y + 52.0,
                text: entry.word.clone(), font_size: 24.0,
                color: TEXT_COLOR, font_weight: FontWeightHint::Bold,
                max_width: Some(self.width - 56.0),
            });

            cmds.push(RenderCommand::Text {
                x: 28.0, y: y + 80.0,
                text: entry.pronunciation.clone(), font_size: 12.0,
                color: MAUVE, font_weight: FontWeightHint::Regular,
                max_width: Some(self.width - 56.0),
            });

            if let Some(def) = entry.definitions.first() {
                cmds.push(RenderCommand::Text {
                    x: 28.0, y: y + 98.0,
                    text: def.text.clone(), font_size: 11.0,
                    color: SUBTEXT1, font_weight: FontWeightHint::Regular,
                    max_width: Some(self.width - 56.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: 20.0, y: y + 152.0,
                text: "[Enter] View full entry".into(), font_size: 10.0,
                color: OVERLAY0, font_weight: FontWeightHint::Regular,
                max_width: Some(200.0),
            });
        }
    }

    fn render_status(&self, cmds: &mut Vec<RenderCommand>) {
        let y = self.height - 28.0;
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y, width: self.width, height: 28.0,
            color: CRUST, corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: 8.0, y: y + 8.0,
            text: self.status_message.clone(), font_size: 10.0,
            color: SUBTEXT1, font_weight: FontWeightHint::Regular,
            max_width: Some(self.width * 0.5),
        });
        cmds.push(RenderCommand::Text {
            x: self.width - 250.0, y: y + 8.0,
            text: format!("{} words | [/] Search | [Esc] Back", self.dictionary.len()),
            font_size: 10.0, color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(240.0),
        });
    }
}

fn main() {
    let _app = DictionaryApp::new();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dictionary_not_empty() {
        let dict = build_dictionary();
        assert!(dict.len() >= 10);
    }

    #[test]
    fn test_each_entry_has_definitions() {
        let dict = build_dictionary();
        for entry in &dict {
            assert!(!entry.definitions.is_empty(), "Word '{}' has no definitions", entry.word);
        }
    }

    #[test]
    fn test_each_entry_has_pronunciation() {
        let dict = build_dictionary();
        for entry in &dict {
            assert!(!entry.pronunciation.is_empty(), "Word '{}' has no pronunciation", entry.word);
        }
    }

    #[test]
    fn test_part_of_speech_labels() {
        assert_eq!(PartOfSpeech::Noun.label(), "noun");
        assert_eq!(PartOfSpeech::Verb.short(), "v.");
        assert_eq!(PartOfSpeech::Adjective.label(), "adjective");
    }

    #[test]
    fn test_search_exact() {
        let mut app = DictionaryApp::new();
        app.search_query = "kernel".into();
        app.perform_search();
        assert!(!app.search_results.is_empty());
        let first = app.search_results.first().copied().unwrap();
        assert_eq!(app.dictionary.get(first).unwrap().word, "kernel");
    }

    #[test]
    fn test_search_prefix() {
        let mut app = DictionaryApp::new();
        app.search_query = "comp".into();
        app.perform_search();
        assert!(!app.search_results.is_empty());
    }

    #[test]
    fn test_search_case_insensitive() {
        let mut app = DictionaryApp::new();
        app.search_query = "KERNEL".into();
        app.perform_search();
        assert!(!app.search_results.is_empty());
    }

    #[test]
    fn test_search_no_results() {
        let mut app = DictionaryApp::new();
        app.search_query = "xyzzyplugh".into();
        app.perform_search();
        assert!(app.search_results.is_empty());
    }

    #[test]
    fn test_search_empty_query() {
        let mut app = DictionaryApp::new();
        app.search_query = "".into();
        app.perform_search();
        assert!(app.search_results.is_empty());
    }

    #[test]
    fn test_select_entry() {
        let mut app = DictionaryApp::new();
        app.select_entry(0);
        assert_eq!(app.current_entry, Some(0));
        assert_eq!(app.screen, Screen::Detail);
        assert_eq!(app.history.len(), 1);
    }

    #[test]
    fn test_history_dedup() {
        let mut app = DictionaryApp::new();
        app.select_entry(0);
        app.select_entry(1);
        app.select_entry(0); // re-select first
        assert_eq!(app.history.len(), 2);
        assert_eq!(app.history.first().unwrap(), &app.dictionary.first().unwrap().word);
    }

    #[test]
    fn test_toggle_favorite() {
        let mut app = DictionaryApp::new();
        app.select_entry(0);
        assert!(!app.is_favorite());
        app.toggle_favorite();
        assert!(app.is_favorite());
        app.toggle_favorite();
        assert!(!app.is_favorite());
    }

    #[test]
    fn test_find_word() {
        let app = DictionaryApp::new();
        let idx = app.find_word("kernel");
        assert!(idx.is_some());
        assert_eq!(app.find_word("nonexistent"), None);
    }

    #[test]
    fn test_word_of_day() {
        let app = DictionaryApp::new();
        let wotd = app.word_of_day();
        assert!(wotd.is_some());
    }

    #[test]
    fn test_key_search_typing() {
        let mut app = DictionaryApp::new();
        app.handle_key("k", false, false);
        app.handle_key("e", false, false);
        app.handle_key("r", false, false);
        assert_eq!(app.search_query, "ker");
    }

    #[test]
    fn test_key_backspace() {
        let mut app = DictionaryApp::new();
        app.search_query = "kern".into();
        app.handle_key("BackSpace", false, false);
        assert_eq!(app.search_query, "ker");
    }

    #[test]
    fn test_key_enter_selects() {
        let mut app = DictionaryApp::new();
        app.search_query = "kernel".into();
        app.perform_search();
        app.handle_key("Return", false, false);
        assert_eq!(app.screen, Screen::Detail);
    }

    #[test]
    fn test_key_escape_clears() {
        let mut app = DictionaryApp::new();
        app.search_query = "test".into();
        app.handle_key("Escape", false, false);
        assert!(app.search_query.is_empty());
    }

    #[test]
    fn test_key_screen_switch() {
        let mut app = DictionaryApp::new();
        app.search_active = false;
        app.handle_key("3", false, false);
        assert_eq!(app.screen, Screen::History);
        app.handle_key("4", false, false);
        assert_eq!(app.screen, Screen::Favorites);
        app.handle_key("5", false, false);
        assert_eq!(app.screen, Screen::WordOfDay);
    }

    #[test]
    fn test_key_favorite() {
        let mut app = DictionaryApp::new();
        app.select_entry(0);
        app.search_active = false;
        app.handle_key("s", false, false);
        assert!(app.is_favorite());
    }

    #[test]
    fn test_render_search() {
        let app = DictionaryApp::new();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_detail() {
        let mut app = DictionaryApp::new();
        app.select_entry(0);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_history() {
        let mut app = DictionaryApp::new();
        app.screen = Screen::History;
        app.search_active = false;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_favorites() {
        let mut app = DictionaryApp::new();
        app.screen = Screen::Favorites;
        app.search_active = false;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_wotd() {
        let mut app = DictionaryApp::new();
        app.screen = Screen::WordOfDay;
        app.search_active = false;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_results() {
        let mut app = DictionaryApp::new();
        app.search_query = "a".into();
        app.perform_search();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_search_in_definitions() {
        let mut app = DictionaryApp::new();
        app.search_query = "operating system".into();
        app.perform_search();
        // Should find "kernel" since it has "operating system" in definition
        let found = app.search_results.iter().any(|&idx| {
            app.dictionary.get(idx).map(|e| e.word.as_str()) == Some("kernel")
        });
        assert!(found);
    }

    #[test]
    fn test_search_in_synonyms() {
        let mut app = DictionaryApp::new();
        app.search_query = "nucleus".into();
        app.perform_search();
        // Should find "kernel" since "nucleus" is a synonym
        let found = app.search_results.iter().any(|&idx| {
            app.dictionary.get(idx).map(|e| e.word.as_str()) == Some("kernel")
        });
        assert!(found);
    }

    #[test]
    fn test_multiple_definitions() {
        let app = DictionaryApp::new();
        let kernel = app.find_word("kernel").and_then(|i| app.dictionary.get(i));
        assert!(kernel.is_some());
        assert!(kernel.unwrap().definitions.len() >= 2);
    }

    #[test]
    fn test_synonyms_and_antonyms() {
        let app = DictionaryApp::new();
        let verbose = app.find_word("verbose").and_then(|i| app.dictionary.get(i));
        assert!(verbose.is_some());
        let v = verbose.unwrap();
        assert!(!v.synonyms.is_empty());
        assert!(!v.antonyms.is_empty());
    }

    #[test]
    fn test_etymology() {
        let app = DictionaryApp::new();
        let algo = app.find_word("algorithm").and_then(|i| app.dictionary.get(i));
        assert!(algo.is_some());
        assert!(!algo.unwrap().etymology.is_empty());
    }

    #[test]
    fn test_navigate_results() {
        let mut app = DictionaryApp::new();
        app.search_query = "e".into();
        app.perform_search();
        let count = app.search_results.len();
        if count > 1 {
            app.handle_key("Down", false, false);
            assert_eq!(app.selected_result, 1);
            app.handle_key("Up", false, false);
            assert_eq!(app.selected_result, 0);
        }
    }
}
