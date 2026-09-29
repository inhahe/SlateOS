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

use std::collections::HashMap;
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
    /// The locals query: where its names are declared and where they are
    /// used, so that a use is coloured as its declaration is. Empty for
    /// none.
    locals: &'static str,
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
const VISIBLE: usize = 20;

/// Every language: the ones a person chooses between by name, then the
/// hidden ones.
static LANGUAGES: [Language; 23] = [
    Language {
        name: "Ada",
        extensions: &["ads", "adb", "ada"],
        file_names: &[],
        interpreters: &[],
        aliases: &[],
        grammar: grammars::ada::generated::language_fn,
        highlights: grammars::ada::HIGHLIGHTS,
        injections: "",
        locals: "",
        index: 0,
    },
    Language {
        name: "Bash",
        extensions: &["sh", "bash", "ksh", "zsh"],
        // A shell's own start-up files, and the build scripts written in it.
        file_names: &[
            ".bashrc",
            ".bash_profile",
            ".bash_login",
            ".bash_logout",
            ".bash_aliases",
            ".profile",
            ".zshrc",
            ".zprofile",
            ".zshenv",
            "PKGBUILD",
            "APKBUILD",
        ],
        interpreters: &["sh", "bash", "dash", "ash", "ksh", "mksh", "zsh"],
        aliases: &["sh", "shell", "shellscript", "ksh", "zsh"],
        grammar: grammars::bash::generated::language_fn,
        highlights: grammars::bash::HIGHLIGHTS,
        injections: "",
        locals: "",
        index: 1,
    },
    Language {
        name: "C",
        extensions: &["c", "h"],
        file_names: &[],
        interpreters: &[],
        aliases: &["h"],
        grammar: grammars::c::generated::language_fn,
        highlights: grammars::c::HIGHLIGHTS,
        injections: "",
        locals: "",
        index: 2,
    },
    Language {
        name: "C++",
        // `.h` stays C's: a header either might have, and C's grammar reads
        // most of a C++ one.
        extensions: &[
            "cc", "cpp", "cxx", "c++", "hh", "hpp", "hxx", "h++", "ipp", "tpp",
        ],
        file_names: &[],
        interpreters: &[],
        aliases: &["cpp", "cc", "cxx", "hpp"],
        grammar: grammars::cpp::generated::language_fn,
        highlights: grammars::cpp::HIGHLIGHTS,
        injections: grammars::cpp::INJECTIONS,
        locals: "",
        index: 3,
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
        locals: "",
        index: 4,
    },
    Language {
        name: "Diff",
        extensions: &["diff", "patch"],
        file_names: &[],
        interpreters: &[],
        aliases: &["patch", "udiff"],
        grammar: grammars::diff::generated::language_fn,
        highlights: grammars::diff::HIGHLIGHTS,
        injections: grammars::diff::INJECTIONS,
        locals: "",
        index: 5,
    },
    Language {
        name: "Go",
        extensions: &["go"],
        file_names: &[],
        interpreters: &[],
        aliases: &["golang"],
        grammar: grammars::go::generated::language_fn,
        highlights: grammars::go::HIGHLIGHTS,
        injections: "",
        locals: "",
        index: 6,
    },
    Language {
        name: "HTML",
        extensions: &["html", "htm", "xhtml", "shtml"],
        file_names: &[],
        interpreters: &[],
        aliases: &["htm", "xhtml"],
        grammar: grammars::html::generated::language_fn,
        highlights: grammars::html::HIGHLIGHTS,
        injections: grammars::html::INJECTIONS,
        locals: "",
        index: 7,
    },
    Language {
        name: "INI",
        extensions: &[
            "ini",
            "cfg",
            "desktop",
            "directory",
            "service",
            "socket",
            "timer",
            "mount",
            "automount",
            "slice",
            "target",
        ],
        file_names: &[".gitconfig", ".editorconfig", ".gitmodules"],
        interpreters: &[],
        aliases: &["dosini", "desktop", "gitconfig", "systemd"],
        grammar: grammars::ini::generated::language_fn,
        highlights: grammars::ini::HIGHLIGHTS,
        injections: "",
        locals: "",
        index: 8,
    },
    Language {
        name: "Java",
        extensions: &["java"],
        file_names: &[],
        interpreters: &[],
        aliases: &[],
        grammar: grammars::java::generated::language_fn,
        highlights: grammars::java::HIGHLIGHTS,
        injections: "",
        locals: "",
        index: 9,
    },
    Language {
        name: "JavaScript",
        extensions: &["js", "mjs", "cjs", "jsx"],
        file_names: &[],
        interpreters: &["node", "nodejs"],
        aliases: &["js", "jsx", "mjs", "cjs", "node"],
        grammar: grammars::javascript::generated::language_fn,
        highlights: grammars::javascript::HIGHLIGHTS,
        injections: grammars::javascript::INJECTIONS,
        locals: grammars::javascript::LOCALS,
        index: 10,
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
        locals: "",
        index: 11,
    },
    Language {
        name: "Make",
        extensions: &["mk", "mak", "make"],
        file_names: &["Makefile", "makefile", "GNUmakefile", "MAKEFILE"],
        interpreters: &[],
        aliases: &["makefile", "mk"],
        grammar: grammars::make::generated::language_fn,
        highlights: grammars::make::HIGHLIGHTS,
        injections: "",
        locals: "",
        index: 12,
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
        locals: "",
        index: 13,
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
        locals: "",
        index: 14,
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
        locals: "",
        index: 15,
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
        locals: "",
        index: 16,
    },
    Language {
        name: "TSX",
        extensions: &["tsx"],
        file_names: &[],
        interpreters: &[],
        aliases: &[],
        grammar: grammars::tsx::generated::language_fn,
        highlights: grammars::tsx::HIGHLIGHTS,
        injections: grammars::javascript::INJECTIONS,
        locals: grammars::tsx::LOCALS,
        index: 17,
    },
    Language {
        name: "TypeScript",
        extensions: &["ts", "mts", "cts"],
        file_names: &[],
        interpreters: &["ts-node"],
        aliases: &["ts"],
        grammar: grammars::typescript::generated::language_fn,
        highlights: grammars::typescript::HIGHLIGHTS,
        injections: grammars::javascript::INJECTIONS,
        locals: grammars::typescript::LOCALS,
        index: 18,
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
        locals: "",
        index: 19,
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
        locals: "",
        index: 20,
    },
    // Hidden: injected by JavaScript and TypeScript into their comments.
    Language {
        name: "jsdoc",
        extensions: &[],
        file_names: &[],
        interpreters: &[],
        aliases: &[],
        grammar: grammars::jsdoc::generated::language_fn,
        highlights: grammars::jsdoc::HIGHLIGHTS,
        injections: "",
        locals: "",
        index: 21,
    },
    // Hidden: injected by JavaScript and TypeScript into their regular
    // expressions.
    Language {
        name: "regex",
        extensions: &[],
        file_names: &[],
        interpreters: &[],
        aliases: &[],
        grammar: grammars::regex::generated::language_fn,
        highlights: grammars::regex::HIGHLIGHTS,
        injections: "",
        locals: "",
        index: 22,
    },
];

