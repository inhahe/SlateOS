//! Syntax highlighting for the code editor (`guitk::codeview::CodeView`):
//! tree-sitter's grammars and runtime, as pure Rust, and a
//! [`guitk::highlight::Highlighter`] that drives them.
//!
//! # What it is made of
//!
//! - **The runtime** is tree-sitter's, transpiled from C to Rust
//!   (`tree-sitter-c2rust`, design-decisions §1437): the incremental parser,
//!   its trees, and its query engine.
//! - **The grammars** are tree-sitter's own grammars, as their authors publish
//!   them (`grammars/<name>/`): each one's generated `parser.c` is converted to
//!   Rust at build time (`build.rs`, `gui/tsgrammar`), and each one's external
//!   scanner -- the part written by hand in C -- is ported by hand
//!   (`src/grammars/`). Each grammar's own test corpus runs against it here
//!   (`src/corpus.rs`), which is what says a converted grammar parses as the C
//!   one does.
//! - **The queries** are the grammars' own `highlights.scm`, whose capture
//!   names (`@keyword`, `@function.method`) map onto the toolkit's kinds of
//!   code ([`guitk::highlight::Highlight::for_capture`]), which a theme
//!   colours. The grammars' own highlight tests run against them here
//!   (`src/highlight_tests.rs`), which is what says the highlighter reads a
//!   query as tree-sitter's does.
//!
//! # Using it
//!
//! ```ignore
//! let language = syntax::Language::for_file(path).or_else(|| syntax::Language::for_first_line(&first));
//! if let Some(language) = language {
//!     view.set_highlighter(Some(Box::new(language.highlighter()?)));
//! }
//! ```
//!
//! The highlighter re-parses only what each edit touched, a few milliseconds
//! at a time ([`SyntaxHighlighter`]), and answers for what is on screen.

mod ffi;
mod grammars;
mod highlighter;

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a corpus that cannot be read is a failure to report loudly"
)]
mod corpus;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::arithmetic_side_effects,
    reason = "a test: a file that cannot be read is a failure to report loudly"
)]
mod highlight_tests;

use std::path::Path;
use std::sync::OnceLock;

pub use highlighter::SyntaxHighlighter;

/// Why a language cannot be highlighted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The runtime refused the grammar -- an ABI version it does not read.
    Grammar {
        /// The language.
        language: &'static str,
        /// What the runtime said.
        message: String,
    },
    /// The grammar's highlight query does not compile against it.
    Query {
        /// The language.
        language: &'static str,
        /// What the query compiler said, with where.
        message: String,
    },
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Grammar { language, message } => {
                write!(f, "the {language} grammar cannot be used: {message}")
            }
            Self::Query { language, message } => {
                write!(
                    f,
                    "the {language} highlight query does not compile: {message}"
                )
            }
        }
    }
}

impl std::error::Error for Error {}

/// A language the highlighter knows: its grammar, its queries, and how its
/// files are recognised.
pub struct Language {
    name: &'static str,
    /// File name extensions, without the dot, in lower case.
    extensions: &'static [&'static str],
    /// Whole file names, for files with no extension that says.
    file_names: &'static [&'static str],
    /// The interpreters a `#!` line may name.
    interpreters: &'static [&'static str],
    /// What another language's text calls it, in lower case: a Markdown
    /// code fence's info string (```` ```rs ````), an injection query's
    /// `injection.language`. Its name, lowered, always counts too.
    aliases: &'static [&'static str],
    grammar: fn() -> tree_sitter_language::LanguageFn,
    highlights: &'static str,
    /// The injection query: the stretches of its text written in another
    /// language. Empty for none.
    injections: &'static str,
    /// Where in [`LANGUAGES`] it is: its compiled queries' slot.
    index: usize,
}