/// A capture's priority where its pattern sets none: Neovim's, whose
/// directive `(#set! priority N)` is.
pub(crate) const DEFAULT_PRIORITY: u16 = 100;

/// A language's queries, compiled: the highlight query with what each of
/// its captures paints, and the injection and locals queries with the
/// captures they are read by.
pub(crate) struct Compiled {
    pub(crate) highlights: tree_sitter::Query,
    /// What each highlight capture paints, by capture index.
    pub(crate) paints: Vec<Paint>,
    /// Whether each highlight pattern leaves alone a name found declared --
    /// `(#is-not? local)` -- by pattern index.
    pub(crate) non_local: Vec<bool>,
    /// The priorities the highlight patterns set -- `(#set! priority 95)`,
    /// or `(#set! @capture priority 95)` for one capture -- by pattern index
    /// and capture index (none: every capture of the pattern). Most queries
    /// set none.
    pub(crate) priorities: HashMap<(usize, Option<u32>), u16>,
    /// How deep the highlight query's patterns go: the most node patterns
    /// any one nests inside another ([`pattern_depth`]).
    pub(crate) depth: usize,
    pub(crate) injections: Option<Injections>,
    pub(crate) locals: Option<LocalsQuery>,
}

impl Compiled {
    /// `highlights`, compiled from `source`, read for what each capture
    /// paints, which patterns leave locals alone, the priorities they set
    /// and how deep they go; with the injection and locals queries.
    pub(crate) fn new(
        highlights: tree_sitter::Query,
        source: &str,
        injections: Option<Injections>,
        locals: Option<LocalsQuery>,
    ) -> Self {
        let paints = highlights
            .capture_names()
            .iter()
            .map(|name| Paint::for_capture(name))
            .collect();
        let non_local = (0..highlights.pattern_count())
            .map(|i| {
                highlights
                    .property_predicates(i)
                    .iter()
                    .any(|(p, positive)| !*positive && &*p.key == "local")
            })
            .collect();
        let mut priorities = HashMap::new();
        for pattern in 0..highlights.pattern_count() {
            for setting in highlights.property_settings(pattern) {
                // Neovim reads the number as Lua's `tonumber` does; a
                // priority that is not one is no priority.
                let value = setting.value.as_deref().and_then(|v| v.parse::<u16>().ok());
                if let (true, Some(value)) = (&*setting.key == "priority", value) {
                    let capture = setting.capture_id.and_then(|c| u32::try_from(c).ok());
                    priorities.insert((pattern, capture), value);
                }
            }
        }
        Self {
            depth: pattern_depth(source),
            highlights,
            paints,
            non_local,
            priorities,
            injections,
            locals,
        }
    }

    /// The priority of capture `capture` of highlight pattern `pattern`:
    /// the capture's own, else its pattern's, else [`DEFAULT_PRIORITY`].
    pub(crate) fn priority(&self, pattern: usize, capture: u32) -> u16 {
        if self.priorities.is_empty() {
            return DEFAULT_PRIORITY;
        }
        self.priorities
            .get(&(pattern, Some(capture)))
            .or_else(|| self.priorities.get(&(pattern, None)))
            .copied()
            .unwrap_or(DEFAULT_PRIORITY)
    }

    /// What highlight capture `index` paints.
    pub(crate) fn paint(&self, index: u32) -> Paint {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.paints.get(i))
            .copied()
            .unwrap_or(Paint::Skip)
    }

    /// Whether highlight pattern `pattern` leaves a local alone.
    pub(crate) fn is_non_local(&self, pattern: usize) -> bool {
        self.non_local.get(pattern).copied().unwrap_or(false)
    }
}

/// How deep `query`'s patterns go: the most node patterns -- `(kind ...)`
/// -- any one of them nests inside another, 0 for a query of lone nodes. A
/// pattern matches a node's children, never deeper, so every node a
/// pattern captures is at most this many levels below the node it matches
/// from -- or one more, for a pattern of siblings with no parent named.
/// Groups, alternations, predicates, fields, anchors, quantifiers and
/// captures nest nothing; strings and comments are passed over.
pub(crate) fn pattern_depth(query: &str) -> usize {
    // Each open bracket: whether it is a node pattern's.
    let mut open: Vec<bool> = Vec::new();
    let mut deepest = 0;
    let mut chars = query.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ';' => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '"' => {
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => {
                            chars.next();
                        }
                        '"' => break,
                        _ => {}
                    }
                }
            }
            '(' => {
                while chars.next_if(|c| c.is_whitespace()).is_some() {}
                let node = chars
                    .peek()
                    .is_some_and(|&c| c.is_alphanumeric() || c == '_');
                open.push(node);
                if node {
                    let nodes = open.iter().filter(|&&n| n).count();
                    deepest = deepest.max(nodes.saturating_sub(1));
                }
            }
            '[' => open.push(false),
            ')' | ']' => {
                open.pop();
            }
            _ => {}
        }
    }
    deepest
}