impl core::fmt::Debug for Language {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Language")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl PartialEq for Language {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}

impl Eq for Language {}

/// How many of [`LANGUAGES`] a person chooses between; the rest are parts
/// of another language that only it injects (Markdown's inline grammar).
const VISIBLE: usize = 8;

/// Every language: the ones a person chooses between by name, then the
/// hidden ones.
static LANGUAGES: [Language; 9] = [
    Language {
        name: "C",
        extensions: &["c", "h"],
        file_names: &[],
        interpreters: &[],
        aliases: &["h"],
        grammar: grammars::c::generated::language_fn,
        highlights: grammars::c::HIGHLIGHTS,
        injections: "",
        index: 0,
    },
    Language {
        name: "CSS",
        extensions: &["css"],
        file_names: &[],
        interpreters: &[],
        aliases: &[],
        grammar: grammars::css::generated::language_fn,
        highlights: grammars::css::HIGHLIGHTS,
        injections: "",
        index: 1,
    },
    Language {
        name: "JSON",
        extensions: &["json", "jsonc", "jsonl", "geojson", "webmanifest"],
        file_names: &[".babelrc", ".eslintrc", ".prettierrc"],
        interpreters: &[],
        aliases: &["jsonc", "json5", "jsonl"],
        grammar: grammars::json::generated::language_fn,
        highlights: grammars::json::HIGHLIGHTS,
        injections: "",
        index: 2,
    },
    Language {
        name: "Markdown",
        extensions: &["md", "markdown", "mdown", "mkd", "mkdn"],
        file_names: &[],
        interpreters: &[],
        aliases: &["md"],
        grammar: grammars::markdown::generated::language_fn,
        highlights: grammars::markdown::HIGHLIGHTS,
        injections: grammars::markdown::INJECTIONS,
        index: 3,
    },
    Language {
        name: "Python",
        extensions: &["py", "pyw", "pyi"],
        file_names: &["SConstruct", "SConscript"],
        interpreters: &["python", "python2", "python3", "pypy", "pypy3"],
        aliases: &["py", "python3", "py3"],
        grammar: grammars::python::generated::language_fn,
        highlights: grammars::python::HIGHLIGHTS,
        injections: "",
        index: 4,
    },
    Language {
        name: "Rust",
        extensions: &["rs"],
        file_names: &[],
        interpreters: &[],
        aliases: &["rs"],
        grammar: grammars::rust::generated::language_fn,
        highlights: grammars::rust::HIGHLIGHTS,
        injections: grammars::rust::INJECTIONS,
        index: 5,
    },
    Language {
        name: "TOML",
        extensions: &["toml"],
        // TOML under names of their own: Cargo's lock file, pipenv's.
        file_names: &["Cargo.lock", "Pipfile"],
        interpreters: &[],
        aliases: &[],
        grammar: grammars::toml::generated::language_fn,
        highlights: grammars::toml::HIGHLIGHTS,
        injections: "",
        index: 6,
    },
    Language {
        name: "YAML",
        extensions: &["yaml", "yml"],
        file_names: &[],
        interpreters: &[],
        aliases: &["yml"],
        grammar: grammars::yaml::generated::language_fn,
        highlights: grammars::yaml::HIGHLIGHTS,
        injections: "",
        index: 7,
    },
    // Hidden: injected by Markdown into its paragraphs and headings.
    Language {
        name: "markdown_inline",
        extensions: &[],
        file_names: &[],
        interpreters: &[],
        aliases: &[],
        grammar: grammars::markdown_inline::generated::language_fn,
        highlights: grammars::markdown_inline::HIGHLIGHTS,
        injections: grammars::markdown_inline::INJECTIONS,
        index: 8,
    },
];

/// A language's queries, compiled: the highlight query with what each of
/// its captures paints, and the injection query with the captures it is
/// read by.
pub(crate) struct Compiled {
    pub(crate) highlights: tree_sitter::Query,
    /// What each highlight capture paints, by capture index.
    pub(crate) paints: Vec<Paint>,
    pub(crate) injections: Option<Injections>,
}

/// An injection query, and which of its captures are the injected text and
/// the name of its language.
pub(crate) struct Injections {
    pub(crate) query: tree_sitter::Query,
    pub(crate) content: Option<u32>,
    pub(crate) language: Option<u32>,
}

/// What a highlight capture does to the text it covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Paint {
    /// Colours it as this kind of code.
    Kind(guitk::highlight::Highlight),
    /// Paints it in the text's own ink: `@none`, which a grammar puts on a
    /// stretch it leaves to an injected language (Markdown's code-fence
    /// contents), so the enclosing capture's colour does not show through.
    Plain,
    /// Nothing: a capture name no kind answers to.
    Skip,
}

impl Paint {
    fn for_capture(name: &str) -> Self {
        if name.strip_prefix('@').unwrap_or(name) == "none" {
            Self::Plain
        } else {
            guitk::highlight::Highlight::for_capture(name).map_or(Self::Skip, Self::Kind)
        }
    }
}

/// Each language's compiled queries, made the first time they are asked for.
static COMPILED: [OnceLock<Result<Compiled, Error>>; 9] = [const { OnceLock::new() }; 9];

impl Language {
    /// Every language a person chooses between, by name.
    #[must_use]
    pub fn all() -> &'static [Self] {
        LANGUAGES.get(..VISIBLE).unwrap_or(&LANGUAGES)
    }

    /// The language called `name`, in any case.
    #[must_use]
    pub fn named(name: &str) -> Option<&'static Self> {
        Self::all()
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(name))
    }

    /// The language another language's text names: a code fence's info
    /// string, an injection query's `injection.language` -- by name or
    /// alias, in any case, hidden languages included.
    #[must_use]
    pub fn for_injection(name: &str) -> Option<&'static Self> {
        let name = name
            .trim()
            .trim_start_matches(['.', '{'])
            .trim_end_matches('}');
        LANGUAGES.iter().find(|l| {
            l.name.eq_ignore_ascii_case(name)
                || l.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
        })
    }

    /// The language of the file at `path`, by its extension or its whole
    /// name -- compared as bytes, since a file's name need not be text.
    #[must_use]
    pub fn for_file(path: &Path) -> Option<&'static Self> {
        let name = path.file_name()?.as_encoded_bytes();
        if let Some(language) = Self::all()
            .iter()
            .find(|l| l.file_names.iter().any(|f| f.as_bytes() == name))
        {
            return Some(language);
        }
        let dot = name.iter().rposition(|&b| b == b'.')?;
        // `.bashrc` is a name, not an extension.
        if dot == 0 {
            return None;
        }
        let extension = name.get(dot.saturating_add(1)..)?;
        Self::all().iter().find(|l| {
            l.extensions
                .iter()
                .any(|e| e.as_bytes().eq_ignore_ascii_case(extension))
        })
    }

    /// The language a script's `#!` line names: `#!/usr/bin/python3`,
    /// `#!/usr/bin/env python3`, `#!/usr/bin/env -S python3 -u`.
    #[must_use]
    pub fn for_first_line(line: &str) -> Option<&'static Self> {
        let rest = line.strip_prefix("#!")?;
        let mut words = rest.split_whitespace();
        let mut program = words.next()?.rsplit('/').next()?;
        if program == "env" {
            program = words.find(|w| !w.starts_with('-') && !w.contains('='))?;
        }
        // `python3.12` is `python3`.
        let program = program.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
        Self::all().iter().find(|l| {
            l.interpreters
                .iter()
                .any(|i| i.trim_end_matches(|c: char| c.is_ascii_digit()) == program)
        })
    }

    /// The language's name, as a person reads it: `Rust`, `JSON`.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// A highlighter for text in this language.
    ///
    /// # Errors
    ///
    /// When the runtime refuses the grammar or one of its queries does not
    /// compile -- which the tests check for every language, so neither
    /// happens in a build that passed them.
    pub fn highlighter(&'static self) -> Result<SyntaxHighlighter, Error> {
        SyntaxHighlighter::new(self)
    }

    /// The grammar, as the runtime takes it.
    fn ts_language(&self) -> tree_sitter::Language {
        tree_sitter::Language::new((self.grammar)())
    }

    /// A query compiled against the grammar, its error said in lines and
    /// columns.
    fn compile(&self, source: &str, which: &str) -> Result<tree_sitter::Query, Error> {
        tree_sitter::Query::new(&self.ts_language(), source).map_err(|e| Error::Query {
            language: self.name,
            message: format!(
                "{which}, line {}, column {}: {}",
                e.row.saturating_add(1),
                e.column.saturating_add(1),
                e.message
            ),
        })
    }

    /// The compiled queries.
    pub(crate) fn compiled(&self) -> Result<&'static Compiled, Error> {
        let slot = COMPILED.get(self.index).ok_or_else(|| Error::Query {
            language: self.name,
            message: "no slot for its queries".to_owned(),
        })?;
        slot.get_or_init(|| {
            let highlights = self.compile(self.highlights, "the highlight query")?;
            let paints = highlights
                .capture_names()
                .iter()
                .map(|name| Paint::for_capture(name))
                .collect();
            let injections = if self.injections.is_empty() {
                None
            } else {
                let query = self.compile(self.injections, "the injection query")?;
                Some(Injections {
                    content: query.capture_index_for_name("injection.content"),
                    language: query.capture_index_for_name("injection.language"),
                    query,
                })
            };
            Ok(Compiled {
                highlights,
                paints,
                injections,
            })
        })
        .as_ref()
        .map_err(Clone::clone)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
mod tests {
    use super::*;

    /// **Each language's index is its place**, so its query lands in its own
    /// slot.
    #[test]
    fn each_languages_index_is_its_place() {
        for (i, l) in LANGUAGES.iter().enumerate() {
            assert_eq!(l.index, i, "{}", l.name);
        }
        assert_eq!(COMPILED.len(), LANGUAGES.len());
        assert!(VISIBLE <= LANGUAGES.len());
    }