/// A locals query, and which of its captures say what: a scope, a name's
/// declaration, the value a declaration gives it (a use inside which is
/// not of it), and a name's use.
pub(crate) struct LocalsQuery {
    pub(crate) query: tree_sitter::Query,
    pub(crate) scope: Option<u32>,
    pub(crate) definition: Option<u32>,
    pub(crate) definition_value: Option<u32>,
    pub(crate) reference: Option<u32>,
}

/// An injection query, and which of its captures are the injected text,
/// the name of its language, and the name of a file whose language it is.
pub(crate) struct Injections {
    pub(crate) query: tree_sitter::Query,
    pub(crate) content: Option<u32>,
    pub(crate) language: Option<u32>,
    pub(crate) filename: Option<u32>,
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
static COMPILED: [OnceLock<Result<Compiled, Error>>; 23] = [const { OnceLock::new() }; 23];

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
            let injections = if self.injections.is_empty() {
                None
            } else {
                let query = self.compile(self.injections, "the injection query")?;
                Some(Injections {
                    content: query.capture_index_for_name("injection.content"),
                    language: query.capture_index_for_name("injection.language"),
                    filename: query.capture_index_for_name("injection.filename"),
                    query,
                })
            };
            let locals = if self.locals.is_empty() {
                None
            } else {
                let query = self.compile(self.locals, "the locals query")?;
                Some(LocalsQuery {
                    scope: query.capture_index_for_name("local.scope"),
                    definition: query.capture_index_for_name("local.definition"),
                    definition_value: query.capture_index_for_name("local.definition-value"),
                    reference: query.capture_index_for_name("local.reference"),
                    query,
                })
            };
            Ok(Compiled::new(
                highlights,
                self.highlights,
                injections,
                locals,
            ))
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
            ("Ada", None, 14, 459, 50),
            ("Bash", Some("bash"), 15, 280, 19),
            ("C", Some("c"), 15, 363, 39),
            ("C++", None, 14, 543, 50),
            ("CSS", Some("css"), 15, 151, 0),
            ("Diff", Some("diff"), 15, 86, 4),
            ("Go", Some("go"), 15, 219, 35),
            ("INI", Some("ini"), 15, 19, 1),
            ("Make", None, 14, 182, 24),
            ("Java", None, 14, 321, 40),
            ("HTML", None, 14, 41, 0),
            ("JavaScript", Some("javascript"), 15, 265, 36),
            ("TOML", None, 14, 66, 0),
            ("TSX", None, 14, 400, 43),
            ("TypeScript", None, 14, 383, 40),
            ("YAML", None, 14, 301, 2),
            ("JSON", None, 14, 25, 2),
            ("Markdown", Some("markdown"), 15, 207, 1),
            ("markdown_inline", Some("markdown_inline"), 15, 153, 0),
            ("jsdoc", Some("jsdoc"), 15, 43, 1),
            ("regex", Some("regex"), 15, 78, 0),
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
            assert!(
                c.paints.iter().any(|p| matches!(p, Paint::Kind(_))),
                "{}: a highlight query that paints nothing",
                l.name
            );
            assert_eq!(c.paints.len(), c.highlights.capture_names().len());
            if let Some(i) = &c.injections {
                assert!(
                    i.content.is_some(),
                    "{}: an injection query with no content",
                    l.name
                );
            }
            assert_eq!(c.locals.is_some(), !l.locals.is_empty(), "{}", l.name);
            if let Some(q) = &c.locals {
                assert!(
                    q.scope.is_some() && q.definition.is_some() && q.reference.is_some(),
                    "{}: a locals query without scopes, declarations and uses",
                    l.name
                );
            }
        }
        // JavaScript's builtins are left alone where the name is declared.
        let js = Language::named("javascript").unwrap().compiled().unwrap();
        assert!(js.non_local.iter().filter(|&&n| n).count() >= 2);
        let rust = Language::named("rust").unwrap().compiled().unwrap();
        assert!(rust.non_local.iter().all(|&n| !n));
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
        assert_eq!(found("drivers/ahci.adb"), Some("Ada"));
        assert_eq!(found("AHCI.ADS"), Some("Ada"));
        assert_eq!(found("main.cpp"), Some("C++"));
        assert_eq!(found("vector.HPP"), Some("C++"));
        assert_eq!(found("a.c++"), Some("C++"));
        assert_eq!(found("site.css"), Some("CSS"));
        assert_eq!(found("cmd/main.go"), Some("Go"));
        assert_eq!(found("fix.patch"), Some("Diff"));
        assert_eq!(found("files.desktop"), Some("INI"));
        assert_eq!(found("/home/me/.gitconfig"), Some("INI"));
        assert_eq!(found("Makefile"), Some("Make"));
        assert_eq!(found("rules.mk"), Some("Make"));
        assert_eq!(found("Main.java"), Some("Java"));
        assert_eq!(found("app.js"), Some("JavaScript"));
        assert_eq!(found("index.html"), Some("HTML"));
        assert_eq!(found("main.ts"), Some("TypeScript"));
        assert_eq!(found("vite.config.MTS"), Some("TypeScript"));
        assert_eq!(found("App.tsx"), Some("TSX"));
        assert_eq!(found("OLD.HTM"), Some("HTML"));
        assert_eq!(found("rollup.config.MJS"), Some("JavaScript"));
        assert_eq!(found("Button.jsx"), Some("JavaScript"));
        assert_eq!(found("appearance.yaml"), Some("YAML"));
        assert_eq!(found("README.md"), Some("Markdown"));
        assert_eq!(found("deploy.sh"), Some("Bash"));
        assert_eq!(found("/home/me/.bashrc"), Some("Bash"));
        assert_eq!(found("PKGBUILD"), Some("Bash"));
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
        assert_eq!(found("#!/bin/sh"), Some("Bash"));
        assert_eq!(found("#!/usr/bin/env bash"), Some("Bash"));
        assert_eq!(found("#!/bin/zsh -f"), Some("Bash"));
        assert_eq!(found("#!/usr/bin/env node"), Some("JavaScript"));
        assert_eq!(found("#!/usr/bin/env ts-node"), Some("TypeScript"));
        assert_eq!(found("#!/usr/bin/perl"), None);
        assert_eq!(found("import os"), None);
        assert_eq!(found("#!"), None);
    }

    /// **A query's depth is how far its node patterns nest**: groups,
    /// alternations, predicates, fields and captures nest nothing, and
    /// brackets inside strings and comments are not brackets.
    #[test]
    fn a_querys_depth_is_how_far_its_nodes_nest() {
        for (query, depth) in [
            ("", 0),
            ("(identifier) @variable", 0),
            ("((identifier) @constant (#match? @constant \"^[A-Z]\"))", 0),
            ("[(true) (false)] @constant", 0),
            ("(call_expression function: (identifier) @function)", 1),
            (
                "(call_expression function: (member_expression property: (property_identifier) @m))",
                2,
            ),
            ("(a (b (c (d))))", 3),
            ("( a ( b ))", 1),
            ("(_ (_) @x)", 1),
            ("(a [(b (c)) (d)])", 2),
            ("(a \"(\" @p (b)) ; (x (y (z (w))))", 1),
            ("(a \"\\\"(\" (b))", 1),
            (
                "(formal_parameters (object_pattern (pair_pattern value: (identifier) @p)))",
                3,
            ),
        ] {
            assert_eq!(pattern_depth(query), depth, "{query}");
        }
        let js = Language::named("javascript").unwrap().compiled().unwrap();
        assert_eq!(js.depth, 3);
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