    /// **The runtime reads each grammar as it was written**: the counts and
    /// the fields at the far end of `TSLanguage` -- its name, its supertypes
    /// -- come back through the runtime as the grammar's `parser.c` states
    /// them. A mirror field out of place would scramble these first.
    #[test]
    fn the_runtime_reads_each_grammar_as_it_was_written() {
        for (language, name, abi, kinds, fields) in [
            ("C", Some("c"), 15, 363, 39),
            ("CSS", Some("css"), 15, 151, 0),
            ("TOML", None, 14, 66, 0),
            ("YAML", None, 14, 301, 2),
            ("JSON", None, 14, 25, 2),
            ("Markdown", Some("markdown"), 15, 207, 1),
            ("markdown_inline", Some("markdown_inline"), 15, 153, 0),
            ("Python", Some("python"), 15, 274, 32),
            ("Rust", Some("rust"), 15, 355, 31),
        ] {
            let l = Language::for_injection(language).unwrap().ts_language();
            assert_eq!(l.abi_version(), abi, "{language}");
            assert_eq!(l.name(), name, "{language}");
            assert_eq!(l.node_kind_count(), kinds, "{language}");
            assert_eq!(l.field_count(), fields, "{language}");
            let mut parser = tree_sitter::Parser::new();
            parser.set_language(&l).expect(language);
        }
        let rust = Language::named("rust").unwrap().ts_language();
        let supertypes: Vec<&str> = rust
            .supertypes()
            .iter()
            .map(|&id| rust.node_kind_for_id(id).unwrap_or("?"))
            .collect();
        assert!(supertypes.contains(&"_expression"), "{supertypes:?}");
        assert_eq!(
            rust.metadata().map(|m| (m.major_version, m.minor_version)),
            Some((0, 24))
        );
    }

    /// **Every language's highlight query compiles** against its grammar --
    /// the hidden ones', reached only by injection, too.
    #[test]
    fn every_highlight_query_compiles() {
        for l in &LANGUAGES {
            let c = l.compiled().unwrap_or_else(|e| panic!("{e}"));
            assert!(c.highlights.capture_names().len() > 3, "{}", l.name);
            assert_eq!(c.paints.len(), c.highlights.capture_names().len());
            if let Some(i) = &c.injections {
                assert!(
                    i.content.is_some(),
                    "{}: an injection query with no content",
                    l.name
                );
            }
        }
    }

    /// **A file is known by its extension or its name**, as bytes and in
    /// any case; a dot file is a name, not an extension.
    #[test]
    fn a_file_is_known_by_its_extension_or_its_name() {
        let found = |p: &str| Language::for_file(Path::new(p)).map(Language::name);
        assert_eq!(found("src/main.rs"), Some("Rust"));
        assert_eq!(found("A.PY"), Some("Python"));
        assert_eq!(found("/etc/x/config.JSON"), Some("JSON"));
        assert_eq!(found("SConstruct"), Some("Python"));
        assert_eq!(found("Cargo.lock"), Some("TOML"));
        assert_eq!(found("gui/syntax/Cargo.toml"), Some("TOML"));
        assert_eq!(found("stdio.h"), Some("C"));
        assert_eq!(found("site.css"), Some("CSS"));
        assert_eq!(found("appearance.yaml"), Some("YAML"));
        assert_eq!(found("README.md"), Some("Markdown"));
        assert_eq!(found(".github/ci.YML"), Some("YAML"));
        assert_eq!(found(".rs"), None);
        assert_eq!(found("notes.txt"), None);
        assert_eq!(found("rs"), None);
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let odd = std::ffi::OsStr::from_bytes(b"caf\xe9.rs");
            assert_eq!(
                Language::for_file(Path::new(odd)).map(Language::name),
                Some("Rust")
            );
        }
    }

    /// **A script is known by the interpreter its `#!` line names**, through
    /// `env` and its options, and with a version on the end.
    #[test]
    fn a_script_is_known_by_its_interpreter() {
        let found = |l: &str| Language::for_first_line(l).map(Language::name);
        assert_eq!(found("#!/usr/bin/python3"), Some("Python"));
        assert_eq!(found("#!/usr/bin/env python3.12"), Some("Python"));
        assert_eq!(
            found("#!/usr/bin/env -S PYTHONPATH=. python -u"),
            Some("Python")
        );
        assert_eq!(found("#! /usr/local/bin/pypy3"), Some("Python"));
        assert_eq!(found("#!/bin/sh"), None);
        assert_eq!(found("import os"), None);
        assert_eq!(found("#!"), None);
    }

    /// **Languages are found by name in any case, and are equal only to
    /// themselves.**
    #[test]
    fn languages_are_found_by_name() {
        assert_eq!(Language::named("json").map(Language::name), Some("JSON"));
        assert!(Language::named("cobol").is_none());
        assert_eq!(Language::named("Rust"), Language::named("RUST"));
        assert_ne!(Language::named("Rust"), Language::named("Python"));
    }
}
