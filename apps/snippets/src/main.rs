//! Snippets — a library of code fragments, in folders, under tags, in twelve
//! languages, syntax-highlighted, searchable, sortable, with favourites, a
//! recently-used list, template placeholders and a JSON export.
//!
//! Three columns under a toolbar: what to look at, what matched, and the one
//! you picked.
//!
//! ## What wiring it found
//!
//! `main` was `let app = App::new(); let _cmds = app.render();` — it built the
//! library, drew one frame into a `Vec`, dropped it and returned. No window
//! was ever opened, so no frame reached a screen and no key or click ever
//! arrived.
//!
//! **The program invented its own window and then drew in absolute pixels
//! inside it.** `render` took no size at all; every one of the five drawing
//! passes was placed against `WINDOW_WIDTH`, `WINDOW_HEIGHT`,
//! `SIDEBAR_WIDTH = 200`, `LIST_WIDTH = 280` and `TOOLBAR_HEIGHT = 44`. A
//! window the user made narrower did not narrow the columns, it cut the
//! editor off; a taller one left a band of nothing under all three. Every
//! band, column, row height and font size is solved from the live window size
//! every frame now.
//!
//! **There was no input of any kind.** No `handle_event`, no key arm, no
//! mouse arm — not a single line anywhere in the file that could change the
//! model. Every piece of view state on `App` was read by the drawing pass and
//! written by nothing at all: `sidebar_view`, `sort_order`, `search_query`,
//! `search_scope`, `selected_snippet_id`, `selected_folder_id`, `show_stats`,
//! `list_scroll` and `scroll_offset` were nine fields describing a picture of
//! one fixed state. The whole program was that picture.
//!
//! **The toolbar drew its buttons on top of its search box.** The four
//! buttons started at a hardcoded `bx = 180.0` and each grew by its own
//! measured label; the search box began at `WINDOW_WIDTH - 320.0`. Nothing
//! checked the first ever stopped before the second, and the title's
//! `max_width` was a fixed `160.0` chosen to match the 180 by eye. The row
//! measures itself and drops what will not fit, right to left, now.
//!
//! **The sidebar's lists had holes in them.** A folder's row was placed at
//! `items_y + fi * 26.0` where `fi` counts *every* folder including the
//! nested ones the loop skips past — so one nested folder left a blank row
//! where the next top-level one should have been. The language list had the
//! same fault with the languages it skips for having no snippets, which is
//! most of them: twelve languages, three in use, and the three drawn at rows
//! 0, 4 and 9. Nested folders are now drawn as a tree under their parent,
//! which is what `Folder::expanded` — set to `true` in three places and read
//! nowhere — was always for.
//!
//! **The snippet list drew through its own header and out of the bottom of
//! the window.** A row was skipped only when it was a full row above the top
//! or entirely below `WINDOW_HEIGHT`, so a row scrolled up by half its height
//! drew over the sort header, and the last row drew past the window edge on
//! to whatever came next. Nothing clipped. Both panels clip to their own
//! rectangle now, and `guitk::scroll_window` decides which rows are in them.
//!
//! **Both scroll offsets were pixel counts into things made of rows.** The
//! code panel divided `scroll_offset` by a line height to get a line number
//! and the list subtracted `list_scroll` from a row's `y`, so both could
//! express positions the renderer then rounded away — and neither had any
//! upper bound, because neither was ever assigned. Both are row indices now,
//! clamped to the last page.
//!
//! **`use_snippet` recorded uses of snippets that do not exist.** The `if let`
//! that bumps the count quietly finds nothing for an unknown id, and the
//! `recently_used.insert(0, id)` beneath it runs regardless — so an id that
//! was never a snippet took one of the twenty places in the recent list and
//! pushed a real entry off the end of it.
//!
//! **`create_snippet` and `create_folder` said "no" by returning `0`,** which
//! is the same `u64` a real id is. Nothing distinguishes a refused create from
//! one that produced snippet zero except knowing that `IdGen` starts at one.
//! Both return an `Option` now.
//!
//! **The toolbar had an Import button for a feature the program does not
//! have.** There is no import function in this file and never was; the module
//! doc claimed "Import/export (JSON format)" and only the export half was ever
//! written. Export now writes the JSON to a file and says on the status line
//! whether it worked, and Import is gone until there is something behind it.
//!
//! **Seventeen lints were blanket-allowed at the top of the file**, in sixteen
//! `#![allow(...)]`, `dead_code` among them — which is what let a program whose
//! `main` discards its own render compile without a word of complaint, along
//! with the six `edit_*` fields and the `editing` flag of an editor that could
//! not be entered, `modified_at` (set to the same value as `created_at` at the
//! one site that set either, then never read, never exported and never updated,
//! because nothing in the program can modify a snippet), and `apply_template`,
//! which nothing but a test has ever called.

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontFamily, FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::{scroll_window, text, wheel};
use oswindow::app::{self, App as WindowApp, Response};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

// ============================================================================
// Catppuccin Mocha theme
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const SKY: Color = Color::from_hex(0x89DCEB);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);

// ============================================================================
// Layout constants
// ============================================================================

/// The window the program asks for when it opens. Only the first frame is
/// this size; every frame after it is whatever the user has left the window
/// at, and everything below is solved from that (see [`Layout::new`]).
const WINDOW_WIDTH: f32 = 1100.0;
/// See [`WINDOW_WIDTH`].
const WINDOW_HEIGHT: f32 = 750.0;

/// The most of the window the sidebar may take, and the most it will ever
/// want. A share, because a wide window should not give a column of six
/// labels a quarter of itself.
const SIDEBAR_SHARE: f32 = 0.18;
/// The widest the sidebar is allowed to be, whatever the share works out to.
const SIDEBAR_MAX: f32 = 220.0;
/// See [`SIDEBAR_SHARE`]. The list is wider because it carries titles.
const LIST_SHARE: f32 = 0.26;
/// See [`SIDEBAR_MAX`].
const LIST_MAX: f32 = 320.0;

/// How many lines of the code panel a window's height is worth, which is what
/// sets the body font size. A bigger number means smaller text and more lines.
const LINES_PER_WINDOW: f32 = 46.0;

/// The most of the toolbar the search box may take, and the most it will want.
/// A share for the same reason the sidebar has one: a 4K window should not
/// hand a one-line query field two thousand pixels.
const SEARCH_SHARE: f32 = 0.34;
/// See [`SEARCH_SHARE`].
const SEARCH_MAX: f32 = 420.0;

/// The share of the window the statistics overlay is allowed to grow to. It
/// is sized from what it holds first; this is the ceiling, so a small window
/// gets a small dialog instead of one drawn off two edges.
const OVERLAY_SHARE: f32 = 0.8;
/// How many languages the overlay lists. The rest are in the counts above it.
const LANGUAGES_ON_OVERLAY: usize = 6;

/// How many tags a snippet row shows before the rest are left off. The row is
/// one line and a title has to fit on it too.
const TAGS_ON_A_ROW: usize = 3;

/// The widest line number the gutter is sized for. Measured rather than
/// guessed at, so the gutter is right in whatever face the mono family is.
const GUTTER_WIDEST: &str = "9999";

const TOOLBAR_TITLE: &str = "Snippets";
const STATS_TITLE: &str = "Library statistics";
const SEARCH_PLACEHOLDER: &str = "Search snippets";
const CLEAR_MARK: &str = "x";
const STAR: &str = "*";
const TWISTY_OPEN: &str = "v";
const TWISTY_SHUT: &str = ">";
const USE_LABEL: &str = "Use";
const DELETE_LABEL: &str = "Delete";
const TEMPLATE_LABEL: &str = "TEMPLATE";
const NEW_FOLDER_LABEL: &str = "+ New folder";
const EMPTY_LIST: &str = "Nothing here";
const EMPTY_HEADLINE: &str = "No snippet selected";
const EMPTY_SUBLINE: &str = "Pick one from the list, or press N for a new one";

// The code panel — and only the code panel — is a grid: it has a line-number
// gutter, and a reader lines indentation up by eye between rows, so column *n*
// of one line has to sit above column *n* of the next. Everything else in this
// app is proportional UI chrome.
//
// There used to be a `char_width()` and a `columns()` here, and the token pen
// advanced by their product. That is the *third* version of one mistake, each
// version a step further out than the last: a hardcoded 8.0 that drifted the
// moment the face or size changed; then `text::digit_advance`, a digit's
// advance in the *proportional* face, which is a cell only digits fit, so real
// source stepped short and consecutive tokens overlapped; then
// `text::cell_advance` in the mono face, which is right for Latin text and
// still only a nominal count — one cell per character is not what the renderer
// does with an ideograph, a combining mark, or a character the face lacks.
//
// The pen now advances by measuring each token exactly as it will be drawn.
// That is the renderer's own answer, so there is nothing left to keep in step:
// see `draw_code`'s inner comment, and `the_pen_advances_by_what_is_drawn`.

const MAX_SNIPPETS: usize = 5000;
const MAX_FOLDERS: usize = 200;
const MAX_TAGS: usize = 500;
const MAX_CONTENT_LEN: usize = 65536;
const MAX_RECENT: usize = 20;

/// Where the Export button writes when nothing has said otherwise.
const DEFAULT_EXPORT_NAME: &str = "snippets-export.json";

// ============================================================================
// Language Support
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    C,
    Cpp,
    Java,
    Go,
    Shell,
    Sql,
    Html,
    Css,
    PlainText,
}

impl Language {
    fn name(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::Python => "Python",
            Self::JavaScript => "JavaScript",
            Self::TypeScript => "TypeScript",
            Self::C => "C",
            Self::Cpp => "C++",
            Self::Java => "Java",
            Self::Go => "Go",
            Self::Shell => "Shell",
            Self::Sql => "SQL",
            Self::Html => "HTML",
            Self::Css => "CSS",
            Self::PlainText => "Plain Text",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Rust => "rs",
            Self::Python => "py",
            Self::JavaScript => "js",
            Self::TypeScript => "ts",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Java => "java",
            Self::Go => "go",
            Self::Shell => "sh",
            Self::Sql => "sql",
            Self::Html => "html",
            Self::Css => "css",
            Self::PlainText => "txt",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Rust => PEACH,
            Self::Python => BLUE,
            Self::JavaScript => YELLOW,
            Self::TypeScript => BLUE,
            Self::C => TEAL,
            Self::Cpp => TEAL,
            Self::Java => RED,
            Self::Go => SKY,
            Self::Shell => GREEN,
            Self::Sql => MAUVE,
            Self::Html => PEACH,
            Self::Css => LAVENDER,
            Self::PlainText => SUBTEXT0,
        }
    }

    fn keywords(self) -> &'static [&'static str] {
        match self {
            Self::Rust => &[
                "fn", "let", "mut", "pub", "struct", "enum", "impl", "trait", "use", "mod",
                "match", "if", "else", "for", "while", "loop", "return", "self", "Self", "const",
                "static", "type", "where", "async", "await", "move", "ref", "unsafe", "extern",
                "crate",
            ],
            Self::Python => &[
                "def", "class", "import", "from", "if", "elif", "else", "for", "while", "return",
                "yield", "with", "as", "try", "except", "finally", "raise", "pass", "break",
                "continue", "lambda", "and", "or", "not", "in", "is", "True", "False", "None",
            ],
            Self::JavaScript | Self::TypeScript => &[
                "function",
                "const",
                "let",
                "var",
                "if",
                "else",
                "for",
                "while",
                "return",
                "class",
                "new",
                "this",
                "import",
                "export",
                "default",
                "async",
                "await",
                "try",
                "catch",
                "throw",
                "typeof",
                "instanceof",
                "null",
                "undefined",
                "true",
                "false",
            ],
            Self::C | Self::Cpp => &[
                "int", "char", "float", "double", "void", "if", "else", "for", "while", "do",
                "return", "struct", "typedef", "enum", "switch", "case", "break", "continue",
                "sizeof", "static", "const", "unsigned", "signed", "long", "short", "extern",
                "include", "define", "ifdef", "ifndef", "endif",
            ],
            Self::Java => &[
                "class",
                "public",
                "private",
                "protected",
                "static",
                "void",
                "int",
                "boolean",
                "String",
                "new",
                "return",
                "if",
                "else",
                "for",
                "while",
                "import",
                "package",
                "extends",
                "implements",
                "interface",
                "try",
                "catch",
                "throw",
                "throws",
                "final",
                "abstract",
                "synchronized",
                "this",
                "super",
                "null",
                "true",
                "false",
            ],
            Self::Go => &[
                "func",
                "package",
                "import",
                "var",
                "const",
                "type",
                "struct",
                "interface",
                "map",
                "chan",
                "go",
                "defer",
                "return",
                "if",
                "else",
                "for",
                "range",
                "switch",
                "case",
                "select",
                "break",
                "continue",
                "nil",
                "true",
                "false",
                "make",
                "append",
                "len",
                "cap",
            ],
            Self::Shell => &[
                "if", "then", "else", "elif", "fi", "for", "do", "done", "while", "until", "case",
                "esac", "function", "return", "echo", "exit", "export", "source", "local",
                "readonly", "shift", "set", "unset", "eval", "exec", "trap",
            ],
            Self::Sql => &[
                "SELECT", "FROM", "WHERE", "INSERT", "UPDATE", "DELETE", "CREATE", "DROP", "ALTER",
                "TABLE", "INDEX", "VIEW", "JOIN", "LEFT", "RIGHT", "INNER", "OUTER", "ON", "AND",
                "OR", "NOT", "IN", "LIKE", "ORDER", "BY", "GROUP", "HAVING", "LIMIT", "OFFSET",
                "AS", "NULL", "INTO", "VALUES", "SET", "DISTINCT", "COUNT", "SUM", "AVG",
            ],
            Self::Html => &[
                "html", "head", "body", "div", "span", "p", "a", "img", "table", "tr", "td", "th",
                "ul", "ol", "li", "form", "input", "button", "script", "style", "link", "meta",
                "h1", "h2", "h3", "h4", "h5", "h6", "br", "hr",
            ],
            Self::Css => &[
                "color",
                "background",
                "margin",
                "padding",
                "border",
                "font",
                "display",
                "position",
                "width",
                "height",
                "flex",
                "grid",
                "align",
                "justify",
                "transform",
                "transition",
                "animation",
                "opacity",
                "overflow",
                "cursor",
                "z-index",
                "box-shadow",
                "text-align",
            ],
            Self::PlainText => &[],
        }
    }

    fn from_extension(ext: &str) -> Self {
        match ext.to_ascii_lowercase().as_str() {
            "rs" => Self::Rust,
            "py" | "pyw" => Self::Python,
            "js" | "jsx" | "mjs" => Self::JavaScript,
            "ts" | "tsx" => Self::TypeScript,
            "c" | "h" => Self::C,
            "cpp" | "cc" | "cxx" | "hpp" | "hxx" => Self::Cpp,
            "java" => Self::Java,
            "go" => Self::Go,
            "sh" | "bash" | "zsh" => Self::Shell,
            "sql" => Self::Sql,
            "html" | "htm" => Self::Html,
            "css" | "scss" | "less" => Self::Css,
            _ => Self::PlainText,
        }
    }

    fn detect_from_content(content: &str) -> Self {
        let first_line = content.lines().next().unwrap_or("");

        // Shebang detection
        if first_line.starts_with("#!") {
            if first_line.contains("python") {
                return Self::Python;
            }
            if first_line.contains("node") {
                return Self::JavaScript;
            }
            if first_line.contains("bash") || first_line.contains("sh") {
                return Self::Shell;
            }
        }

        // Keyword-based heuristic. `fn ` is a strong Rust signal (no other
        // supported language uses it); pair it with any one of the common Rust
        // tokens rather than requiring all of them, since plenty of valid Rust
        // has no `->` or `::`.
        if content.contains("fn ")
            && (content.contains("let ") || content.contains("->") || content.contains("::"))
        {
            return Self::Rust;
        }
        if content.contains("def ") && content.contains("import ") && !content.contains('{') {
            return Self::Python;
        }
        if content.contains("func ") && content.contains("package ") {
            return Self::Go;
        }
        if content.contains("public class ") || content.contains("System.out") {
            return Self::Java;
        }
        if content.contains("SELECT ") || content.contains("CREATE TABLE") {
            return Self::Sql;
        }
        if content.contains("<!DOCTYPE") || content.contains("<html") {
            return Self::Html;
        }
        if (content.contains("function ") || content.contains("const ") || content.contains("=>"))
            && content.contains('{')
        {
            return Self::JavaScript;
        }
        if content.contains("#include") && content.contains("int main") {
            return Self::C;
        }

        Self::PlainText
    }

    fn all() -> &'static [Self] {
        &[
            Self::Rust,
            Self::Python,
            Self::JavaScript,
            Self::TypeScript,
            Self::C,
            Self::Cpp,
            Self::Java,
            Self::Go,
            Self::Shell,
            Self::Sql,
            Self::Html,
            Self::Css,
            Self::PlainText,
        ]
    }
}

// ============================================================================
// Syntax Highlighting
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenKind {
    Keyword,
    String,
    Number,
    Comment,
    Operator,
    Punctuation,
    Identifier,
    Type,
    Plain,
}

impl TokenKind {
    fn color(self) -> Color {
        match self {
            Self::Keyword => MAUVE,
            Self::String => GREEN,
            Self::Number => PEACH,
            Self::Comment => OVERLAY0,
            Self::Operator => RED,
            Self::Punctuation => SUBTEXT1,
            Self::Identifier => TEXT,
            Self::Type => YELLOW,
            Self::Plain => TEXT,
        }
    }
}

#[derive(Debug, Clone)]
struct Token {
    text: String,
    kind: TokenKind,
}

fn tokenize(content: &str, language: Language) -> Vec<Vec<Token>> {
    let keywords = language.keywords();
    let mut result = Vec::new();

    for line in content.lines() {
        let tokens = tokenize_line(line, keywords, language);
        result.push(tokens);
    }

    // `str::lines()` yields nothing for an empty string, but an empty document
    // still has a single (empty) line in editor terms.
    if result.is_empty() {
        result.push(Vec::new());
    }

    result
}

fn tokenize_line(line: &str, keywords: &[&str], language: Language) -> Vec<Token> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;

    while let Some(&c) = chars.get(i) {
        // Comment detection. One arm each, because the three are three
        // different languages' rules, but one body: whatever the rule, the
        // rest of the line is the comment.
        let comment_starts_here = match c {
            '/' => chars.get(i.saturating_add(1)) == Some(&'/'),
            '#' => matches!(language, Language::Python | Language::Shell),
            '-' => chars.get(i.saturating_add(1)) == Some(&'-') && language == Language::Sql,
            _ => false,
        };
        if comment_starts_here {
            tokens.push(Token {
                text: chars.get(i..).unwrap_or_default().iter().collect(),
                kind: TokenKind::Comment,
            });
            break;
        }

        // String detection
        if c == '"' || c == '\'' || c == '`' {
            let quote = c;
            let mut s = String::new();
            s.push(c);
            i = i.saturating_add(1);
            while let Some(&sc) = chars.get(i) {
                s.push(sc);
                if sc == '\\' {
                    i = i.saturating_add(1);
                    if let Some(&escaped) = chars.get(i) {
                        s.push(escaped);
                    }
                } else if sc == quote {
                    break;
                }
                i = i.saturating_add(1);
            }
            tokens.push(Token {
                text: s,
                kind: TokenKind::String,
            });
            i = i.saturating_add(1);
            continue;
        }

        // Number
        if c.is_ascii_digit()
            || (c == '.'
                && chars
                    .get(i.saturating_add(1))
                    .is_some_and(char::is_ascii_digit))
        {
            let mut n = String::new();
            while let Some(&d) = chars.get(i) {
                if !(d.is_ascii_digit()
                    || d == '.'
                    || d == 'x'
                    || d == 'b'
                    || (d.is_ascii_hexdigit() && n.contains("0x")))
                {
                    break;
                }
                n.push(d);
                i = i.saturating_add(1);
            }
            tokens.push(Token {
                text: n,
                kind: TokenKind::Number,
            });
            continue;
        }

        // Identifier/keyword
        if c.is_ascii_alphabetic() || c == '_' {
            let mut ident = String::new();
            while let Some(&d) = chars.get(i) {
                if !(d.is_ascii_alphanumeric() || d == '_') {
                    break;
                }
                ident.push(d);
                i = i.saturating_add(1);
            }

            let kind = if keywords.contains(&ident.as_str()) {
                TokenKind::Keyword
            } else if ident.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                && language != Language::Sql
            {
                TokenKind::Type
            } else {
                TokenKind::Identifier
            };

            tokens.push(Token { text: ident, kind });
            continue;
        }

        // Operators
        if "+-*/%=<>!&|^~".contains(c) {
            let mut op = String::new();
            op.push(c);
            i = i.saturating_add(1);
            // Check for two-char operators
            if let Some(&next) = chars.get(i)
                && "=>&|+-".contains(next)
            {
                op.push(next);
                i = i.saturating_add(1);
            }
            tokens.push(Token {
                text: op,
                kind: TokenKind::Operator,
            });
            continue;
        }

        // Punctuation
        if "(){}[].,;:@#?".contains(c) {
            tokens.push(Token {
                text: c.to_string(),
                kind: TokenKind::Punctuation,
            });
            i = i.saturating_add(1);
            continue;
        }

        // Whitespace and other
        tokens.push(Token {
            text: c.to_string(),
            kind: TokenKind::Plain,
        });
        i = i.saturating_add(1);
    }

    tokens
}

// ============================================================================
// Data Model
// ============================================================================

type SnippetId = u64;
type FolderId = u64;

#[derive(Debug, Clone)]
struct Snippet {
    id: SnippetId,
    title: String,
    content: String,
    language: Language,
    folder_id: Option<FolderId>,
    tags: Vec<String>,
    favorite: bool,
    /// When it was made. Also what "Oldest"/"Newest" sort by.
    ///
    /// There used to be a `modified_at` beside this, set to the same value at
    /// the one site that sets either and then never read, never exported and
    /// never updated — because nothing in this program can modify a snippet.
    /// `#![allow(dead_code)]` was what kept it from being said out loud.
    created_at: u64,
    use_count: u32,
    description: String,
    is_template: bool,
    template_vars: Vec<String>,
}

#[derive(Debug, Clone)]
struct Folder {
    id: FolderId,
    name: String,
    parent_id: Option<FolderId>,
    expanded: bool,
    color: Color,
}

struct IdGen {
    next: u64,
}

impl IdGen {
    fn new() -> Self {
        Self { next: 1 }
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next;
        self.next = self.next.saturating_add(1);
        id
    }
}

// ============================================================================
// Search
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchScope {
    All,
    Title,
    Content,
    Tags,
}

impl SearchScope {
    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Title => "Title",
            Self::Content => "Content",
            Self::Tags => "Tags",
        }
    }

    /// The next scope the Scope button steps to.
    ///
    /// Written as a match rather than as arithmetic on a discriminant so that
    /// adding a scope is a compile error here rather than a scope the button
    /// silently skips.
    fn next(self) -> Self {
        match self {
            Self::All => Self::Title,
            Self::Title => Self::Content,
            Self::Content => Self::Tags,
            Self::Tags => Self::All,
        }
    }
}

fn search_snippets<'a>(
    snippets: &'a [Snippet],
    query: &str,
    scope: SearchScope,
) -> Vec<&'a Snippet> {
    if query.is_empty() {
        return snippets.iter().collect();
    }

    let lower_query = query.to_ascii_lowercase();
    snippets
        .iter()
        .filter(|s| match scope {
            SearchScope::All => {
                s.title.to_ascii_lowercase().contains(&lower_query)
                    || s.content.to_ascii_lowercase().contains(&lower_query)
                    || s.tags
                        .iter()
                        .any(|t| t.to_ascii_lowercase().contains(&lower_query))
                    || s.description.to_ascii_lowercase().contains(&lower_query)
            }
            SearchScope::Title => s.title.to_ascii_lowercase().contains(&lower_query),
            SearchScope::Content => s.content.to_ascii_lowercase().contains(&lower_query),
            SearchScope::Tags => s
                .tags
                .iter()
                .any(|t| t.to_ascii_lowercase().contains(&lower_query)),
        })
        .collect()
}

// ============================================================================
// Import/Export
// ============================================================================

fn export_snippets_json(snippets: &[Snippet]) -> String {
    use std::fmt::Write as _;
    let mut json = String::from("{\n  \"snippets\": [\n");

    for (i, snippet) in snippets.iter().enumerate() {
        json.push_str("    {\n");
        let _ = writeln!(json, "      \"title\": {},", json_escape(&snippet.title));
        let _ = writeln!(json, "      \"language\": \"{}\",", snippet.language.name());
        let _ = writeln!(
            json,
            "      \"content\": {},",
            json_escape(&snippet.content)
        );
        let _ = writeln!(
            json,
            "      \"description\": {},",
            json_escape(&snippet.description)
        );

        json.push_str("      \"tags\": [");
        for (ti, tag) in snippet.tags.iter().enumerate() {
            if ti > 0 {
                json.push_str(", ");
            }
            json.push_str(&json_escape(tag));
        }
        json.push_str("],\n");

        let _ = writeln!(json, "      \"favorite\": {},", snippet.favorite);
        let _ = writeln!(json, "      \"is_template\": {}", snippet.is_template);

        json.push_str("    }");
        if i < snippets.len().saturating_sub(1) {
            json.push(',');
        }
        json.push('\n');
    }

    json.push_str("  ]\n}\n");
    json
}

/// Render a string as a complete JSON string literal, surrounding quotes
/// included.
fn json_escape(s: &str) -> String {
    format!("\"{}\"", guitk::escape::json_string(s))
}

// ============================================================================
// Template Processing
// ============================================================================

fn extract_template_vars(content: &str) -> Vec<String> {
    let mut vars = Vec::new();
    let chars: Vec<char> = content.chars().collect();
    let mut i = 0;

    while let Some(&c) = chars.get(i) {
        if c == '$' && chars.get(i.saturating_add(1)) == Some(&'{') {
            i = i.saturating_add(2);
            let mut var = String::new();
            while let Some(&v) = chars.get(i) {
                if v == '}' {
                    break;
                }
                var.push(v);
                i = i.saturating_add(1);
            }
            if !var.is_empty() && !vars.contains(&var) {
                vars.push(var);
            }
        }
        i = i.saturating_add(1);
    }

    vars
}

/// What language a snippet with this title and this content is.
///
/// The name is asked first because it is what the user typed and the content
/// is what a heuristic guesses at; the content is only consulted when the name
/// carries no extension, or carries one nobody recognises. Both halves —
/// [`Language::from_extension`] and [`Language::detect_from_content`] — were
/// written and then never called by anything, which is what the crate-root
/// `#![allow(dead_code)]` was for.
fn guess_language(title: &str, content: &str) -> Language {
    if let Some((_, ext)) = title.rsplit_once('.')
        && !ext.is_empty()
    {
        let by_extension = Language::from_extension(ext);
        if by_extension != Language::PlainText {
            return by_extension;
        }
    }
    Language::detect_from_content(content)
}

fn apply_template(content: &str, vars: &[(String, String)]) -> String {
    let mut result = content.to_string();
    for (name, value) in vars {
        let placeholder = format!("${{{name}}}");
        result = result.replace(&placeholder, value);
    }
    result
}

// ============================================================================
// Statistics
// ============================================================================

pub struct LibraryStats {
    total_snippets: usize,
    total_folders: usize,
    total_tags: usize,
    favorites: usize,
    templates: usize,
    by_language: Vec<(Language, usize)>,
    total_lines: usize,
    total_chars: usize,
}

fn compute_stats(snippets: &[Snippet], folders: &[Folder]) -> LibraryStats {
    let total_snippets = snippets.len();
    let total_folders = folders.len();
    let favorites = snippets.iter().filter(|s| s.favorite).count();
    let templates = snippets.iter().filter(|s| s.is_template).count();

    let mut tag_set: Vec<String> = Vec::new();
    for snippet in snippets {
        for tag in &snippet.tags {
            if !tag_set.contains(tag) {
                tag_set.push(tag.clone());
            }
        }
    }
    let total_tags = tag_set.len();

    let mut by_language: Vec<(Language, usize)> = Vec::new();
    for lang in Language::all() {
        let count = snippets.iter().filter(|s| s.language == *lang).count();
        if count > 0 {
            by_language.push((*lang, count));
        }
    }
    by_language.sort_by_key(|&(_, count)| std::cmp::Reverse(count));

    let total_lines: usize = snippets.iter().map(|s| s.content.lines().count()).sum();
    let total_chars: usize = snippets.iter().map(|s| s.content.len()).sum();

    LibraryStats {
        total_snippets,
        total_folders,
        total_tags,
        favorites,
        templates,
        by_language,
        total_lines,
        total_chars,
    }
}

// ============================================================================
// Application State
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarView {
    Folders,
    Tags,
    Languages,
    Favorites,
    Recent,
    Templates,
}

impl SidebarView {
    /// Every view, in the order the sidebar lists them and Tab steps through
    /// them. The drawing and the keyboard both read this, so neither can
    /// offer a view the other does not.
    const ALL: [Self; 6] = [
        Self::Folders,
        Self::Tags,
        Self::Languages,
        Self::Favorites,
        Self::Recent,
        Self::Templates,
    ];

    /// The view Tab steps to. See [`SearchScope::next`] for why it is a match.
    fn next(self) -> Self {
        match self {
            Self::Folders => Self::Tags,
            Self::Tags => Self::Languages,
            Self::Languages => Self::Favorites,
            Self::Favorites => Self::Recent,
            Self::Recent => Self::Templates,
            Self::Templates => Self::Folders,
        }
    }

    /// The view Shift+Tab steps to.
    fn prev(self) -> Self {
        match self {
            Self::Folders => Self::Templates,
            Self::Tags => Self::Folders,
            Self::Languages => Self::Tags,
            Self::Favorites => Self::Languages,
            Self::Recent => Self::Favorites,
            Self::Templates => Self::Recent,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Folders => "Folders",
            Self::Tags => "Tags",
            Self::Languages => "Languages",
            Self::Favorites => "Favorites",
            Self::Recent => "Recent",
            Self::Templates => "Templates",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Folders => "[D]",
            Self::Tags => "[#]",
            Self::Languages => "[<>]",
            Self::Favorites => "[*]",
            Self::Recent => "[~]",
            Self::Templates => "[T]",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortOrder {
    NameAsc,
    NameDesc,
    DateAsc,
    DateDesc,
    UsageDesc,
    LanguageAsc,
}

impl SortOrder {
    fn label(self) -> &'static str {
        match self {
            Self::NameAsc => "Name A-Z",
            Self::NameDesc => "Name Z-A",
            Self::DateAsc => "Oldest",
            Self::DateDesc => "Newest",
            Self::UsageDesc => "Most Used",
            Self::LanguageAsc => "Language",
        }
    }

    /// The order the Sort button steps to. See [`SearchScope::next`].
    fn next(self) -> Self {
        match self {
            Self::NameAsc => Self::NameDesc,
            Self::NameDesc => Self::DateAsc,
            Self::DateAsc => Self::DateDesc,
            Self::DateDesc => Self::UsageDesc,
            Self::UsageDesc => Self::LanguageAsc,
            Self::LanguageAsc => Self::NameAsc,
        }
    }
}

// ============================================================================
// What a click can land on
// ============================================================================

/// Everything on the screen a pointer can hit.
///
/// The drawing pass records these as it draws, so a control is clickable
/// exactly where its ink is and nowhere else — the geometry is written down
/// once (`known-issues.md` lesson 63). Before this there was no pointer
/// handling at all, so there was no second copy of the geometry to disagree
/// with the first; there was no first either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Add a snippet.
    New,
    /// Write the library out as JSON.
    Export,
    /// Open the statistics overlay.
    Stats,
    /// Shut it again. The whole backdrop is this, so a click anywhere outside
    /// the dialog dismisses it.
    CloseStats,
    /// The search box. Clicking it is what sends typing there.
    Search,
    /// The cross at the end of the search box, drawn only when there is a
    /// query to clear.
    ClearSearch,
    /// Which fields the query is matched against. Clicking cycles.
    Scope,
    /// The list's sort order. Clicking cycles.
    Sort,
    /// One of the six things the sidebar can be showing.
    View(SidebarView),
    /// A folder, in the Folders view.
    Folder(FolderId),
    /// The triangle in front of a folder that has children. Separate from
    /// [`Target::Folder`] so that opening a folder and selecting it are two
    /// different clicks, as they are in every other tree.
    Twisty(FolderId),
    /// The row under the folder tree that makes a folder.
    NewFolder,
    /// The cross on the selected folder's row. Only the selected folder has
    /// one, so a mis-aimed click in a tree cannot delete a folder.
    DeleteFolder(FolderId),
    /// A tag, by its position in [`App::all_tags`].
    Tag(usize),
    /// A language, in the Languages view.
    Lang(Language),
    /// A snippet's row in the list.
    Row(SnippetId),
    /// The star on a row.
    Star(SnippetId),
    /// Count a use of the selected snippet, and fill it if it is a template.
    Use,
    /// Delete the selected snippet.
    Delete,
    /// The body of the list, so the wheel over it scrolls the list.
    List,
    /// The code panel, so the wheel over it scrolls the code.
    Code,
}

/// Whether an event changed anything the window would need to redraw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventResult {
    Consumed,
    Ignored,
}

// ============================================================================
// Layout
// ============================================================================

/// The bands and columns a window of a given size is divided into.
///
/// Built fresh every frame from the live window size and never stored on the
/// model, because a remembered layout is one that can disagree with the window
/// it is drawn in — which is what the fixed `SIDEBAR_WIDTH`, `LIST_WIDTH` and
/// `TOOLBAR_HEIGHT` this replaces amounted to, with `WINDOW_WIDTH` standing in
/// for a width nobody had asked the window for.
/// The three bands the editor column is split into.
///
/// One struct rather than three functions because the three are solved
/// together — the code panel is what the header and the status bar leave —
/// and three functions that each re-derive the other two are three places for
/// them to disagree (known-issues lesson 63).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorParts {
    /// Title, language and description.
    pub header: Rect,
    /// The scrolling source view, inset by the padding.
    pub code: Rect,
    /// Line count, tags, and what the last export did.
    pub status: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    /// Title, buttons, search box, count.
    pub toolbar: Rect,
    /// The navigation column. Empty when the window has no room to spare for
    /// it.
    pub sidebar: Rect,
    /// The results column. Empty when the window has no room to spare for it.
    pub list: Rect,
    /// What is left. Never empty while the window has any width at all: the
    /// editor is what the program is for, so it is never the column that gives
    /// way.
    pub editor: Rect,
    /// The toolbar's title.
    pub title: f32,
    /// A panel heading, and the selected snippet's title.
    pub head: f32,
    /// Body text.
    pub font: f32,
    /// Secondary text: counts, the status line, folder names.
    pub small: f32,
    /// Badges and tag pills.
    pub tiny: f32,
    /// The margin between a band and what is inside it.
    pub pad: f32,
    /// The height of one sidebar row.
    pub row: f32,
    /// The height of one snippet row in the list.
    pub list_row: f32,
    /// The height of one line of code.
    pub line: f32,
}

impl Layout {
    /// Solve the bands and columns for a window of this size.
    #[must_use]
    pub fn new(w: f32, h: f32) -> Self {
        let w = w.max(0.0);
        let h = h.max(0.0);
        let pad = (w.min(h) * 0.012).clamp(2.0, 10.0);
        let font = (h / LINES_PER_WINDOW).clamp(8.0, 16.0);
        let small = (font - 2.0).max(7.0);
        let tiny = (font - 4.0).max(6.0);
        let head = font + 2.0;
        let title = font + 4.0;

        let row = text::line_height(font, FontWeightHint::Regular) + pad;
        // A list row carries a badge, a title and a line of tags, so it is
        // three lines tall plus its own margin. Measured rather than the old
        // flat 58 pixels, which was three lines at one font size and two at
        // another.
        let list_row = text::line_height(tiny, FontWeightHint::Bold)
            + text::line_height(font, FontWeightHint::Bold)
            + text::line_height(tiny, FontWeightHint::Regular)
            + pad * 2.0;
        let line = text::line_height_in(font, FontWeightHint::Regular, FontFamily::Mono);

        let toolbar_h = (text::line_height(title, FontWeightHint::Bold) + pad * 2.0).min(h);
        let toolbar = Rect::new(0.0, 0.0, w, toolbar_h);
        let body_y = toolbar.bottom();
        let body_h = (h - toolbar_h).max(0.0);

        // Which of the two left-hand columns there is room for.
        //
        // The editor is what the program is for, so it is never the column
        // that gives way. A window too narrow to hold a column *and* leave the
        // editor something worth having drops that column outright, rather
        // than draw a strip of ellipses and take the room the code needed.
        // Navigation goes first and the results list second, because a list
        // you can search is more use than a tree you cannot read.
        let want_side = (w * SIDEBAR_SHARE).min(SIDEBAR_MAX);
        let want_list = (w * LIST_SHARE).min(LIST_MAX);
        let least_editor = font * 24.0;
        let sidebar_w = if w - want_side - want_list >= least_editor {
            want_side
        } else {
            0.0
        };
        let list_w = if w - sidebar_w - want_list >= least_editor {
            want_list
        } else {
            0.0
        };

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            toolbar,
            sidebar: Rect::new(0.0, body_y, sidebar_w, body_h),
            list: Rect::new(sidebar_w, body_y, list_w, body_h),
            editor: Rect::new(
                sidebar_w + list_w,
                body_y,
                (w - sidebar_w - list_w).max(0.0),
                body_h,
            ),
            title,
            head,
            font,
            small,
            tiny,
            pad,
            row,
            list_row,
            line,
        }
    }
}

/// The library and everything about how it is being looked at.
///
/// The six `edit_*` fields and the `editing` flag that used to sit at the
/// bottom of this struct are gone. They were an editor that could not be
/// entered — set once in `new` and read by nothing, in a file that allowed
/// `dead_code` — so nothing could tell whether they were right. What is left
/// is state something can actually put a value into.
pub struct App {
    // Data
    snippets: Vec<Snippet>,
    folders: Vec<Folder>,
    id_gen: IdGen,

    // Selection
    selected_snippet_id: Option<SnippetId>,
    selected_folder_id: Option<FolderId>,
    /// The tag picked in the Tags view, if any.
    ///
    /// The Tags and Languages views used to list things that filtered nothing:
    /// `filtered_snippets` had one `_ => {}` arm covering both, so switching to
    /// them changed what the sidebar showed and left the list showing every
    /// snippet in the library. These two fields are what the lists are for.
    selected_tag: Option<String>,
    /// The language picked in the Languages view, if any. See
    /// [`App::selected_tag`].
    selected_language: Option<Language>,

    // View state
    sidebar_view: SidebarView,
    sort_order: SortOrder,
    search_query: String,
    search_scope: SearchScope,
    /// Whether what is typed goes into the search box. A letter means "find
    /// this" while it is set and "do this" while it is not, which is the only
    /// thing that lets one keyboard serve both a text field and a set of
    /// shortcuts.
    search_focus: bool,

    /// The first line of the selected snippet on show, as a line number.
    ///
    /// It was `scroll_offset: f32`, a count of pixels that the drawing pass
    /// divided by a line height to get this. A continuous offset into
    /// something drawn in whole rows can only express positions that are then
    /// rounded away (`guitk::wheel`), and this one had no bound either way.
    code_scroll: usize,
    /// The first snippet of the filtered list on show, as a row number. See
    /// [`App::code_scroll`].
    list_scroll: usize,
    /// The fractions of a wheel notch a trackpad has sent that have not yet
    /// added up to a row. Without this a device that sends fifths of a notch
    /// scrolls nothing, ever.
    wheel: wheel::Accumulator,

    recently_used: Vec<SnippetId>,
    show_stats: bool,

    /// Where [`App::export`] writes, so that a test can point it at a file of
    /// its own rather than at whatever the working directory happens to be.
    export_path: PathBuf,
    /// What the last export did, shown on the status line until the next one.
    ///
    /// A `Result` rather than a string with a mood, because the status line
    /// colours it — and a failure that reads like a success is worse than no
    /// note at all.
    export_note: Option<Result<String, String>>,

    /// The size the last frame was drawn at, which is the size the next click
    /// is read against. Only stored for that.
    size: (f32, f32),
}

impl App {
    fn new() -> Self {
        let mut id_gen = IdGen::new();
        let mut folders = Vec::new();
        let mut snippets = Vec::new();

        // Default folders
        let general_id = id_gen.next_id();
        folders.push(Folder {
            id: general_id,
            name: "General".into(),
            parent_id: None,
            expanded: true,
            color: BLUE,
        });

        let web_id = id_gen.next_id();
        folders.push(Folder {
            id: web_id,
            name: "Web Dev".into(),
            parent_id: None,
            expanded: true,
            color: PEACH,
        });

        let utils_id = id_gen.next_id();
        folders.push(Folder {
            id: utils_id,
            name: "Utilities".into(),
            parent_id: None,
            expanded: true,
            color: GREEN,
        });

        // One folder inside another, so the sidebar opens on a tree rather
        // than on a flat list. `Folder::parent_id`, the indent in
        // `draw_folder_tree`, the twisty and the recursion in `walk_folders`
        // were all written for nesting the seeded library never had — nothing
        // a user opened the program on could exercise any of them.
        let snippets_id = id_gen.next_id();
        folders.push(Folder {
            id: snippets_id,
            name: "Regex".into(),
            parent_id: Some(utils_id),
            expanded: true,
            color: TEAL,
        });

        // Sample snippets
        snippets.push(Snippet {
            id: id_gen.next_id(),
            title: "Hello World (Rust)".into(),
            content: "fn main() {\n    println!(\"Hello, world!\");\n}".into(),
            language: Language::Rust,
            folder_id: Some(general_id),
            tags: vec!["hello-world".into(), "beginner".into()],
            favorite: true,
            created_at: 1000,
            use_count: 5,
            description: "Basic Rust hello world program".into(),
            is_template: false,
            template_vars: Vec::new(),
        });

        snippets.push(Snippet {
            id: id_gen.next_id(),
            title: "HTTP Server (Python)".into(),
            content: "from http.server import HTTPServer, SimpleHTTPRequestHandler\n\ndef run(port=8080):\n    server = HTTPServer(('', port), SimpleHTTPRequestHandler)\n    print(f'Serving on port {port}')\n    server.serve_forever()\n\nif __name__ == '__main__':\n    run()".into(),
            language: Language::Python,
            folder_id: Some(web_id),
            tags: vec!["http".into(), "server".into(), "web".into()],
            favorite: false,
            created_at: 2000,
            use_count: 3,
            description: "Simple HTTP server using Python stdlib".into(),
            is_template: false,
            template_vars: Vec::new(),
        });

        snippets.push(Snippet {
            id: id_gen.next_id(),
            title: "Function Template".into(),
            content: "fn ${function_name}(${params}) -> ${return_type} {\n    ${body}\n}".into(),
            language: Language::Rust,
            folder_id: Some(utils_id),
            tags: vec!["template".into(), "function".into()],
            favorite: false,
            created_at: 3000,
            use_count: 10,
            description: "Rust function template with placeholders".into(),
            is_template: true,
            template_vars: vec![
                "function_name".into(),
                "params".into(),
                "return_type".into(),
                "body".into(),
            ],
        });

        snippets.push(Snippet {
            id: id_gen.next_id(),
            title: "SQL Select Join".into(),
            content: "SELECT u.name, o.total\nFROM users u\nINNER JOIN orders o ON u.id = o.user_id\nWHERE o.total > 100\nORDER BY o.total DESC\nLIMIT 10;".into(),
            language: Language::Sql,
            folder_id: Some(utils_id),
            tags: vec!["sql".into(), "join".into(), "query".into()],
            favorite: true,
            created_at: 4000,
            use_count: 7,
            description: "SQL join query with filtering and ordering".into(),
            is_template: false,
            template_vars: Vec::new(),
        });

        snippets.push(Snippet {
            id: id_gen.next_id(),
            title: "CSS Flexbox Center".into(),
            content: ".container {\n    display: flex;\n    justify-content: center;\n    align-items: center;\n    height: 100vh;\n}".into(),
            language: Language::Css,
            folder_id: Some(web_id),
            tags: vec!["css".into(), "flexbox".into(), "layout".into()],
            favorite: false,
            created_at: 5000,
            use_count: 12,
            description: "Center content with flexbox".into(),
            is_template: false,
            template_vars: Vec::new(),
        });

        Self {
            snippets,
            folders,
            id_gen,
            selected_snippet_id: None,
            selected_folder_id: None,
            selected_tag: None,
            selected_language: None,
            sidebar_view: SidebarView::Folders,
            sort_order: SortOrder::DateDesc,
            search_query: String::new(),
            search_scope: SearchScope::All,
            search_focus: false,
            code_scroll: 0,
            list_scroll: 0,
            wheel: wheel::Accumulator::default(),
            recently_used: Vec::new(),
            show_stats: false,
            export_path: PathBuf::from(DEFAULT_EXPORT_NAME),
            export_note: None,
            size: (WINDOW_WIDTH, WINDOW_HEIGHT),
        }
    }

    /// Add a snippet, or `None` if the library is full or the content too
    /// long.
    ///
    /// It used to answer a refusal with `0`, which is the same `u64` as a real
    /// id: nothing distinguished "did not make one" from "made snippet zero"
    /// except knowing that [`IdGen`] happens to start at one.
    fn create_snippet(
        &mut self,
        title: &str,
        content: &str,
        language: Language,
    ) -> Option<SnippetId> {
        if self.snippets.len() >= MAX_SNIPPETS || content.len() > MAX_CONTENT_LEN {
            return None;
        }

        let id = self.id_gen.next_id();
        let template_vars = extract_template_vars(content);
        let is_template = !template_vars.is_empty();

        self.snippets.push(Snippet {
            id,
            title: title.into(),
            content: content.into(),
            language,
            folder_id: self.selected_folder_id,
            tags: Vec::new(),
            favorite: false,
            created_at: id, // simplified timestamp
            use_count: 0,
            description: String::new(),
            is_template,
            template_vars,
        });

        Some(id)
    }

    fn delete_snippet(&mut self, id: SnippetId) {
        self.snippets.retain(|s| s.id != id);
        if self.selected_snippet_id == Some(id) {
            self.selected_snippet_id = None;
        }
        self.recently_used.retain(|&rid| rid != id);
    }

    /// Add a folder under the selected one, or `None` if there are already
    /// [`MAX_FOLDERS`] of them or the name is empty. See [`App::create_snippet`]
    /// for why this is an `Option` and not a bare id.
    fn create_folder(&mut self, name: &str) -> Option<FolderId> {
        if self.folders.len() >= MAX_FOLDERS || name.is_empty() {
            return None;
        }

        let id = self.id_gen.next_id();
        self.folders.push(Folder {
            id,
            name: name.into(),
            parent_id: self.selected_folder_id,
            expanded: true,
            color: BLUE,
        });
        Some(id)
    }

    fn delete_folder(&mut self, id: FolderId) {
        // Move snippets to root
        for snippet in &mut self.snippets {
            if snippet.folder_id == Some(id) {
                snippet.folder_id = None;
            }
        }
        // Delete child folders
        let child_ids: Vec<FolderId> = self
            .folders
            .iter()
            .filter(|f| f.parent_id == Some(id))
            .map(|f| f.id)
            .collect();
        for child_id in child_ids {
            self.delete_folder(child_id);
        }
        self.folders.retain(|f| f.id != id);
        if self.selected_folder_id == Some(id) {
            self.selected_folder_id = None;
        }
    }

    fn toggle_favorite(&mut self, id: SnippetId) {
        if let Some(snippet) = self.snippets.iter_mut().find(|s| s.id == id) {
            snippet.favorite = !snippet.favorite;
        }
    }

    /// Count a use of a snippet and put it at the head of the recent list.
    ///
    /// An id that is not a snippet's is not recorded. It used to be: the `if
    /// let` that bumps the count quietly found nothing, and the `insert` below
    /// it ran regardless — so an id that was never a snippet took one of the
    /// [`MAX_RECENT`] places and pushed a real entry off the end of the list,
    /// where it was invisible for ever after, since the Recent view only shows
    /// ids that match a snippet.
    /// Using a template makes a filled copy and selects it, so that a
    /// template's Use leaves something behind rather than only counting.
    /// Every variable is filled with its own name in angle brackets — this
    /// program has nowhere to ask for values, and `${name}` left as-is would
    /// be a copy indistinguishable from the template.
    ///
    /// [`apply_template`] and [`Snippet::template_vars`] existed for this and
    /// nothing called either.
    fn use_snippet(&mut self, id: SnippetId) -> EventResult {
        let Some(snippet) = self.snippets.iter_mut().find(|s| s.id == id) else {
            return EventResult::Ignored;
        };
        snippet.use_count = snippet.use_count.saturating_add(1);
        self.recently_used.retain(|&rid| rid != id);
        self.recently_used.insert(0, id);
        self.recently_used.truncate(MAX_RECENT);

        let filled = self.filled_from_template(id);
        if let Some((title, content, language)) = filled
            && let Some(new_id) = self.create_snippet(&title, &content, language)
        {
            self.select(new_id);
        }
        EventResult::Consumed
    }

    /// The title, filled content and language of the copy a template's Use
    /// makes, or `None` if the snippet is not a template.
    fn filled_from_template(&self, id: SnippetId) -> Option<(String, String, Language)> {
        let snippet = self.snippets.iter().find(|s| s.id == id)?;
        if !snippet.is_template {
            return None;
        }
        let values: Vec<(String, String)> = snippet
            .template_vars
            .iter()
            .map(|name| (name.clone(), format!("<{name}>")))
            .collect();
        Some((
            format!("{} (filled)", snippet.title),
            apply_template(&snippet.content, &values),
            snippet.language,
        ))
    }

    fn filtered_snippets(&self) -> Vec<&Snippet> {
        let mut results = search_snippets(&self.snippets, &self.search_query, self.search_scope);

        // Apply sidebar filter. Every view narrows the list — the Tags and
        // Languages arms used to fall into a `_ => {}` that did nothing, so
        // those two sidebars listed things that were not filters.
        match self.sidebar_view {
            SidebarView::Folders => {
                if let Some(fid) = self.selected_folder_id {
                    results.retain(|s| s.folder_id == Some(fid));
                }
            }
            SidebarView::Tags => {
                if let Some(tag) = &self.selected_tag {
                    results.retain(|s| s.tags.iter().any(|t| t == tag));
                }
            }
            SidebarView::Languages => {
                if let Some(lang) = self.selected_language {
                    results.retain(|s| s.language == lang);
                }
            }
            SidebarView::Favorites => {
                results.retain(|s| s.favorite);
            }
            SidebarView::Templates => {
                results.retain(|s| s.is_template);
            }
            SidebarView::Recent => {
                let recent = &self.recently_used;
                results.retain(|s| recent.contains(&s.id));
                // Sort by recency. Returned here rather than falling through:
                // the sort order below is the *list's*, and the Recent view's
                // whole content is an order of its own.
                results.sort_by_key(|s| {
                    recent
                        .iter()
                        .position(|&id| id == s.id)
                        .unwrap_or(usize::MAX)
                });
                return results;
            }
        }

        // Sort
        match self.sort_order {
            SortOrder::NameAsc => results.sort_by(|a, b| a.title.cmp(&b.title)),
            SortOrder::NameDesc => results.sort_by(|a, b| b.title.cmp(&a.title)),
            SortOrder::DateAsc => results.sort_by_key(|s| s.created_at),
            SortOrder::DateDesc => results.sort_by_key(|s| std::cmp::Reverse(s.created_at)),
            SortOrder::UsageDesc => results.sort_by_key(|s| std::cmp::Reverse(s.use_count)),
            SortOrder::LanguageAsc => {
                results.sort_by(|a, b| a.language.name().cmp(b.language.name()));
            }
        }

        results
    }

    fn selected_snippet(&self) -> Option<&Snippet> {
        self.selected_snippet_id
            .and_then(|id| self.snippets.iter().find(|s| s.id == id))
    }

    fn stats(&self) -> LibraryStats {
        compute_stats(&self.snippets, &self.folders)
    }

    /// Every distinct tag in the library and how many snippets carry it, most
    /// used first, capped at [`MAX_TAGS`].
    ///
    /// The cap was declared next to [`MAX_SNIPPETS`] and [`MAX_FOLDERS`] and
    /// then never mentioned again, which `#![allow(dead_code)]` was hiding.
    /// It bites here because [`Target::Tag`] is an index into this list and
    /// the sidebar draws a row per entry: without it, a library with a
    /// hundred thousand distinct tags is a hundred thousand rows to build and
    /// sort on every single frame.
    fn all_tags(&self) -> Vec<(String, usize)> {
        let mut tag_counts: Vec<(String, usize)> = Vec::new();
        for snippet in &self.snippets {
            for tag in &snippet.tags {
                if let Some(entry) = tag_counts.iter_mut().find(|(t, _)| t == tag) {
                    entry.1 = entry.1.saturating_add(1);
                } else {
                    tag_counts.push((tag.clone(), 1));
                }
            }
        }
        // Stable, so tags with equal counts keep the order they were first
        // seen in and the cap below drops the same ones every frame.
        tag_counts.sort_by_key(|&(_, count)| std::cmp::Reverse(count));
        tag_counts.truncate(MAX_TAGS);
        tag_counts
    }

    // ── Geometry the model needs ────────────────────────────────────────

    /// Remember the size the next click will be read against.
    ///
    /// The one piece of geometry the model holds, and it holds it only because
    /// a click arrives with a point and no window.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.size = (width.max(0.0), height.max(0.0));
    }

    /// How the window was last laid out.
    #[must_use]
    pub fn layout(&self) -> Layout {
        Layout::new(self.size.0, self.size.1)
    }

    /// The sort header at the top of the list column.
    #[must_use]
    pub fn list_header(&self, l: &Layout) -> Rect {
        Rect::new(l.list.x, l.list.y, l.list.w, l.row.min(l.list.h))
    }

    /// The part of the list column the rows go in: everything under the sort
    /// header.
    ///
    /// The drawing pass, the wheel and the keyboard all ask this one function,
    /// so none of them can disagree about how many rows are on screen
    /// (`known-issues.md` lesson 63). The list used to answer that question
    /// nowhere at all: it drew every row it had and let the ones past the
    /// bottom fall off the window.
    #[must_use]
    pub fn list_body(&self, l: &Layout) -> Rect {
        let head = self.list_header(l);
        Rect::new(
            l.list.x,
            head.bottom(),
            l.list.w,
            (l.list.bottom() - head.bottom()).max(0.0),
        )
    }

    /// Split the editor column into its header, its code panel and its status
    /// bar. See [`App::list_body`] for why this is one function.
    #[must_use]
    pub fn editor_parts(&self, l: &Layout) -> EditorParts {
        let e = l.editor;
        let header_h = (text::line_height(l.head, FontWeightHint::Bold)
            + text::line_height(l.small, FontWeightHint::Regular) * 2.0
            + l.pad * 3.0)
            .min(e.h);
        let status_h =
            (text::line_height(l.tiny, FontWeightHint::Regular) + l.pad * 2.0).min(e.h - header_h);
        let header = Rect::new(e.x, e.y, e.w, header_h);
        let status = Rect::new(e.x, e.bottom() - status_h, e.w, status_h);
        let code = Rect::new(
            e.x + l.pad,
            header.bottom(),
            (e.w - l.pad * 2.0).max(0.0),
            (status.y - header.bottom()).max(0.0),
        );
        EditorParts {
            header,
            code,
            status,
        }
    }

    /// The ids of the snippets on show, in the order they are shown.
    ///
    /// The list, the keyboard and the scroll bounds all ask this, so none of
    /// them can disagree about which snippet row three is.
    #[must_use]
    pub fn filtered_ids(&self) -> Vec<SnippetId> {
        self.filtered_snippets().iter().map(|s| s.id).collect()
    }

    /// Where the selected snippet sits in the list on show, if it is in it at
    /// all — a snippet can be selected and then filtered out from under the
    /// selection.
    #[must_use]
    pub fn selected_row(&self) -> Option<usize> {
        let id = self.selected_snippet_id?;
        self.filtered_ids().iter().position(|&i| i == id)
    }

    /// How many lines of the selected snippet the code panel can show.
    #[must_use]
    pub fn code_capacity(&self, l: &Layout) -> usize {
        let code = self.editor_parts(l).code;
        scroll_window::capacity(l.line, code.h - l.pad * 2.0)
    }

    // ── Doing things ────────────────────────────────────────────────────

    /// Pick a snippet, and start it at its first line.
    ///
    /// The scroll is reset because it belongs to the panel, not to the
    /// snippet: leaving it where it was showed a new snippet from line forty
    /// — or, since nothing bounded it, from past its end.
    fn select(&mut self, id: SnippetId) {
        self.selected_snippet_id = Some(id);
        self.code_scroll = 0;
    }

    /// Scroll the list so that `row` is on it.
    ///
    /// Without this, arrowing past the bottom of the panel moves a selection
    /// the user cannot see.
    fn scroll_row_into_view(&mut self, row: usize) {
        let l = self.layout();
        let capacity = scroll_window::capacity(l.list_row, self.list_body(&l).h);
        if capacity == 0 {
            // A panel too short for one row has nowhere to bring it into, and
            // the arithmetic below would answer "scroll to it" — which is a
            // scroll position no row is ever drawn at.
            return;
        }
        if row < self.list_scroll {
            self.list_scroll = row;
        } else if row >= self.list_scroll.saturating_add(capacity) {
            self.list_scroll = row.saturating_sub(capacity.saturating_sub(1));
        }
    }

    /// Move the selection `delta` rows down the list on show.
    ///
    /// `.min(last)` is what makes a walk that has reached an end still *do*
    /// something: it re-picks the row already picked, and the
    /// `scroll_row_into_view` below then brings it back on screen. That is the
    /// only case it has — without it the walk names a row past the end,
    /// `ids.get` refuses and the key does nothing at all — and it is invisible
    /// unless the row it re-picks is somewhere the user cannot see
    /// (`known-issues.md` lesson 70).
    ///
    /// There used to be a `checked_sub(1)` early return above this for the
    /// empty list. Nothing could reach it: with no rows, `selected_row` is
    /// `None`, both remaining arms name row 0, and `ids.get(0)` of an empty
    /// list refuses — so it was a guard in front of a rule that already held
    /// (lesson 51), and no test could tell it from its own absence.
    fn move_selection(&mut self, delta: isize) -> EventResult {
        let ids = self.filtered_ids();
        let last = ids.len().saturating_sub(1);
        let next = match self.selected_row() {
            Some(row) => scroll_window::shift(row, delta).min(last),
            // Nothing picked yet: down takes the first row and up the last,
            // which is what makes Up worth pressing before anything is chosen.
            None if delta < 0 => last,
            None => 0,
        };
        let Some(&id) = ids.get(next) else {
            return EventResult::Ignored;
        };
        self.select(id);
        self.scroll_row_into_view(next);
        EventResult::Consumed
    }

    /// Pick the first row of the list on show, or the last.
    ///
    /// Home and End are absolute, not a very large relative move. Routing
    /// them through `move_selection(isize::MIN)` and `isize::MAX` gave the
    /// right answer only while something was already selected: the
    /// no-selection branch there reads the *sign* of the delta and enters
    /// the list from the end a walk would enter it from, so on a fresh
    /// window End landed on the first row and Home on the last — each one
    /// doing the other's job.
    fn select_end(&mut self, last: bool) -> EventResult {
        let ids = self.filtered_ids();
        let Some(bottom) = ids.len().checked_sub(1) else {
            return EventResult::Ignored;
        };
        let row = if last { bottom } else { 0 };
        let Some(&id) = ids.get(row) else {
            return EventResult::Ignored;
        };
        self.select(id);
        self.scroll_row_into_view(row);
        EventResult::Consumed
    }

    /// Show the next of the six sidebar views, or the previous one.
    fn cycle_view(&mut self, back: bool) {
        self.sidebar_view = if back {
            self.sidebar_view.prev()
        } else {
            self.sidebar_view.next()
        };
        self.list_scroll = 0;
    }

    /// Write the library out as JSON and say what happened.
    ///
    /// The button used to be drawn beside an Import button for a feature that
    /// does not exist, and neither of them did anything, because nothing in
    /// the program could receive a click.
    fn export(&mut self) {
        let json = export_snippets_json(&self.snippets);
        let path = self.export_path.clone();
        self.export_note = Some(match std::fs::write(&path, json) {
            Ok(()) => Ok(format!(
                "Exported {} to {}",
                self.snippets.len(),
                show_path(&path)
            )),
            Err(err) => Err(format!("Could not write {}: {err}", show_path(&path))),
        });
    }

    /// Act on a left click that landed on `target`.
    fn press(&mut self, target: Target) -> EventResult {
        match target {
            Target::New => return self.new_snippet(),
            Target::Export => self.export(),
            Target::Stats => self.show_stats = true,
            Target::CloseStats => self.show_stats = false,
            Target::Search => self.search_focus = true,
            Target::ClearSearch => {
                self.search_query.clear();
                self.list_scroll = 0;
            }
            Target::Scope => {
                self.search_scope = self.search_scope.next();
                self.list_scroll = 0;
            }
            Target::Sort => {
                self.sort_order = self.sort_order.next();
                self.list_scroll = 0;
            }
            Target::View(view) => {
                self.sidebar_view = view;
                self.list_scroll = 0;
            }
            Target::Folder(id) => {
                // Clicking the folder that is already picked unpicks it, which
                // is the only way back to "all folders" with a pointer.
                self.selected_folder_id = (self.selected_folder_id != Some(id)).then_some(id);
                self.list_scroll = 0;
            }
            Target::Twisty(id) => {
                if let Some(folder) = self.folders.iter_mut().find(|f| f.id == id) {
                    folder.expanded = !folder.expanded;
                }
            }
            Target::Tag(index) => {
                let tag = self.all_tags().get(index).map(|(t, _)| t.clone());
                self.selected_tag = if self.selected_tag == tag { None } else { tag };
                self.list_scroll = 0;
            }
            Target::Lang(lang) => {
                self.selected_language = (self.selected_language != Some(lang)).then_some(lang);
                self.list_scroll = 0;
            }
            Target::Row(id) => self.select(id),
            Target::Star(id) => self.toggle_favorite(id),
            Target::NewFolder => return self.new_folder(),
            Target::DeleteFolder(id) => self.delete_folder(id),
            Target::Use => {
                let Some(id) = self.selected_snippet_id else {
                    return EventResult::Ignored;
                };
                return self.use_snippet(id);
            }
            Target::Delete => {
                let Some(id) = self.selected_snippet_id else {
                    return EventResult::Ignored;
                };
                self.delete_snippet(id);
            }
            // The panels themselves are hit boxes so the wheel knows which one
            // it is over. A press on one is a press on nothing.
            Target::List | Target::Code => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    /// Make a snippet named after whatever is in the search box.
    ///
    /// The name is the only thing on screen a user can type, so it is the only
    /// thing a "New" that opens no dialog can be named from — and naming it
    /// `Untitled` in a list sorted by name would put every new snippet in the
    /// same place. The language follows from the name, which is what
    /// [`Language::from_extension`] was written for and never called for.
    fn new_snippet(&mut self) -> EventResult {
        let title = self.search_query.trim();
        let title = if title.is_empty() { "Untitled" } else { title }.to_string();
        let language = guess_language(&title, "");
        let Some(id) = self.create_snippet(&title, "", language) else {
            return EventResult::Ignored;
        };
        // Picked so it is the one on screen: a create that leaves the old
        // snippet showing looks like nothing happened.
        self.select(id);
        EventResult::Consumed
    }

    /// Make a folder named after the search box, under the selected folder.
    ///
    /// See [`App::new_snippet`] for why the name comes from there.
    /// [`App::create_folder`] and [`MAX_FOLDERS`] had been written and left
    /// with nothing that could call them.
    fn new_folder(&mut self) -> EventResult {
        let name = self.search_query.trim();
        let name = if name.is_empty() { "Folder" } else { name }.to_string();
        if self.create_folder(&name).is_none() {
            return EventResult::Ignored;
        }
        EventResult::Consumed
    }

    /// Act on a wheel turn over `target`.
    ///
    /// `dy` is in notches, and the accumulator is what keeps the fractions a
    /// trackpad sends from rounding to nothing on every event (`guitk::wheel`).
    fn scroll(&mut self, target: Target, dy: f32) -> EventResult {
        let rows = self.wheel.rows(dy);
        if rows == 0 {
            // A fraction of a notch that has not yet added up to a row has
            // been banked, not lost — but nothing moved, so nothing is redrawn.
            return EventResult::Ignored;
        }
        let l = self.layout();
        match target {
            Target::List | Target::Row(_) | Target::Star(_) => {
                let total = self.filtered_ids().len();
                let capacity = scroll_window::capacity(l.list_row, self.list_body(&l).h);
                self.list_scroll = clamp_scroll(self.list_scroll, rows, total, capacity);
            }
            Target::Code => {
                let total = self
                    .selected_snippet()
                    .map_or(0, |s| s.content.lines().count());
                self.code_scroll =
                    clamp_scroll(self.code_scroll, rows, total, self.code_capacity(&l));
            }
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    /// Act on a keystroke.
    pub fn handle_key(&mut self, ev: &KeyEvent) -> EventResult {
        if !ev.pressed {
            return EventResult::Ignored;
        }
        // The overlay is modal. A key that reached the library behind it would
        // change a list the user cannot see, and would leave the numbers on
        // the overlay describing a library that had moved on.
        if self.show_stats {
            return match ev.key {
                Key::Escape | Key::Enter | Key::S => {
                    self.show_stats = false;
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            };
        }
        if self.search_focus {
            return self.handle_search_key(ev);
        }
        match ev.key {
            Key::Slash => {
                self.search_focus = true;
                EventResult::Consumed
            }
            Key::F if ev.modifiers.ctrl => {
                self.search_focus = true;
                EventResult::Consumed
            }
            Key::Up => self.move_selection(-1),
            Key::Down => self.move_selection(1),
            Key::PageUp => self.move_selection(self.page().saturating_neg()),
            Key::PageDown => self.move_selection(self.page()),
            Key::Home => self.select_end(false),
            Key::End => self.select_end(true),
            Key::Tab => {
                self.cycle_view(ev.modifiers.shift);
                EventResult::Consumed
            }
            Key::Enter => self.press(Target::Use),
            Key::Delete => self.press(Target::Delete),
            Key::F => {
                let Some(id) = self.selected_snippet_id else {
                    return EventResult::Ignored;
                };
                self.toggle_favorite(id);
                EventResult::Consumed
            }
            Key::S => self.press(Target::Stats),
            Key::N => self.press(Target::New),
            Key::E => self.press(Target::Export),
            Key::O => self.press(Target::Sort),
            Key::Escape => {
                if self.search_query.is_empty() {
                    return EventResult::Ignored;
                }
                self.press(Target::ClearSearch)
            }
            _ => EventResult::Ignored,
        }
    }

    /// A keystroke while the search box has the keyboard.
    ///
    /// A letter means "find this" here and "do this" outside, which is the
    /// only thing that lets one keyboard serve both a text field and a set of
    /// single-letter shortcuts.
    fn handle_search_key(&mut self, ev: &KeyEvent) -> EventResult {
        match ev.key {
            Key::Escape | Key::Enter => {
                self.search_focus = false;
                EventResult::Consumed
            }
            Key::Backspace => {
                if self.search_query.pop().is_none() {
                    return EventResult::Ignored;
                }
                self.list_scroll = 0;
                EventResult::Consumed
            }
            // Arrows still move the selection while typing: a search you
            // cannot walk the results of is half a search.
            Key::Up => self.move_selection(-1),
            Key::Down => self.move_selection(1),
            _ => {
                if !ev.types_text() {
                    return EventResult::Ignored;
                }
                // `typed`, not `text`: on most layouts Enter, Tab and Escape
                // all produce text, and a field that appends whatever arrives
                // fills up with control characters.
                self.search_query.extend(ev.typed());
                self.list_scroll = 0;
                EventResult::Consumed
            }
        }
    }

    /// How many rows a page key moves: one panel's worth, less a row so the
    /// reader keeps a line of context across the jump.
    fn page(&self) -> isize {
        let l = self.layout();
        let capacity = scroll_window::capacity(l.list_row, self.list_body(&l).h);
        isize::try_from(capacity.saturating_sub(1).max(1)).unwrap_or(1)
    }

    /// Act on a pointer event.
    pub fn handle_mouse(&mut self, ev: &MouseEvent) -> EventResult {
        let frame = self.frame(self.size.0, self.size.1);
        let Some(target) = frame.hit_test(ev.x, ev.y) else {
            return EventResult::Ignored;
        };
        match ev.kind {
            MouseEventKind::Press(MouseButton::Left) => self.press(target),
            MouseEventKind::Scroll { dy, .. } => self.scroll(target, dy),
            _ => EventResult::Ignored,
        }
    }

    // ── Drawing ─────────────────────────────────────────────────────────

    /// Draw one frame, recording where every control went as it goes.
    ///
    /// Everything is solved from `width` and `height`, which is the whole
    /// difference from the program this replaces: that one drew against
    /// `WINDOW_WIDTH`, `SIDEBAR_WIDTH`, `LIST_WIDTH` and `TOOLBAR_HEIGHT`
    /// whatever window it was in, so a narrower window did not narrow the
    /// columns, it cut the editor off the end.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame<Target> {
        let mut f = Frame::new(width, height);
        let l = Layout::new(width, height);
        fill(&mut f, l.window, BASE, CornerRadii::ZERO);
        self.draw_toolbar(&mut f, &l);
        self.draw_sidebar(&mut f, &l);
        self.draw_list(&mut f, &l);
        self.draw_editor(&mut f, &l);
        if self.show_stats {
            self.draw_stats(&mut f, &l);
        }
        f
    }

    fn draw_toolbar(&self, f: &mut Frame<Target>, l: &Layout) {
        let bar = l.toolbar;
        fill(f, bar, CRUST, CornerRadii::ZERO);
        f.clip(bar);
        let mut rest = inset_x(bar, l.pad);

        // Right to left first, so that what is measured from the right edge is
        // taken out of the row before the left-hand items are asked what they
        // can have. The row this replaces did the opposite — buttons from a
        // hardcoded `bx = 180.0` and a search box from `WINDOW_WIDTH - 320.0`
        // — and nothing anywhere checked that the first stopped before the
        // second.
        let count = self.snippets.len().to_string();
        let count_w = text::measure(&count, l.small, FontWeightHint::Regular);
        let count_rect = take_right(&mut rest, count_w, l.pad);
        label_left(
            f,
            &Label {
                text: &count,
                size: l.small,
                weight: FontWeightHint::Regular,
                color: SUBTEXT0,
            },
            count_rect,
        );

        let search_w = (rest.w * SEARCH_SHARE).min(SEARCH_MAX);
        let search = take_right(&mut rest, search_w, l.pad);
        self.draw_search(f, l, search);

        // Then left to right. Each item takes what it measures or nothing at
        // all, so a toolbar that runs out of room loses whole controls from
        // the middle rather than drawing them on top of one another.
        let title_w = text::measure(TOOLBAR_TITLE, l.title, FontWeightHint::Bold);
        let title_rect = take_left(&mut rest, title_w, l.pad * 2.0);
        label_left(
            f,
            &Label {
                text: TOOLBAR_TITLE,
                size: l.title,
                weight: FontWeightHint::Bold,
                color: TEXT,
            },
            title_rect,
        );

        for (label, color, target) in [
            ("+ New", BLUE, Target::New),
            ("Export", GREEN, Target::Export),
            ("Stats", MAUVE, Target::Stats),
        ] {
            // Drawn bold, so measured bold: the old estimate sized "Import"
            // from its regular-weight guess and let the bold label touch the
            // edge of its own button.
            let want = text::padded_width(label, l.pad * 2.0, l.small, FontWeightHint::Bold);
            let button = inset_y(take_left(&mut rest, want, l.pad), l.pad * 0.5);
            fill(f, button, color, CornerRadii::all(l.pad * 0.5));
            label_centred(
                f,
                &Label {
                    text: label,
                    size: l.small,
                    weight: FontWeightHint::Bold,
                    color: CRUST,
                },
                button,
            );
            f.hit(target, button);
        }
        f.unclip();
    }

    fn draw_search(&self, f: &mut Frame<Target>, l: &Layout, r: Rect) {
        let outer = inset_y(r, l.pad * 0.5);
        if outer.is_empty() {
            return;
        }
        let round = CornerRadii::all(outer.h / 2.0);
        fill(f, outer, SURFACE0, round);
        // Recorded before the two controls inside it, because a hit test takes
        // the last match and these are drawn on top of the box.
        f.hit(Target::Search, outer);
        if self.search_focus {
            // The one thing that says a letter will be typed rather than acted
            // on.
            stroke(f, outer, BLUE, 1.0, round);
        }

        let mut inner = inset_x(outer, outer.h / 2.0);
        if !self.search_query.is_empty() {
            let cross = take_right(
                &mut inner,
                text::measure(CLEAR_MARK, l.small, FontWeightHint::Bold),
                l.pad,
            );
            label_centred(
                f,
                &Label {
                    text: CLEAR_MARK,
                    size: l.small,
                    weight: FontWeightHint::Bold,
                    color: SUBTEXT0,
                },
                cross,
            );
            f.hit(Target::ClearSearch, cross);
        }

        let scope = self.search_scope.label();
        let scope_rect = take_right(
            &mut inner,
            text::measure(scope, l.tiny, FontWeightHint::Bold),
            l.pad,
        );
        label_centred(
            f,
            &Label {
                text: scope,
                size: l.tiny,
                weight: FontWeightHint::Bold,
                color: LAVENDER,
            },
            scope_rect,
        );
        f.hit(Target::Scope, scope_rect);

        let (shown, color) = if self.search_query.is_empty() {
            (SEARCH_PLACEHOLDER, OVERLAY0)
        } else {
            (self.search_query.as_str(), TEXT)
        };
        label_left(
            f,
            &Label {
                text: shown,
                size: l.small,
                weight: FontWeightHint::Regular,
                color,
            },
            inner,
        );
    }

    fn draw_sidebar(&self, f: &mut Frame<Target>, l: &Layout) {
        let side = l.sidebar;
        if side.is_empty() {
            return;
        }
        fill(f, side, MANTLE, CornerRadii::ZERO);
        f.clip(side);

        // One icon column, as wide as the widest icon. They used to be drawn
        // at their own widths against a label column fixed at x = 44, so the
        // labels lined up only because nobody had measured them.
        let icon_w = SidebarView::ALL
            .iter()
            .map(|v| text::measure(v.icon(), l.small, FontWeightHint::Regular))
            .fold(0.0_f32, f32::max);

        let mut y = side.y + l.pad;
        for view in SidebarView::ALL {
            let r = Rect::new(side.x + l.pad, y, (side.w - l.pad * 2.0).max(0.0), l.row);
            if r.bottom() > side.bottom() {
                break;
            }
            let selected = view == self.sidebar_view;
            if selected {
                fill(f, r, SURFACE0, CornerRadii::all(l.pad * 0.5));
            }
            let mut rest = inset_x(r, l.pad * 0.5);
            let icon = take_left(&mut rest, icon_w, l.pad);
            label_left(
                f,
                &Label {
                    text: view.icon(),
                    size: l.small,
                    weight: FontWeightHint::Regular,
                    color: if selected { BLUE } else { OVERLAY0 },
                },
                icon,
            );
            label_left(
                f,
                &Label {
                    text: view.label(),
                    size: l.font,
                    weight: if selected {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                    color: if selected { TEXT } else { SUBTEXT0 },
                },
                rest,
            );
            f.hit(Target::View(view), r);
            y += l.row;
        }

        let sep = Rect::new(
            side.x + l.pad,
            y + l.pad * 0.5,
            (side.w - l.pad * 2.0).max(0.0),
            1.0,
        );
        fill(f, sep, SURFACE1, CornerRadii::ZERO);

        let items = Rect::new(
            side.x,
            sep.bottom() + l.pad,
            side.w,
            (side.bottom() - sep.bottom() - l.pad).max(0.0),
        );
        match self.sidebar_view {
            SidebarView::Folders => self.draw_folder_tree(f, l, items),
            SidebarView::Tags => self.draw_tag_list(f, l, items),
            SidebarView::Languages => self.draw_language_list(f, l, items),
            // These three are filters in themselves — there is nothing under
            // them to pick — so the space says what is being shown instead of
            // sitting blank.
            SidebarView::Favorites | SidebarView::Recent | SidebarView::Templates => {
                let count = self.filtered_ids().len();
                let note = format!("{count} shown");
                label_left(
                    f,
                    &Label {
                        text: &note,
                        size: l.small,
                        weight: FontWeightHint::Regular,
                        color: OVERLAY0,
                    },
                    Rect::new(items.x + l.pad * 2.0, items.y, items.w, l.row),
                );
            }
        }
        f.unclip();
    }

    fn draw_folder_tree(&self, f: &mut Frame<Target>, l: &Layout, area: Rect) {
        let rows = self.folder_rows();
        let visible = scroll_window::visible(rows.len(), l.row, area.h, 0);
        for (offset, &(id, depth)) in rows
            .iter()
            .skip(visible.start)
            .take(visible.count)
            .enumerate()
        {
            let Some(folder) = self.folders.iter().find(|folder| folder.id == id) else {
                continue;
            };
            let r = Rect::new(
                area.x + l.pad,
                area.y + f32_from_usize(offset) * l.row,
                (area.w - l.pad * 2.0).max(0.0),
                l.row,
            );
            let selected = self.selected_folder_id == Some(id);
            if selected {
                fill(f, r, SURFACE0, CornerRadii::all(l.pad * 0.5));
            }
            f.hit(Target::Folder(id), r);

            let indent = f32_from_usize(depth) * l.pad * 2.0;
            let mut rest = Rect::new(r.x + indent, r.y, (r.w - indent).max(0.0), r.h);

            // The triangle is drawn for every folder and clickable only where
            // there is something to open, so the names below one another line
            // up whether or not a folder has children.
            let twisty = take_left(
                &mut rest,
                text::measure(TWISTY_OPEN, l.tiny, FontWeightHint::Bold),
                l.pad * 0.5,
            );
            if self.has_children(id) {
                label_centred(
                    f,
                    &Label {
                        text: if folder.expanded {
                            TWISTY_OPEN
                        } else {
                            TWISTY_SHUT
                        },
                        size: l.tiny,
                        weight: FontWeightHint::Bold,
                        color: SUBTEXT0,
                    },
                    twisty,
                );
                f.hit(Target::Twisty(id), twisty);
            }

            // Only the selected folder carries a cross, so no single mis-aimed
            // click in a tree can delete one: it takes a click to select and
            // then a second click on the cross.
            if selected {
                let cross = take_right(
                    &mut rest,
                    text::measure(CLEAR_MARK, l.small, FontWeightHint::Bold),
                    l.pad,
                );
                label_centred(
                    f,
                    &Label {
                        text: CLEAR_MARK,
                        size: l.small,
                        weight: FontWeightHint::Bold,
                        color: RED,
                    },
                    cross,
                );
                f.hit(Target::DeleteFolder(id), cross);
            }

            let count = self
                .snippets
                .iter()
                .filter(|s| s.folder_id == Some(id))
                .count()
                .to_string();
            let count_rect = take_right(
                &mut rest,
                text::measure(&count, l.tiny, FontWeightHint::Regular),
                l.pad,
            );
            label_left(
                f,
                &Label {
                    text: &count,
                    size: l.tiny,
                    weight: FontWeightHint::Regular,
                    color: OVERLAY0,
                },
                count_rect,
            );

            let dot_side = l.tiny * 0.7;
            let dot_slot = take_left(&mut rest, dot_side, l.pad * 0.5);
            fill(
                f,
                Rect::new(
                    dot_slot.x,
                    dot_slot.y + (dot_slot.h - dot_side) / 2.0,
                    dot_side,
                    dot_side,
                ),
                folder.color,
                CornerRadii::all(dot_side / 2.0),
            );
            label_left(
                f,
                &Label {
                    text: &folder.name,
                    size: l.small,
                    weight: FontWeightHint::Regular,
                    color: if selected { TEXT } else { SUBTEXT0 },
                },
                rest,
            );
        }

        // The one way to make a folder. It sits after the last row that was
        // drawn — not after the last row that exists — so it is never below
        // the panel it belongs to.
        let below = Rect::new(
            area.x + l.pad,
            area.y + f32_from_usize(visible.count) * l.row,
            (area.w - l.pad * 2.0).max(0.0),
            l.row,
        );
        let new_row = Rect::new(
            below.x,
            below.y,
            below.w,
            below.h.min((area.bottom() - below.y).max(0.0)),
        );
        label_left(
            f,
            &Label {
                text: NEW_FOLDER_LABEL,
                size: l.small,
                weight: FontWeightHint::Bold,
                color: BLUE,
            },
            inset_x(new_row, l.pad),
        );
        f.hit(Target::NewFolder, new_row);
    }

    fn draw_tag_list(&self, f: &mut Frame<Target>, l: &Layout, area: Rect) {
        let tags = self.all_tags();
        let visible = scroll_window::visible(tags.len(), l.row, area.h, 0);
        for (offset, (tag, count)) in tags
            .iter()
            .skip(visible.start)
            .take(visible.count)
            .enumerate()
        {
            let index = visible.start.saturating_add(offset);
            let r = Rect::new(
                area.x + l.pad,
                area.y + f32_from_usize(offset) * l.row,
                (area.w - l.pad * 2.0).max(0.0),
                l.row,
            );
            let selected = self.selected_tag.as_deref() == Some(tag.as_str());
            if selected {
                fill(f, r, SURFACE0, CornerRadii::all(l.pad * 0.5));
            }
            f.hit(Target::Tag(index), r);

            let mut rest = inset_x(r, l.pad);
            let shown = count.to_string();
            let count_rect = take_right(
                &mut rest,
                text::measure(&shown, l.tiny, FontWeightHint::Regular),
                l.pad,
            );
            label_left(
                f,
                &Label {
                    text: &shown,
                    size: l.tiny,
                    weight: FontWeightHint::Regular,
                    color: OVERLAY0,
                },
                count_rect,
            );
            // The `#` is drawn, so it is measured and elided with the name.
            let label = format!("#{tag}");
            label_left(
                f,
                &Label {
                    text: &label,
                    size: l.small,
                    weight: FontWeightHint::Regular,
                    color: if selected { TEXT } else { TEAL },
                },
                rest,
            );
        }
    }

    fn draw_language_list(&self, f: &mut Frame<Target>, l: &Layout, area: Rect) {
        // Only the languages that have snippets, gathered *before* anything is
        // positioned. The list used to place language `i` of twelve at row `i`
        // and then skip the empty ones, so three languages in use were drawn
        // at rows 0, 4 and 9 with blank rows between them.
        let used = self.languages_in_use();
        let visible = scroll_window::visible(used.len(), l.row, area.h, 0);
        for (offset, &(lang, count)) in used
            .iter()
            .skip(visible.start)
            .take(visible.count)
            .enumerate()
        {
            let r = Rect::new(
                area.x + l.pad,
                area.y + f32_from_usize(offset) * l.row,
                (area.w - l.pad * 2.0).max(0.0),
                l.row,
            );
            let selected = self.selected_language == Some(lang);
            if selected {
                fill(f, r, SURFACE0, CornerRadii::all(l.pad * 0.5));
            }
            f.hit(Target::Lang(lang), r);

            let mut rest = inset_x(r, l.pad);
            let shown = count.to_string();
            let count_rect = take_right(
                &mut rest,
                text::measure(&shown, l.tiny, FontWeightHint::Regular),
                l.pad,
            );
            label_left(
                f,
                &Label {
                    text: &shown,
                    size: l.tiny,
                    weight: FontWeightHint::Regular,
                    color: OVERLAY0,
                },
                count_rect,
            );

            let dot_side = l.tiny * 0.7;
            let dot_slot = take_left(&mut rest, dot_side, l.pad * 0.5);
            fill(
                f,
                Rect::new(
                    dot_slot.x,
                    dot_slot.y + (dot_slot.h - dot_side) / 2.0,
                    dot_side,
                    dot_side,
                ),
                lang.color(),
                CornerRadii::all(dot_side / 2.0),
            );
            label_left(
                f,
                &Label {
                    text: lang.name(),
                    size: l.small,
                    weight: FontWeightHint::Regular,
                    color: if selected { TEXT } else { SUBTEXT0 },
                },
                rest,
            );
        }
    }

    fn draw_list(&self, f: &mut Frame<Target>, l: &Layout) {
        let col = l.list;
        if col.is_empty() {
            return;
        }
        fill(f, col, BASE, CornerRadii::ZERO);
        fill(
            f,
            Rect::new(col.x, col.y, 1.0, col.h),
            SURFACE0,
            CornerRadii::ZERO,
        );

        let head = self.list_header(l);
        fill(f, head, CRUST, CornerRadii::ZERO);
        let sort = format!("Sort: {}", self.sort_order.label());
        label_left(
            f,
            &Label {
                text: &sort,
                size: l.small,
                weight: FontWeightHint::Regular,
                color: SUBTEXT0,
            },
            inset_x(head, l.pad),
        );
        f.hit(Target::Sort, head);

        let body = self.list_body(l);
        // Clipped to the body, so a row scrolled half off the top is cut at
        // the header rather than drawn over it — which is what the old
        // visibility test allowed, skipping a row only once it was a whole row
        // above the top and never clipping anything.
        f.clip(body);
        // Recorded before the rows, because a hit test takes the last match:
        // the panel is only here to tell the wheel which of the two scrollable
        // things it is over, and must not swallow the rows on top of it.
        f.hit(Target::List, body);

        let filtered = self.filtered_snippets();
        let rows = scroll_window::visible(filtered.len(), l.list_row, body.h, self.list_scroll);
        for (offset, snippet) in filtered
            .iter()
            .skip(rows.start)
            .take(rows.count)
            .enumerate()
        {
            self.draw_list_row(
                f,
                l,
                snippet,
                Rect::new(
                    body.x,
                    body.y + f32_from_usize(offset) * l.list_row,
                    body.w,
                    l.list_row,
                ),
            );
        }

        if filtered.is_empty() {
            label_centred(
                f,
                &Label {
                    text: EMPTY_LIST,
                    size: l.font,
                    weight: FontWeightHint::Regular,
                    color: OVERLAY0,
                },
                Rect::new(body.x, body.y, body.w, l.list_row),
            );
        }
        f.unclip();
    }

    fn draw_list_row(&self, f: &mut Frame<Target>, l: &Layout, s: &Snippet, r: Rect) {
        let selected = self.selected_snippet_id == Some(s.id);
        let card = shrink(r, l.pad * 0.5);
        if selected {
            fill(f, card, SURFACE0, CornerRadii::all(l.pad * 0.5));
        }
        f.hit(Target::Row(s.id), r);

        let inner = inset_x(card, l.pad);
        let badge_h = text::line_height(l.tiny, FontWeightHint::Bold);
        let mut top = Rect::new(inner.x, inner.y + l.pad * 0.5, inner.w, badge_h);

        // The star is drawn whether or not the snippet is a favourite, dim
        // when it is not — a control you can only see once you have used it is
        // one nobody finds. It used to be drawn only for favourites, which was
        // harmless while nothing could be clicked and is not now.
        let star = take_right(
            &mut top,
            text::measure(STAR, l.font, FontWeightHint::Bold),
            l.pad * 0.5,
        );
        label_centred(
            f,
            &Label {
                text: STAR,
                size: l.font,
                weight: FontWeightHint::Bold,
                color: if s.favorite { YELLOW } else { SURFACE1 },
            },
            star,
        );
        f.hit(Target::Star(s.id), star);

        let name = s.language.name();
        let badge = Rect::new(
            top.x,
            top.y,
            text::padded_width(name, l.pad, l.tiny, FontWeightHint::Bold).min(top.w),
            badge_h,
        );
        fill(
            f,
            badge,
            s.language.color(),
            CornerRadii::all(badge_h / 2.0),
        );
        label_centred(
            f,
            &Label {
                text: name,
                size: l.tiny,
                weight: FontWeightHint::Bold,
                color: CRUST,
            },
            badge,
        );

        let title_h = text::line_height(l.font, FontWeightHint::Bold);
        let title = Rect::new(inner.x, top.bottom(), inner.w, title_h);
        label_left(
            f,
            &Label {
                text: &s.title,
                size: l.font,
                weight: FontWeightHint::Bold,
                color: if selected { TEXT } else { SUBTEXT1 },
            },
            title,
        );

        if !s.tags.is_empty() {
            let shown: String = s
                .tags
                .iter()
                .take(TAGS_ON_A_ROW)
                .map(|t| format!("#{t}"))
                .collect::<Vec<_>>()
                .join(" ");
            label_left(
                f,
                &Label {
                    text: &shown,
                    size: l.tiny,
                    weight: FontWeightHint::Regular,
                    color: OVERLAY0,
                },
                Rect::new(
                    inner.x,
                    title.bottom(),
                    inner.w,
                    text::line_height(l.tiny, FontWeightHint::Regular),
                ),
            );
        }
    }

    fn draw_editor(&self, f: &mut Frame<Target>, l: &Layout) {
        let col = l.editor;
        if col.is_empty() {
            return;
        }
        fill(f, col, MANTLE, CornerRadii::ZERO);
        fill(
            f,
            Rect::new(col.x, col.y, 1.0, col.h),
            SURFACE0,
            CornerRadii::ZERO,
        );
        let parts = self.editor_parts(l);
        f.clip(col);
        if let Some(s) = self.selected_snippet() {
            self.draw_editor_header(f, l, s, parts.header);
            self.draw_code(f, l, s, parts.code);
        } else {
            let headline = Rect::new(
                col.x,
                col.y + col.h / 2.0 - l.head,
                col.w,
                text::line_height(l.head, FontWeightHint::Regular),
            );
            label_centred(
                f,
                &Label {
                    text: EMPTY_HEADLINE,
                    size: l.head,
                    weight: FontWeightHint::Regular,
                    color: OVERLAY0,
                },
                headline,
            );
            label_centred(
                f,
                &Label {
                    text: EMPTY_SUBLINE,
                    size: l.font,
                    weight: FontWeightHint::Regular,
                    color: OVERLAY0,
                },
                Rect::new(
                    col.x,
                    headline.bottom() + l.pad,
                    col.w,
                    text::line_height(l.font, FontWeightHint::Regular),
                ),
            );
        }
        self.draw_status(f, l, parts.status);
        f.unclip();
    }

    fn draw_editor_header(&self, f: &mut Frame<Target>, l: &Layout, s: &Snippet, area: Rect) {
        fill(f, area, CRUST, CornerRadii::ZERO);
        let mut top = Rect::new(
            area.x + l.pad,
            area.y + l.pad,
            (area.w - l.pad * 2.0).max(0.0),
            text::line_height(l.head, FontWeightHint::Bold),
        );

        for (label, color, target) in [
            (DELETE_LABEL, RED, Target::Delete),
            (USE_LABEL, BLUE, Target::Use),
        ] {
            let want = text::padded_width(label, l.pad * 2.0, l.tiny, FontWeightHint::Bold);
            let button = inset_y(take_right(&mut top, want, l.pad), l.pad * 0.25);
            fill(f, button, color, CornerRadii::all(l.pad * 0.5));
            label_centred(
                f,
                &Label {
                    text: label,
                    size: l.tiny,
                    weight: FontWeightHint::Bold,
                    color: CRUST,
                },
                button,
            );
            f.hit(target, button);
        }

        if s.is_template {
            let pill = inset_y(
                take_right(
                    &mut top,
                    text::padded_width(TEMPLATE_LABEL, l.pad * 2.0, l.tiny, FontWeightHint::Bold),
                    l.pad,
                ),
                l.pad * 0.25,
            );
            fill(f, pill, YELLOW, CornerRadii::all(pill.h / 2.0));
            label_centred(
                f,
                &Label {
                    text: TEMPLATE_LABEL,
                    size: l.tiny,
                    weight: FontWeightHint::Bold,
                    color: CRUST,
                },
                pill,
            );
        }

        label_left(
            f,
            &Label {
                text: &s.title,
                size: l.head,
                weight: FontWeightHint::Bold,
                color: TEXT,
            },
            top,
        );

        let mut second = Rect::new(
            area.x + l.pad,
            top.bottom(),
            (area.w - l.pad * 2.0).max(0.0),
            text::line_height(l.small, FontWeightHint::Regular),
        );
        // The extension is shown beside the name because it is what a new
        // snippet's language is *guessed from* (see `guess_language`), so a
        // user who wants a different language has to be able to see which
        // suffix would have got it. `Language::extension` had been written and
        // never called.
        let lang = format!("{} .{}", s.language.name(), s.language.extension());
        let lang_rect = take_left(
            &mut second,
            text::measure(&lang, l.small, FontWeightHint::Bold),
            l.pad * 2.0,
        );
        label_left(
            f,
            &Label {
                text: &lang,
                size: l.small,
                weight: FontWeightHint::Bold,
                color: s.language.color(),
            },
            lang_rect,
        );
        let used = format!("Used {} times", s.use_count);
        label_left(
            f,
            &Label {
                text: &used,
                size: l.small,
                weight: FontWeightHint::Regular,
                color: OVERLAY0,
            },
            second,
        );

        if !s.description.is_empty() {
            label_left(
                f,
                &Label {
                    text: &s.description,
                    size: l.small,
                    weight: FontWeightHint::Regular,
                    color: SUBTEXT0,
                },
                Rect::new(
                    area.x + l.pad,
                    second.bottom(),
                    (area.w - l.pad * 2.0).max(0.0),
                    text::line_height(l.small, FontWeightHint::Regular),
                ),
            );
        }
    }

    fn draw_code(&self, f: &mut Frame<Target>, l: &Layout, s: &Snippet, area: Rect) {
        if area.is_empty() {
            return;
        }
        fill(f, area, BASE, CornerRadii::all(l.pad * 0.6));
        f.clip(area);
        f.hit(Target::Code, area);

        let lines = tokenize(&s.content, s.language);
        let inner = shrink(area, l.pad);
        let rows = scroll_window::visible(lines.len(), l.line, inner.h, self.code_scroll);
        let gutter = text::measure_in(
            GUTTER_WIDEST,
            l.small,
            FontWeightHint::Regular,
            FontFamily::Mono,
        );

        // The code area only. Its pen advances by what each token measures, so
        // the tokens have to be drawn in the face that measurement came from —
        // otherwise consecutive tokens on a line overlap or leave gaps, and
        // indentation stops lining up between rows, which is the whole reason
        // this panel is a grid. The header, tag pills and status bar around it
        // are proportional chrome and stay outside.
        f.push(RenderCommand::PushFont {
            family: FontFamily::Mono,
        });
        for (offset, tokens) in lines.iter().skip(rows.start).take(rows.count).enumerate() {
            let y = inner.y + f32_from_usize(offset) * l.line;
            let number = rows
                .start
                .saturating_add(offset)
                .saturating_add(1)
                .to_string();
            push_text(
                f,
                &Label {
                    text: &number,
                    size: l.small,
                    weight: FontWeightHint::Regular,
                    color: OVERLAY0,
                },
                inner.x,
                y,
                gutter,
            );
            let mut pen = inner.x + gutter + l.pad;
            for token in tokens {
                let weight = if token.kind == TokenKind::Keyword {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                };
                // The pen advances by what this token will actually be drawn
                // as — same string, same size, same weight, same family —
                // rather than by a nominal cell count. A count is only equal to
                // the drawn width where every character advances the same
                // distance, which is true of Latin text in a mono face and not
                // true of the text a snippet can hold: a CJK ideograph is two
                // cells, a combining accent is none, and a character the face
                // lacks advances by whatever `.notdef` happens to be. Where
                // they disagreed the next token overlapped this one or left a
                // gap, and indentation stopped lining up between rows.
                let advance = text::measure_in(&token.text, l.font, weight, FontFamily::Mono);
                push_text(
                    f,
                    &Label {
                        text: &token.text,
                        size: l.font,
                        weight,
                        color: token.kind.color(),
                    },
                    pen,
                    y,
                    inner.right() - pen,
                );
                pen += advance;
            }
        }
        f.push(RenderCommand::PopFont);
        f.unclip();
    }

    fn draw_status(&self, f: &mut Frame<Target>, l: &Layout, area: Rect) {
        if area.is_empty() {
            return;
        }
        fill(f, area, CRUST, CornerRadii::ZERO);
        let mut rest = inset_x(area, l.pad);

        if let Some(s) = self.selected_snippet() {
            let lines = format!("{} lines", s.content.lines().count());
            let rect = take_right(
                &mut rest,
                text::measure(&lines, l.tiny, FontWeightHint::Regular),
                l.pad,
            );
            label_left(
                f,
                &Label {
                    text: &lines,
                    size: l.tiny,
                    weight: FontWeightHint::Regular,
                    color: OVERLAY0,
                },
                rect,
            );
        }

        // What the last export did takes the line while there is something to
        // say, because it is news and the tags are not.
        if let Some(note) = &self.export_note {
            let (message, color) = match note {
                Ok(message) => (message, GREEN),
                Err(message) => (message, RED),
            };
            label_left(
                f,
                &Label {
                    text: message,
                    size: l.tiny,
                    weight: FontWeightHint::Regular,
                    color,
                },
                rest,
            );
            return;
        }

        let Some(s) = self.selected_snippet() else {
            return;
        };
        for tag in &s.tags {
            // The `#` is drawn, so it is measured: the old estimate sized the
            // pill from the bare tag and let the last character sit on the
            // rounded edge.
            let label = format!("#{tag}");
            let want = text::padded_width(&label, l.pad * 2.0, l.tiny, FontWeightHint::Regular);
            let pill = inset_y(take_left(&mut rest, want, l.pad * 0.5), l.pad * 0.25);
            fill(f, pill, SURFACE0, CornerRadii::all(pill.h / 2.0));
            label_centred(
                f,
                &Label {
                    text: &label,
                    size: l.tiny,
                    weight: FontWeightHint::Regular,
                    color: TEAL,
                },
                pill,
            );
        }
    }

    fn draw_stats(&self, f: &mut Frame<Target>, l: &Layout) {
        // The backdrop is the dismiss control, so a click anywhere outside the
        // dialog shuts it. Recorded first; the dialog draws over it and takes
        // nothing, so a click inside stays inside.
        fill(f, l.window, Color::rgba(0, 0, 0, 128), CornerRadii::ZERO);
        f.hit(Target::CloseStats, l.window);

        let stats = self.stats();
        let rows = self.stat_rows(&stats);
        let languages: Vec<&(Language, usize)> = stats
            .by_language
            .iter()
            .take(LANGUAGES_ON_OVERLAY)
            .collect();

        // Sized from what it holds and then held to the window, rather than the
        // 400x300 it used to be whatever was in it and whatever it was in.
        let line = text::line_height(l.font, FontWeightHint::Bold) + l.pad * 0.5;
        let wanted_h = l.pad * 2.0
            + text::line_height(l.head, FontWeightHint::Bold)
            + line * f32_from_usize(rows.len().saturating_add(languages.len()).saturating_add(1))
            + l.pad * 2.0;
        let name_w = rows
            .iter()
            .map(|(name, _)| text::measure(name, l.font, FontWeightHint::Regular))
            .fold(0.0_f32, f32::max);
        let value_w = rows
            .iter()
            .map(|(_, value)| text::measure(value, l.font, FontWeightHint::Bold))
            .fold(0.0_f32, f32::max);
        let wanted_w = name_w + value_w + l.pad * 6.0;

        let w = wanted_w.min(l.window.w * OVERLAY_SHARE);
        let h = wanted_h.min(l.window.h * OVERLAY_SHARE);
        let dialog = Rect::new(
            l.window.x + (l.window.w - w) / 2.0,
            l.window.y + (l.window.h - h) / 2.0,
            w,
            h,
        );
        fill(f, dialog, MANTLE, CornerRadii::all(l.pad));
        f.clip(dialog);

        let inner = shrink(dialog, l.pad * 2.0);
        let mut y = inner.y;
        let head_h = text::line_height(l.head, FontWeightHint::Bold);
        label_left(
            f,
            &Label {
                text: STATS_TITLE,
                size: l.head,
                weight: FontWeightHint::Bold,
                color: BLUE,
            },
            Rect::new(inner.x, y, inner.w, head_h),
        );
        y += head_h + l.pad;

        for (name, value) in &rows {
            let row = Rect::new(inner.x, y, inner.w, line);
            label_left(
                f,
                &Label {
                    text: name,
                    size: l.font,
                    weight: FontWeightHint::Regular,
                    color: SUBTEXT0,
                },
                Rect::new(row.x, row.y, name_w, row.h),
            );
            label_left(
                f,
                &Label {
                    text: value,
                    size: l.font,
                    weight: FontWeightHint::Bold,
                    color: TEXT,
                },
                Rect::new(row.x + name_w + l.pad * 2.0, row.y, value_w, row.h),
            );
            y += line;
        }

        y += l.pad * 0.5;
        for &&(lang, count) in &languages {
            let row = Rect::new(inner.x, y, inner.w, line);
            let dot_side = l.tiny * 0.7;
            fill(
                f,
                Rect::new(row.x, row.y + (row.h - dot_side) / 2.0, dot_side, dot_side),
                lang.color(),
                CornerRadii::all(dot_side / 2.0),
            );
            let label = format!("{}: {count}", lang.name());
            label_left(
                f,
                &Label {
                    text: &label,
                    size: l.small,
                    weight: FontWeightHint::Regular,
                    color: SUBTEXT0,
                },
                Rect::new(
                    row.x + dot_side + l.pad,
                    row.y,
                    (row.w - dot_side - l.pad).max(0.0),
                    row.h,
                ),
            );
            y += line;
        }
        f.unclip();
    }

    /// The overlay's rows, as they are written.
    #[must_use]
    pub fn stat_rows(&self, stats: &LibraryStats) -> Vec<(&'static str, String)> {
        vec![
            ("Snippets", stats.total_snippets.to_string()),
            ("Folders", stats.total_folders.to_string()),
            ("Tags", stats.total_tags.to_string()),
            ("Favorites", stats.favorites.to_string()),
            ("Templates", stats.templates.to_string()),
            ("Total Lines", stats.total_lines.to_string()),
            ("Total Size", format_size(stats.total_chars)),
        ]
    }

    /// The folders in the order the tree shows them, each with its depth.
    ///
    /// Nested folders used to be skipped outright — `if folder.parent_id
    /// .is_some() { continue; }` — while their index still counted towards the
    /// row position of every folder after them, so one nested folder left a
    /// blank row where the next top-level one should have been.
    /// `Folder::expanded`, set to `true` in three places and read nowhere, is
    /// what this walk is for.
    #[must_use]
    pub fn folder_rows(&self) -> Vec<(FolderId, usize)> {
        let mut rows = Vec::new();
        self.walk_folders(None, 0, &mut rows);
        rows
    }

    fn walk_folders(
        &self,
        parent: Option<FolderId>,
        depth: usize,
        out: &mut Vec<(FolderId, usize)>,
    ) {
        // There used to be a `depth >= MAX_FOLDER_DEPTH` bail-out here, put in
        // against a folder that is its own ancestor. It could not do that job,
        // and it did real damage instead.
        //
        // Why it could not: a folder has *one* parent, and this walk only ever
        // enters from `None`. So a folder is reached only if its parent chain
        // ends at `None`, and every folder in a cycle has a chain that never
        // does — the cycle is a separate component the walk never enters. The
        // same argument bounds the recursion: a reached folder's chain is
        // finite and cannot repeat a folder (a repeat *is* a cycle, so it
        // would not have been reached), hence depth is at most `folders.len()`
        // and the walk ends on any data at all.
        //
        // What it did instead: truncate. A ninth level of nesting — which the
        // New Folder button will happily build, since it files the new folder
        // under whichever one is picked — vanished from the tree, with the
        // snippets in it. That is a guard in front of a rule that already
        // holds, paid for in lost data (`known-issues.md` lesson 51).
        for folder in self.folders.iter().filter(|f| f.parent_id == parent) {
            out.push((folder.id, depth));
            if folder.expanded {
                self.walk_folders(Some(folder.id), depth.saturating_add(1), out);
            }
        }
    }

    /// Whether anything is filed under this folder.
    #[must_use]
    pub fn has_children(&self, id: FolderId) -> bool {
        self.folders.iter().any(|f| f.parent_id == Some(id))
    }

    /// The languages something is written in, with how many, in the order
    /// [`Language::all`] gives them.
    #[must_use]
    pub fn languages_in_use(&self) -> Vec<(Language, usize)> {
        Language::all()
            .iter()
            .filter_map(|&lang| {
                let count = self.snippets.iter().filter(|s| s.language == lang).count();
                (count > 0).then_some((lang, count))
            })
            .collect()
    }
}

// ============================================================================
// Drawing helpers
// ============================================================================

fn fill(f: &mut Frame<Target>, r: Rect, color: Color, corner_radii: CornerRadii) {
    if r.is_empty() {
        return;
    }
    f.push(RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii,
    });
}

fn stroke(
    f: &mut Frame<Target>,
    r: Rect,
    color: Color,
    line_width: f32,
    corner_radii: CornerRadii,
) {
    if r.is_empty() {
        return;
    }
    f.push(RenderCommand::StrokeRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        line_width,
        corner_radii,
    });
}

/// One string and everything about how it looks, minus where it goes.
struct Label<'a> {
    text: &'a str,
    size: f32,
    weight: FontWeightHint,
    color: Color,
}

/// The one place a `Text` command is built.
///
/// `limit` is passed straight through as `max_width`, so a caller that worked
/// out a width limit gets one the renderer will actually stop at, and the
/// overflow rule follows from it rather than being a second choice that could
/// disagree with it. The program this replaces set `max_width` to a constant
/// beside each string — `Some(160.0)`, `Some(200.0)`, `Some(25.0)` — chosen to
/// match a layout that was itself made of constants.
fn push_text(f: &mut Frame<Target>, l: &Label, x: f32, y: f32, limit: f32) {
    if l.text.is_empty() || limit <= 0.0 {
        return;
    }
    f.push(RenderCommand::Text {
        x,
        y,
        text: l.text.to_string(),
        color: l.color,
        font_size: l.size,
        font_weight: l.weight,
        max_width: Some(limit),
        overflow: TextOverflow::Ellipsis,
    });
}

/// Against the left edge of `r`, centred down it.
fn label_left(f: &mut Frame<Target>, l: &Label, r: Rect) {
    if r.is_empty() {
        return;
    }
    let lh = text::line_height(l.size, l.weight);
    push_text(f, l, r.x, r.y + (r.h - lh) / 2.0, r.w);
}

/// Centred in `r` — across from the measured width, down from the line height
/// — **and limited to `r`**.
///
/// The width that decides the centre is the width the renderer is told to stop
/// at, so the two cannot disagree; and because that width is never more than
/// `r.w`, `(r.w - w) / 2.0` is never negative, which is what keeps a string too
/// wide for its box starting at the box rather than to the left of it.
///
/// That claim was, for a while, only a claim: the centre was computed from the
/// measured `w` and the renderer was handed `r.w`, so a label offset into the
/// middle of its box was allowed to run half the leftover slack past the box's
/// right edge before it was told to stop. The empty editor's headline, centred
/// in the whole column, was permitted to reach 229 pixels outside a 1100-wide
/// window on that arithmetic.
fn label_centred(f: &mut Frame<Target>, l: &Label, r: Rect) {
    if r.is_empty() {
        return;
    }
    let w = text::measure(l.text, l.size, l.weight).min(r.w);
    let lh = text::line_height(l.size, l.weight);
    push_text(f, l, r.x + (r.w - w) / 2.0, r.y + (r.h - lh) / 2.0, w);
}

/// Take `w` off the right-hand end of `area`, leaving `gap` between what was
/// taken and what is left.
///
/// Returns [`Rect::EMPTY`] and takes nothing if there is not room, so a row
/// that runs out of space drops its right-hand items rather than drawing them
/// on top of its left-hand ones.
fn take_right(area: &mut Rect, w: f32, gap: f32) -> Rect {
    if w <= 0.0 || area.w < w {
        return Rect::EMPTY;
    }
    let taken = Rect::new(area.right() - w, area.y, w, area.h);
    area.w = (area.w - w - gap).max(0.0);
    taken
}

/// Take `w` off the left-hand end of `area`. See [`take_right`].
fn take_left(area: &mut Rect, w: f32, gap: f32) -> Rect {
    if w <= 0.0 || area.w < w {
        return Rect::EMPTY;
    }
    let taken = Rect::new(area.x, area.y, w, area.h);
    area.x += w + gap;
    area.w = (area.w - w - gap).max(0.0);
    taken
}

/// `r` with `dx` taken off each of its left and right edges.
fn inset_x(r: Rect, dx: f32) -> Rect {
    Rect::new(r.x + dx, r.y, (r.w - dx * 2.0).max(0.0), r.h)
}

/// `r` with `dy` taken off each of its top and bottom edges.
fn inset_y(r: Rect, dy: f32) -> Rect {
    Rect::new(r.x, r.y + dy, r.w, (r.h - dy * 2.0).max(0.0))
}

/// `r` with `d` taken off all four edges.
fn shrink(r: Rect, d: f32) -> Rect {
    inset_y(inset_x(r, d), d)
}

/// A row number as a distance to multiply a row height by.
///
/// Written out so the lint does not have to be turned off across the whole
/// file, which is what the eighteen crate-root allows amounted to.
#[expect(
    clippy::cast_precision_loss,
    reason = "a row index on a screen is orders of magnitude below 2^24"
)]
fn f32_from_usize(v: usize) -> f32 {
    v as f32
}

/// A window dimension as a length. See [`f32_from_usize`].
#[expect(
    clippy::cast_precision_loss,
    reason = "a window dimension is orders of magnitude below 2^24"
)]
fn f32_from_u32(v: u32) -> f32 {
    v as f32
}

/// Move `offset` by `rows` and hold it to a list of `total` rows shown
/// `capacity` at a time.
///
/// The clamp is the half that was missing: neither scroll offset had an upper
/// bound of any kind, because neither was ever assigned to.
fn clamp_scroll(offset: usize, rows: isize, total: usize, capacity: usize) -> usize {
    scroll_window::shift(offset, rows).min(total.saturating_sub(capacity))
}

/// A path as it goes on the status line.
///
/// Lossy on purpose and only here: a path is bytes and this is the one place
/// one is shown to a human rather than used.
fn show_path(path: &Path) -> String {
    path.display().to_string()
}

fn format_size(bytes: usize) -> String {
    guitk::bytes::iec(u64::try_from(bytes).unwrap_or(u64::MAX))
}

// ============================================================================
// The window
// ============================================================================

/// The one body both the window and the test probe drive, so what a click does
/// in a test is what it does on a screen.
pub fn handle_event(app: &mut App, event: &Event) -> EventResult {
    match event {
        Event::Key(ev) => app.handle_key(ev),
        Event::Mouse(ev) => app.handle_mouse(ev),
        Event::Resize { width, height } => {
            app.resize(f32_from_u32(*width), f32_from_u32(*height));
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

impl WindowApp for App {
    fn title(&self) -> String {
        TOOLBAR_TITLE.to_string()
    }

    fn app_id(&self) -> String {
        "snippets".to_string()
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the two window constants are small positive whole numbers"
    )]
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
        // against, which is the only reason it is stored at all. The program
        // this replaces drew at `WINDOW_WIDTH` whatever window it was in, and
        // received no clicks to read against anything.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for App {
    type Target = Target;
    type Outcome = EventResult;
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame<Target> {
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

    fn scroll_at(&mut self, x: f32, y: f32, dy: f32, size: (f32, f32)) -> Option<Self::Outcome> {
        self.resize(size.0, size.1);
        Some(handle_event(
            self,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Scroll { dx: 0.0, dy },
            }),
        ))
    }
}

fn main() -> ExitCode {
    let mut app = App::new();
    app::launch("snippets", &mut app)
}

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "a test that panics on bad data is a test that failed, which is the point"
)]
mod tests {
    use super::*;
    use guitk::probe::{
        bare_point, click, click_background, click_matching, control_names, ctrl, key, press,
        rect_of, release, scroll_at_point, shift, type_str,
    };

    // ── Helpers ─────────────────────────────────────────────────────────

    /// The window every test that does not say otherwise is read against.
    const W: (f32, f32) = App::SIZE;

    fn app() -> App {
        App::new()
    }

    /// An app whose library is exactly the snippets named, in that order, so a
    /// test can say "row two" and mean something.
    fn app_with(titles: &[&str]) -> App {
        let mut a = App::new();
        a.snippets.clear();
        a.folders.clear();
        a.recently_used.clear();
        a.selected_snippet_id = None;
        a.selected_folder_id = None;
        a.sort_order = SortOrder::DateAsc;
        for title in titles {
            let id = a.create_snippet(title, "", Language::PlainText).unwrap();
            // `DateAsc` sorts by `created_at`, which is the id, so the order
            // the titles were given in is the order they are listed in.
            assert!(id > 0);
        }
        a
    }

    fn titles(a: &App) -> Vec<String> {
        a.filtered_snippets()
            .iter()
            .map(|s| s.title.clone())
            .collect()
    }

    fn selected_title(a: &App) -> Option<String> {
        a.selected_snippet().map(|s| s.title.clone())
    }

    /// Every string the app draws at this size, in the order it draws them.
    ///
    /// What is on the screen, asked of the frame rather than of the model, so
    /// a test can tell "the model knows" from "the user can see".
    fn texts(a: &App, size: (f32, f32)) -> Vec<String> {
        a.frame(size.0, size.1)
            .into_tree()
            .commands
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }

    fn shows(a: &App, needle: &str) -> bool {
        texts(a, W).iter().any(|t| t == needle)
    }

    /// A document of `n` numbered lines: long enough that the code panel has
    /// to scroll, and numbered so a test can name the line it expects to see.
    fn numbered_lines(n: usize) -> String {
        use std::fmt::Write as _;
        let mut body = String::new();
        for i in 1..=n {
            let _ = writeln!(body, "line {i}");
        }
        body
    }

    fn id_of(a: &App, title: &str) -> SnippetId {
        a.snippets
            .iter()
            .find(|s| s.title == title)
            .unwrap_or_else(|| panic!("no snippet titled {title}"))
            .id
    }

    // ── Languages ───────────────────────────────────────────────────────

    #[test]
    fn a_language_has_a_name_an_extension_and_a_colour() {
        for &lang in Language::all() {
            assert!(!lang.name().is_empty(), "{lang:?} has no name");
            assert!(!lang.extension().is_empty(), "{lang:?} has no extension");
        }
    }

    #[test]
    fn every_language_is_reachable_from_its_own_extension() {
        // Except plain text, which is what an unknown extension answers, so it
        // has nothing of its own to be reached by.
        for &lang in Language::all() {
            if lang == Language::PlainText {
                continue;
            }
            assert_eq!(
                Language::from_extension(lang.extension()),
                lang,
                "{lang:?}'s own extension does not name it"
            );
        }
    }

    #[test]
    fn an_unknown_extension_is_plain_text() {
        assert_eq!(Language::from_extension("wat"), Language::PlainText);
        assert_eq!(Language::from_extension(""), Language::PlainText);
    }

    #[test]
    fn an_extension_is_recognised_whatever_its_case() {
        assert_eq!(Language::from_extension("RS"), Language::Rust);
        assert_eq!(Language::from_extension("Py"), Language::Python);
    }

    #[test]
    fn a_shebang_names_the_language() {
        assert_eq!(
            Language::detect_from_content("#!/usr/bin/env python3\nx = 1\n"),
            Language::Python
        );
        assert_eq!(
            Language::detect_from_content("#!/usr/bin/env node\nlet x = 1\n"),
            Language::JavaScript
        );
        assert_eq!(
            Language::detect_from_content("#!/bin/bash\necho hi\n"),
            Language::Shell
        );
    }

    #[test]
    fn rust_is_detected_from_a_function_and_one_other_token() {
        assert_eq!(
            Language::detect_from_content("fn main() { let x = 1; }"),
            Language::Rust
        );
        // `fn ` on its own is not enough — it is a word in English too.
        assert_ne!(
            Language::detect_from_content("the fn abbreviation"),
            Language::Rust
        );
    }

    #[test]
    fn content_nobody_recognises_is_plain_text() {
        assert_eq!(
            Language::detect_from_content("just some prose, honestly"),
            Language::PlainText
        );
    }

    #[test]
    fn a_name_beats_the_content_it_disagrees_with() {
        // The extension is what the user typed; the content sniffer is a guess.
        assert_eq!(
            guess_language("notes.py", "fn main() { let x = 1; }"),
            Language::Python
        );
    }

    #[test]
    fn a_name_with_no_useful_extension_falls_through_to_the_content() {
        assert_eq!(
            guess_language("notes", "fn main() { let x = 1; }"),
            Language::Rust
        );
        assert_eq!(
            guess_language("notes.wat", "fn main() { let x = 1; }"),
            Language::Rust
        );
        assert_eq!(
            guess_language("notes.", "fn main() { let x = 1; }"),
            Language::Rust
        );
    }

    #[test]
    fn every_language_offers_keywords_to_colour() {
        for &lang in Language::all() {
            if lang == Language::PlainText {
                continue;
            }
            assert!(
                !lang.keywords().is_empty(),
                "{lang:?} highlights nothing at all"
            );
        }
    }

    // ── Tokenizing ──────────────────────────────────────────────────────

    fn kinds(line: &str, lang: Language) -> Vec<(String, TokenKind)> {
        let lines = tokenize(line, lang);
        lines
            .into_iter()
            .flatten()
            .map(|t| (t.text, t.kind))
            .collect()
    }

    #[test]
    fn an_empty_document_still_has_one_line() {
        assert_eq!(tokenize("", Language::Rust).len(), 1);
    }

    #[test]
    fn a_document_has_one_entry_per_line() {
        assert_eq!(tokenize("a\nb\nc", Language::Rust).len(), 3);
    }

    #[test]
    fn a_keyword_is_a_keyword_and_a_name_is_not() {
        let got = kinds("fn main", Language::Rust);
        assert_eq!(got[0], ("fn".to_string(), TokenKind::Keyword));
        assert!(
            got.iter()
                .any(|(t, k)| t == "main" && *k == TokenKind::Identifier)
        );
    }

    #[test]
    fn a_capitalised_name_is_a_type_except_in_sql() {
        assert!(
            kinds("Vec", Language::Rust)
                .iter()
                .any(|(_, k)| *k == TokenKind::Type)
        );
        assert!(
            !kinds("SELECT", Language::Sql)
                .iter()
                .any(|(_, k)| *k == TokenKind::Type)
        );
    }

    #[test]
    fn a_string_keeps_its_quotes_and_its_escapes() {
        let got = kinds(r#""a\"b" x"#, Language::Rust);
        assert_eq!(got[0].1, TokenKind::String);
        assert_eq!(got[0].0, r#""a\"b""#);
    }

    #[test]
    fn an_unterminated_string_runs_to_the_end_of_the_line() {
        let got = kinds("\"never closed", Language::Rust);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, "\"never closed");
    }

    #[test]
    fn a_number_is_a_number_including_hex() {
        assert!(
            kinds("42", Language::Rust)
                .iter()
                .any(|(t, k)| t == "42" && *k == TokenKind::Number)
        );
        assert!(
            kinds("0xFF", Language::Rust)
                .iter()
                .any(|(t, k)| t == "0xFF" && *k == TokenKind::Number)
        );
        assert!(
            kinds("3.14", Language::Rust)
                .iter()
                .any(|(t, k)| t == "3.14" && *k == TokenKind::Number)
        );
    }

    #[test]
    fn each_language_family_has_its_own_comment_marker() {
        for (line, lang) in [
            ("// gone", Language::Rust),
            ("# gone", Language::Python),
            ("# gone", Language::Shell),
            ("-- gone", Language::Sql),
        ] {
            let got = kinds(line, lang);
            assert_eq!(got.len(), 1, "{lang:?} did not take {line} as one comment");
            assert_eq!(got[0].1, TokenKind::Comment, "{lang:?} on {line}");
        }
    }

    #[test]
    fn a_hash_is_not_a_comment_in_a_language_that_does_not_use_one() {
        assert!(
            !kinds("# not a comment", Language::Rust)
                .iter()
                .any(|(_, k)| *k == TokenKind::Comment)
        );
    }

    #[test]
    fn a_single_dash_is_an_operator_not_the_start_of_a_comment() {
        let got = kinds("a - b", Language::Sql);
        assert!(
            got.iter()
                .any(|(t, k)| t == "-" && *k == TokenKind::Operator)
        );
    }

    #[test]
    fn a_two_character_operator_is_one_token() {
        assert!(
            kinds("a == b", Language::Rust)
                .iter()
                .any(|(t, k)| t == "==" && *k == TokenKind::Operator)
        );
    }

    #[test]
    fn tokenizing_loses_nothing() {
        // Every character of the line comes back, in order: a highlighter that
        // drops a character draws source that is not the source.
        for (line, lang) in [
            ("fn main() { let x = 0xFF; } // done", Language::Rust),
            ("SELECT * FROM t -- all", Language::Sql),
            ("x = \"a\\\"b\" # note", Language::Python),
        ] {
            let joined: String = kinds(line, lang).into_iter().map(|(t, _)| t).collect();
            assert_eq!(joined, line, "{lang:?}");
        }
    }

    // ── Templates ───────────────────────────────────────────────────────

    #[test]
    fn a_template_variable_is_found_once_however_often_it_appears() {
        assert_eq!(
            extract_template_vars("${a} ${b} ${a}"),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn text_with_no_placeholder_has_no_variables() {
        assert!(extract_template_vars("plain text $ { } ${}").is_empty());
    }

    #[test]
    fn an_unclosed_placeholder_is_still_a_variable() {
        assert_eq!(extract_template_vars("${name"), vec!["name".to_string()]);
    }

    #[test]
    fn applying_a_template_replaces_every_copy_of_a_variable() {
        let filled = apply_template("${a}-${a}", &[("a".to_string(), "z".to_string())]);
        assert_eq!(filled, "z-z");
    }

    #[test]
    fn applying_a_template_leaves_variables_nobody_gave_a_value_for() {
        assert_eq!(apply_template("${a}", &[]), "${a}");
    }

    // ── Searching ───────────────────────────────────────────────────────

    #[test]
    fn an_empty_query_matches_everything() {
        let a = app();
        assert_eq!(
            search_snippets(&a.snippets, "", SearchScope::Title).len(),
            a.snippets.len()
        );
    }

    #[test]
    fn a_scope_looks_only_where_it_says() {
        let mut a = app_with(&["alpha"]);
        a.snippets[0].content = "beta".into();
        a.snippets[0].tags = vec!["gamma".into()];
        for (scope, hits) in [
            (SearchScope::Title, ["alpha"]),
            (SearchScope::Content, ["beta"]),
            (SearchScope::Tags, ["gamma"]),
        ] {
            assert_eq!(
                search_snippets(&a.snippets, hits[0], scope).len(),
                1,
                "{scope:?} did not find {}",
                hits[0]
            );
            let elsewhere = if scope == SearchScope::Title {
                "beta"
            } else {
                "alpha"
            };
            assert_eq!(
                search_snippets(&a.snippets, elsewhere, scope).len(),
                0,
                "{scope:?} found {elsewhere}, which is not in its scope"
            );
        }
    }

    #[test]
    fn the_all_scope_looks_everywhere() {
        let mut a = app_with(&["alpha"]);
        a.snippets[0].content = "beta".into();
        a.snippets[0].tags = vec!["gamma".into()];
        a.snippets[0].description = "delta".into();
        for needle in ["alpha", "beta", "gamma", "delta"] {
            assert_eq!(
                search_snippets(&a.snippets, needle, SearchScope::All).len(),
                1,
                "All missed {needle}"
            );
        }
    }

    #[test]
    fn a_query_ignores_case() {
        let a = app_with(&["Alpha"]);
        assert_eq!(
            search_snippets(&a.snippets, "ALPHA", SearchScope::Title).len(),
            1
        );
    }

    // ── Export ──────────────────────────────────────────────────────────

    #[test]
    fn an_export_names_every_snippet() {
        let a = app_with(&["one", "two"]);
        let json = export_snippets_json(&a.snippets);
        assert!(json.contains("\"one\""), "{json}");
        assert!(json.contains("\"two\""), "{json}");
    }

    #[test]
    fn an_export_escapes_what_would_break_it() {
        let mut a = app_with(&["a"]);
        a.snippets[0].content = "say \"hi\"\n\tthen stop".into();
        let json = export_snippets_json(&a.snippets);
        assert!(
            !json.contains("say \"hi\""),
            "the quotes went in raw: {json}"
        );
        assert!(json.contains("\\\"hi\\\""), "{json}");
        assert!(json.contains("\\n"), "{json}");
        assert!(json.contains("\\t"), "{json}");
    }

    #[test]
    fn an_export_of_nothing_is_still_json() {
        let json = export_snippets_json(&[]);
        assert!(json.starts_with('{'), "{json}");
        assert!(json.trim_end().ends_with('}'), "{json}");
    }

    // ── Layout ──────────────────────────────────────────────────────────

    #[test]
    fn the_layout_fills_the_window_it_is_given() {
        for size in [W, (640.0, 480.0), (1920.0, 1080.0), (400.0, 300.0)] {
            let l = Layout::new(size.0, size.1);
            assert_eq!(l.window, Rect::new(0.0, 0.0, size.0, size.1), "{size:?}");
        }
    }

    #[test]
    fn the_columns_lie_side_by_side_and_leave_no_gap() {
        for size in [W, (1400.0, 900.0), (900.0, 600.0)] {
            let l = Layout::new(size.0, size.1);
            assert_eq!(l.sidebar.x, 0.0, "{size:?}");
            assert_eq!(l.sidebar.right(), l.list.x, "{size:?} sidebar to list");
            assert_eq!(l.list.right(), l.editor.x, "{size:?} list to editor");
            assert_eq!(l.editor.right(), size.0, "{size:?} editor to edge");
        }
    }

    #[test]
    fn the_toolbar_is_above_the_columns_and_they_reach_the_bottom() {
        let l = Layout::new(W.0, W.1);
        assert_eq!(l.toolbar.y, 0.0);
        for column in [l.sidebar, l.list, l.editor] {
            assert_eq!(column.y, l.toolbar.bottom());
            assert_eq!(column.bottom(), W.1);
        }
    }

    #[test]
    fn a_wider_window_widens_the_editor_not_the_chrome() {
        // The columns are capped; the editor is what is left. A window twice
        // as wide is an editor much more than twice as wide, which is the
        // point of a cap.
        let narrow = Layout::new(1400.0, 750.0);
        let wide = Layout::new(2800.0, 750.0);
        assert_eq!(wide.sidebar.w, narrow.sidebar.w);
        assert_eq!(wide.list.w, narrow.list.w);
        assert!(
            wide.editor.w > narrow.editor.w * 1.9,
            "{} {}",
            narrow.editor.w,
            wide.editor.w
        );
    }

    #[test]
    fn a_narrow_window_drops_the_sidebar_before_the_list() {
        // The list is the one that says which snippet is on screen, so it is
        // the one that survives longer.
        let l = Layout::new(650.0, 750.0);
        assert_eq!(l.sidebar.w, 0.0, "the sidebar should have gone");
        assert!(l.list.w > 0.0, "the list should not have");
    }

    #[test]
    fn a_window_too_narrow_for_either_column_is_all_editor() {
        let l = Layout::new(450.0, 750.0);
        assert_eq!(l.sidebar.w, 0.0);
        assert_eq!(l.list.w, 0.0);
        assert_eq!(l.editor.w, 450.0);
    }

    #[test]
    fn the_editor_is_never_squeezed_out_by_the_columns() {
        // Sweep, because a rule that holds at three widths and fails at the
        // fourth is a rule nobody has checked.
        let mut w = 200.0_f32;
        while w <= 3000.0 {
            let l = Layout::new(w, 750.0);
            assert!(
                l.editor.w >= l.font * 24.0 || l.editor.w == w,
                "at {w} the editor got {} px",
                l.editor.w
            );
            w += 7.0;
        }
    }

    #[test]
    fn no_part_of_the_layout_leaves_the_window() {
        let mut h = 200.0_f32;
        while h <= 1600.0 {
            let l = Layout::new(1000.0, h);
            for r in [l.toolbar, l.sidebar, l.list, l.editor] {
                assert!(r.x >= 0.0 && r.y >= 0.0, "at h={h}: {r:?}");
                assert!(r.right() <= 1000.0 + 0.01, "at h={h}: {r:?}");
                assert!(r.bottom() <= h + 0.01, "at h={h}: {r:?}");
            }
            h += 13.0;
        }
    }

    #[test]
    fn a_taller_window_does_not_make_the_text_bigger_for_ever() {
        // Capped, or a 4K window would draw a menu in 40pt.
        let l = Layout::new(1000.0, 4000.0);
        assert!(l.font <= 16.0, "{}", l.font);
        let tiny = Layout::new(1000.0, 100.0);
        assert!(tiny.font >= 8.0, "{}", tiny.font);
    }

    #[test]
    fn the_text_sizes_keep_their_order() {
        for h in [120.0, 300.0, 750.0, 2000.0] {
            let l = Layout::new(1000.0, h);
            assert!(l.tiny < l.small, "at h={h}");
            assert!(l.small < l.font, "at h={h}");
            assert!(l.font < l.head, "at h={h}");
            assert!(l.head < l.title, "at h={h}");
        }
    }

    #[test]
    fn a_row_is_tall_enough_for_the_text_that_goes_in_it() {
        for h in [200.0, 750.0, 1600.0] {
            let l = Layout::new(1000.0, h);
            assert!(
                l.row >= text::line_height(l.small, FontWeightHint::Regular),
                "at h={h}: row {} vs text {}",
                l.row,
                text::line_height(l.small, FontWeightHint::Regular)
            );
            assert!(
                l.list_row >= text::line_height(l.font, FontWeightHint::Bold),
                "at h={h}"
            );
        }
    }

    #[test]
    fn the_list_header_sits_on_top_of_the_list_body() {
        let a = app();
        let l = a.layout();
        let head = a.list_header(&l);
        let body = a.list_body(&l);
        assert_eq!(head.y, l.list.y);
        assert_eq!(head.bottom(), body.y);
        assert_eq!(body.bottom(), l.list.bottom());
    }

    #[test]
    fn the_editor_is_a_header_a_code_panel_and_a_status_bar_in_that_order() {
        let a = app();
        let l = a.layout();
        let p = a.editor_parts(&l);
        assert_eq!(p.header.y, l.editor.y);
        assert_eq!(p.code.y, p.header.bottom());
        assert_eq!(p.code.bottom(), p.status.y);
        assert_eq!(p.status.bottom(), l.editor.bottom());
    }

    #[test]
    fn the_editor_parts_survive_an_editor_with_no_room_in_it() {
        // A window short enough that the header alone would overflow: the
        // parts must still be inside the column rather than upside down.
        let l = Layout::new(1000.0, 50.0);
        let a = app();
        let p = a.editor_parts(&l);
        // The fixture only tests what it is named for while the header really
        // does fill the column — at 100px tall it did not, and the clamps
        // below it were never reached.
        assert!(
            (p.header.h - l.editor.h).abs() < 0.01,
            "the window is not short enough: header {} of column {}",
            p.header.h,
            l.editor.h
        );
        for r in [p.header, p.code, p.status] {
            assert!(r.w >= 0.0 && r.h >= 0.0, "{r:?}");
            assert!(r.y >= l.editor.y - 0.01, "{r:?} starts above the column");
            assert!(r.bottom() <= l.editor.bottom() + 0.01, "{r:?}");
        }
        // Still stacked, in order, when there is no room for the stack. Every
        // part staying inside the column is not enough: with no height left
        // over, a status bar given its full height sits *on top of* the header
        // — inside the column, drawn over the title.
        assert!(
            p.code.y >= p.header.bottom() - 0.01,
            "the code panel climbed into the header: {p:?}"
        );
        assert!(
            p.status.y >= p.code.bottom() - 0.01,
            "the status bar climbed into the code panel: {p:?}"
        );
    }

    #[test]
    fn the_code_panel_holds_as_many_lines_as_it_draws() {
        // Asked of the drawing, not of a second copy of the arithmetic: the
        // capacity is what the keyboard scrolls by, and a capacity that
        // disagrees with the panel scrolls past lines nobody saw.
        let mut a = app_with(&["long"]);
        let body = numbered_lines(400);
        a.snippets[0].content = body;
        a.select(id_of(&a, "long"));
        let capacity = a.code_capacity(&a.layout());
        assert!(capacity > 0, "a 750px window should show some code");
        let drawn = texts(&a, W);
        assert!(
            drawn.iter().any(|t| t == &capacity.to_string()),
            "line {capacity} was counted as visible but not drawn"
        );
        assert!(
            !drawn
                .iter()
                .any(|t| t == &capacity.saturating_add(1).to_string()),
            "line {} was drawn but not counted",
            capacity + 1
        );
    }

    // ── Clicks ──────────────────────────────────────────────────────────

    #[test]
    fn every_control_the_toolbar_draws_can_be_clicked() {
        let a = app();
        for target in [Target::New, Target::Export, Target::Stats, Target::Search] {
            assert!(
                rect_of(&a, target).is_some(),
                "{target:?} is drawn with no hit box"
            );
        }
    }

    #[test]
    fn a_toolbar_button_does_not_sit_on_top_of_the_search_box() {
        // It used to: the buttons were laid out from a fixed x and the search
        // box from a fixed width, and at the default size they overlapped, so
        // one of the two was unclickable wherever they crossed.
        let a = app();
        let search = rect_of(&a, Target::Search).unwrap();
        for target in [Target::New, Target::Export, Target::Stats] {
            let r = rect_of(&a, target).unwrap();
            assert!(
                r.right() <= search.x || r.x >= search.right(),
                "{target:?} at {r:?} overlaps the search box at {search:?}"
            );
        }
    }

    #[test]
    fn the_toolbar_buttons_do_not_sit_on_top_of_each_other() {
        let a = app();
        let rects: Vec<(Target, Rect)> = [Target::New, Target::Export, Target::Stats]
            .into_iter()
            .map(|t| (t, rect_of(&a, t).unwrap()))
            .collect();
        for (i, (ta, ra)) in rects.iter().enumerate() {
            for (tb, rb) in rects.iter().skip(i + 1) {
                assert!(
                    ra.right() <= rb.x || rb.right() <= ra.x,
                    "{ta:?} at {ra:?} overlaps {tb:?} at {rb:?}"
                );
            }
        }
    }

    #[test]
    fn every_control_is_inside_the_window_it_is_drawn_in() {
        for size in [W, (700.0, 500.0), (1600.0, 1000.0)] {
            let a = app();
            for (_, r) in a.frame(size.0, size.1).hits() {
                assert!(r.x >= 0.0 && r.y >= 0.0, "{r:?} at {size:?}");
                assert!(r.right() <= size.0 + 0.01, "{r:?} at {size:?}");
                assert!(r.bottom() <= size.1 + 0.01, "{r:?} at {size:?}");
            }
        }
    }

    #[test]
    fn clicking_new_makes_a_snippet_and_shows_it() {
        let mut a = app_with(&["one"]);
        assert_eq!(click(&mut a, Target::New), EventResult::Consumed);
        assert_eq!(a.snippets.len(), 2);
        assert_eq!(selected_title(&a).as_deref(), Some("Untitled"));
    }

    #[test]
    fn a_new_snippet_takes_its_name_from_the_search_box() {
        let mut a = app_with(&["one"]);
        a.search_query = "helper.py".into();
        click(&mut a, Target::New);
        assert_eq!(selected_title(&a).as_deref(), Some("helper.py"));
    }

    #[test]
    fn a_new_snippet_takes_its_language_from_its_name() {
        let mut a = app_with(&["one"]);
        a.search_query = "helper.py".into();
        click(&mut a, Target::New);
        assert_eq!(a.selected_snippet().unwrap().language, Language::Python);
    }

    #[test]
    fn a_full_library_refuses_a_new_snippet_rather_than_pretending() {
        let mut a = app_with(&["one"]);
        while a.snippets.len() < MAX_SNIPPETS {
            a.snippets.push(a.snippets[0].clone());
        }
        assert_eq!(click(&mut a, Target::New), EventResult::Ignored);
        assert_eq!(a.snippets.len(), MAX_SNIPPETS);
    }

    #[test]
    fn clicking_a_row_selects_that_snippet() {
        let mut a = app_with(&["one", "two", "three"]);
        let id = id_of(&a, "two");
        assert_eq!(click(&mut a, Target::Row(id)), EventResult::Consumed);
        assert_eq!(selected_title(&a).as_deref(), Some("two"));
    }

    #[test]
    fn a_row_is_where_its_own_title_is_drawn() {
        // Not merely "a click somewhere reached it" — lesson 71: asking only
        // whether a click landed cannot tell a hit box in the right place from
        // one a whole row away.
        let a = app_with(&["alpha", "beta", "gamma"]);
        let l = a.layout();
        let body = a.list_body(&l);
        for (row, title) in ["alpha", "beta", "gamma"].iter().enumerate() {
            let r = rect_of(&a, Target::Row(id_of(&a, title))).unwrap();
            let expected_y = body.y + f32_from_usize(row) * l.list_row;
            assert!(
                (r.y - expected_y).abs() < 0.01,
                "{title} is row {row} but its box is at y={} not {expected_y}",
                r.y
            );
        }
    }

    #[test]
    fn the_star_on_a_row_is_inside_that_row() {
        let a = app_with(&["one", "two"]);
        for title in ["one", "two"] {
            let id = id_of(&a, title);
            let row = rect_of(&a, Target::Row(id)).unwrap();
            let star = rect_of(&a, Target::Star(id)).unwrap();
            assert!(
                star.y >= row.y - 0.01 && star.bottom() <= row.bottom() + 0.01,
                "{title}: star {star:?} is not inside row {row:?}"
            );
        }
    }

    #[test]
    fn clicking_a_star_favourites_only_that_snippet() {
        let mut a = app_with(&["one", "two"]);
        let two = id_of(&a, "two");
        click(&mut a, Target::Star(two));
        assert!(
            !a.snippets
                .iter()
                .find(|s| s.title == "one")
                .unwrap()
                .favorite
        );
        assert!(
            a.snippets
                .iter()
                .find(|s| s.title == "two")
                .unwrap()
                .favorite
        );
    }

    #[test]
    fn a_star_click_does_not_also_select_the_row() {
        // The star is recorded after the row, and the last hit wins, so a
        // click on the star is a star click and nothing else.
        let mut a = app_with(&["one", "two"]);
        a.select(id_of(&a, "one"));
        let two = id_of(&a, "two");
        click(&mut a, Target::Star(two));
        assert_eq!(selected_title(&a).as_deref(), Some("one"));
    }

    #[test]
    fn clicking_use_counts_a_use_and_remembers_it() {
        let mut a = app_with(&["one"]);
        a.select(id_of(&a, "one"));
        click(&mut a, Target::Use);
        assert_eq!(a.snippets[0].use_count, 1);
        assert_eq!(a.recently_used, vec![id_of(&a, "one")]);
    }

    #[test]
    fn using_with_nothing_selected_changes_nothing() {
        let mut a = app_with(&["one"]);
        a.selected_snippet_id = None;
        assert_eq!(a.press(Target::Use), EventResult::Ignored);
        assert!(a.recently_used.is_empty());
    }

    #[test]
    fn using_a_snippet_twice_leaves_one_entry_in_the_recent_list() {
        let mut a = app_with(&["one"]);
        a.select(id_of(&a, "one"));
        click(&mut a, Target::Use);
        click(&mut a, Target::Use);
        assert_eq!(a.recently_used.len(), 1);
        assert_eq!(a.snippets[0].use_count, 2);
    }

    #[test]
    fn the_recent_list_holds_the_most_recent_first() {
        let mut a = app_with(&["one", "two"]);
        a.select(id_of(&a, "one"));
        click(&mut a, Target::Use);
        a.select(id_of(&a, "two"));
        click(&mut a, Target::Use);
        assert_eq!(a.recently_used, vec![id_of(&a, "two"), id_of(&a, "one")]);
    }

    #[test]
    fn the_recent_list_never_grows_past_its_limit() {
        let mut a = app();
        a.recently_used.clear();
        for i in 0..(MAX_RECENT + 5) {
            let id = a
                .create_snippet(&format!("s{i}"), "", Language::PlainText)
                .unwrap();
            a.use_snippet(id);
        }
        assert_eq!(a.recently_used.len(), MAX_RECENT);
    }

    #[test]
    fn an_id_that_is_not_a_snippet_takes_no_place_in_the_recent_list() {
        // It used to: the count bump found nothing and the insert ran anyway,
        // so a phantom id pushed a real entry off the end, where the Recent
        // view — which only lists ids that match a snippet — hid the loss.
        let mut a = app_with(&["one"]);
        assert_eq!(a.use_snippet(9_999), EventResult::Ignored);
        assert!(a.recently_used.is_empty());
    }

    #[test]
    fn using_a_template_leaves_a_filled_copy_behind() {
        let mut a = app_with(&["t"]);
        a.snippets[0].content = "hello ${name}, you are ${age}".into();
        a.snippets[0].is_template = true;
        a.snippets[0].template_vars = vec!["name".into(), "age".into()];
        a.select(id_of(&a, "t"));
        click(&mut a, Target::Use);
        assert_eq!(a.snippets.len(), 2);
        let made = a.selected_snippet().unwrap();
        assert_eq!(made.title, "t (filled)");
        assert_eq!(made.content, "hello <name>, you are <age>");
    }

    #[test]
    fn using_an_ordinary_snippet_leaves_no_copy() {
        let mut a = app_with(&["plain"]);
        a.select(id_of(&a, "plain"));
        click(&mut a, Target::Use);
        assert_eq!(a.snippets.len(), 1);
    }

    #[test]
    fn clicking_delete_removes_the_selected_snippet_and_only_that_one() {
        let mut a = app_with(&["one", "two"]);
        a.select(id_of(&a, "two"));
        click(&mut a, Target::Delete);
        assert_eq!(titles(&a), vec!["one".to_string()]);
        assert!(a.selected_snippet_id.is_none());
    }

    #[test]
    fn deleting_with_nothing_selected_changes_nothing() {
        let mut a = app_with(&["one"]);
        a.selected_snippet_id = None;
        assert_eq!(a.press(Target::Delete), EventResult::Ignored);
        assert_eq!(a.snippets.len(), 1);
    }

    #[test]
    fn a_deleted_snippet_leaves_the_recent_list_too() {
        let mut a = app_with(&["one"]);
        let id = id_of(&a, "one");
        a.select(id);
        click(&mut a, Target::Use);
        click(&mut a, Target::Delete);
        assert!(a.recently_used.is_empty());
    }

    #[test]
    fn clicking_the_scope_button_steps_through_every_scope_and_comes_back() {
        let mut a = app();
        let first = a.search_scope;
        let mut seen = vec![first];
        for _ in 0..3 {
            click(&mut a, Target::Scope);
            seen.push(a.search_scope);
        }
        click(&mut a, Target::Scope);
        assert_eq!(a.search_scope, first, "it should have come back round");
        seen.sort_by_key(|s| format!("{s:?}"));
        seen.dedup();
        assert_eq!(seen.len(), 4, "a scope was skipped: {seen:?}");
    }

    #[test]
    fn clicking_the_sort_button_steps_through_every_order_and_comes_back() {
        let mut a = app();
        let first = a.sort_order;
        let mut seen = vec![first];
        for _ in 0..5 {
            click(&mut a, Target::Sort);
            seen.push(a.sort_order);
        }
        click(&mut a, Target::Sort);
        assert_eq!(a.sort_order, first);
        seen.sort_by_key(|s| format!("{s:?}"));
        seen.dedup();
        assert_eq!(seen.len(), 6, "an order was skipped: {seen:?}");
    }

    #[test]
    fn every_sidebar_view_has_a_button_and_the_button_selects_it() {
        let mut a = app();
        for view in SidebarView::ALL {
            assert_eq!(click(&mut a, Target::View(view)), EventResult::Consumed);
            assert_eq!(a.sidebar_view, view);
        }
    }

    #[test]
    fn clicking_a_folder_selects_it_and_clicking_it_again_lets_go() {
        let mut a = app();
        let id = a.folders[0].id;
        click(&mut a, Target::Folder(id));
        assert_eq!(a.selected_folder_id, Some(id));
        click(&mut a, Target::Folder(id));
        assert_eq!(a.selected_folder_id, None);
    }

    #[test]
    fn a_twisty_opens_and_shuts_its_own_folder_and_no_other() {
        let mut a = app();
        let parent = a
            .folders
            .iter()
            .find(|f| a.has_children(f.id))
            .map(|f| f.id)
            .expect("the seeded library has a folder with children");
        let others: Vec<(FolderId, bool)> = a
            .folders
            .iter()
            .filter(|f| f.id != parent)
            .map(|f| (f.id, f.expanded))
            .collect();
        let before = a.folders.iter().find(|f| f.id == parent).unwrap().expanded;
        click(&mut a, Target::Twisty(parent));
        assert_eq!(
            a.folders.iter().find(|f| f.id == parent).unwrap().expanded,
            !before
        );
        for (id, was) in others {
            assert_eq!(a.folders.iter().find(|f| f.id == id).unwrap().expanded, was);
        }
    }

    #[test]
    fn a_twisty_is_drawn_only_where_there_is_something_to_open() {
        let a = app();
        for folder in &a.folders {
            let has = rect_of(&a, Target::Twisty(folder.id)).is_some();
            assert_eq!(
                has,
                a.has_children(folder.id),
                "folder {} has a twisty it cannot use",
                folder.name
            );
        }
    }

    #[test]
    fn shutting_a_folder_hides_its_children_from_the_tree() {
        let mut a = app();
        let parent = a
            .folders
            .iter()
            .find(|f| a.has_children(f.id))
            .map(|f| f.id)
            .unwrap();
        let child = a
            .folders
            .iter()
            .find(|f| f.parent_id == Some(parent))
            .unwrap()
            .id;
        assert!(a.folder_rows().iter().any(|&(id, _)| id == child));
        click(&mut a, Target::Twisty(parent));
        assert!(!a.folder_rows().iter().any(|&(id, _)| id == child));
    }

    #[test]
    fn a_folder_cycle_does_not_hang_the_tree() {
        // Nothing stops two folders naming each other as parent, and the walk
        // must end on the data it is given rather than on the data it hopes
        // for. It ends because a cycle is *unreachable* from the roots, not
        // because a depth counter cuts it off — so the thing to assert is that
        // the walk returns and that the two folders in the cycle are simply
        // not in the tree, rather than a bound on how many rows came back.
        let mut a = app();
        let (a_id, b_id) = (a.folders[0].id, a.folders[1].id);
        a.folders[0].parent_id = Some(b_id);
        a.folders[1].parent_id = Some(a_id);
        for f in &mut a.folders {
            f.expanded = true;
        }
        let rows = a.folder_rows();
        assert!(
            !rows.iter().any(|&(id, _)| id == a_id || id == b_id),
            "a folder with no way up to the root was shown anyway"
        );
        assert_eq!(
            rows.len(),
            a.folders.len().saturating_sub(2),
            "every other folder is still in the tree exactly once"
        );
    }

    #[test]
    fn a_deeply_nested_folder_is_still_in_the_tree() {
        // The fault a depth cap caused: past its limit the tree stopped, and
        // the folders below it — and the snippets filed in them — were gone
        // from the sidebar with nothing said. The New Folder button files a
        // new folder under the picked one, so a user can build a chain this
        // long by clicking it.
        let mut a = app();
        let mut parent = a.folders[0].id;
        let mut chain = vec![parent];
        for i in 0..20 {
            a.selected_folder_id = Some(parent);
            parent = a
                .create_folder(&format!("deep{i}"))
                .expect("a named folder is made");
            chain.push(parent);
        }
        for f in &mut a.folders {
            f.expanded = true;
        }
        let rows = a.folder_rows();
        for (want_depth, id) in chain.iter().enumerate() {
            let (_, got) = rows
                .iter()
                .find(|&&(row_id, _)| row_id == *id)
                .copied()
                .unwrap_or_else(|| panic!("the folder {want_depth} deep is not in the tree"));
            assert_eq!(
                got, want_depth,
                "the folder is in the tree at the wrong indent"
            );
        }
    }

    #[test]
    fn clicking_new_folder_makes_one_under_the_selected_folder() {
        let mut a = app();
        let parent = a.folders[0].id;
        a.selected_folder_id = Some(parent);
        let before = a.folders.len();
        a.search_query = "Scratch".into();
        assert_eq!(click(&mut a, Target::NewFolder), EventResult::Consumed);
        assert_eq!(a.folders.len(), before + 1);
        let made = a.folders.last().unwrap();
        assert_eq!(made.name, "Scratch");
        assert_eq!(made.parent_id, Some(parent));
    }

    #[test]
    fn a_new_folder_with_nothing_typed_still_gets_a_name() {
        let mut a = app();
        a.search_query.clear();
        click(&mut a, Target::NewFolder);
        assert_eq!(a.folders.last().unwrap().name, "Folder");
    }

    #[test]
    fn a_full_shelf_of_folders_refuses_another() {
        let mut a = app();
        while a.folders.len() < MAX_FOLDERS {
            a.folders.push(a.folders[0].clone());
        }
        assert_eq!(a.press(Target::NewFolder), EventResult::Ignored);
        assert_eq!(a.folders.len(), MAX_FOLDERS);
    }

    #[test]
    fn only_the_selected_folder_offers_to_be_deleted() {
        let mut a = app();
        let id = a.folders[0].id;
        assert!(rect_of(&a, Target::DeleteFolder(id)).is_none());
        click(&mut a, Target::Folder(id));
        assert!(rect_of(&a, Target::DeleteFolder(id)).is_some());
    }

    #[test]
    fn deleting_a_folder_keeps_its_snippets_and_moves_them_to_the_root() {
        let mut a = app();
        let id = a.folders.iter().find(|f| !a.has_children(f.id)).unwrap().id;
        let mut kept = a.snippets.len();
        if !a.snippets.iter().any(|s| s.folder_id == Some(id)) {
            a.snippets[0].folder_id = Some(id);
            kept = a.snippets.len();
        }
        click(&mut a, Target::Folder(id));
        click(&mut a, Target::DeleteFolder(id));
        assert!(!a.folders.iter().any(|f| f.id == id));
        assert_eq!(a.snippets.len(), kept, "a snippet went with the folder");
        assert!(!a.snippets.iter().any(|s| s.folder_id == Some(id)));
    }

    #[test]
    fn clicking_stats_opens_the_overlay_and_clicking_outside_shuts_it() {
        let mut a = app();
        click(&mut a, Target::Stats);
        assert!(a.show_stats);
        click(&mut a, Target::CloseStats);
        assert!(!a.show_stats);
    }

    #[test]
    fn the_overlay_covers_everything_behind_it() {
        // Every hit box the frame offers while the overlay is up either closes
        // it or is the dialog itself; nothing behind can be reached.
        let mut a = app();
        a.show_stats = true;
        let frame = a.frame(W.0, W.1);
        for (target, r) in frame.hits() {
            // What a click at the middle of that box would actually reach, not
            // merely what was recorded: the backdrop is recorded last and the
            // last hit wins, which is the mechanism being checked.
            let reached = frame.hit_test(r.x + r.w / 2.0, r.y + r.h / 2.0);
            assert_eq!(
                reached,
                Some(Target::CloseStats),
                "a click on {target:?} at {r:?} reaches {reached:?} behind a modal"
            );
        }
    }

    #[test]
    fn clicking_the_cross_empties_the_search_box() {
        let mut a = app();
        a.search_query = "abc".into();
        assert!(rect_of(&a, Target::ClearSearch).is_some());
        click(&mut a, Target::ClearSearch);
        assert!(a.search_query.is_empty());
    }

    #[test]
    fn there_is_no_cross_when_there_is_nothing_to_clear() {
        let mut a = app();
        a.search_query.clear();
        assert!(rect_of(&a, Target::ClearSearch).is_none());
    }

    #[test]
    fn a_click_on_nothing_is_ignored() {
        let mut a = app();
        assert_eq!(click_background(&mut a), EventResult::Ignored);
    }

    #[test]
    fn a_click_on_a_panel_body_is_not_a_click_on_a_thing() {
        let mut a = app();
        assert_eq!(a.press(Target::List), EventResult::Ignored);
        assert_eq!(a.press(Target::Code), EventResult::Ignored);
    }

    // ── The keyboard ────────────────────────────────────────────────────

    #[test]
    fn down_walks_the_list_and_stops_at_the_end() {
        let mut a = app_with(&["one", "two", "three"]);
        for expected in ["one", "two", "three", "three"] {
            key(&mut a, &press(Key::Down));
            assert_eq!(selected_title(&a).as_deref(), Some(expected));
        }
    }

    #[test]
    fn up_walks_back_and_stops_at_the_top() {
        let mut a = app_with(&["one", "two", "three"]);
        a.select(id_of(&a, "three"));
        for expected in ["two", "one", "one"] {
            key(&mut a, &press(Key::Up));
            assert_eq!(selected_title(&a).as_deref(), Some(expected));
        }
    }

    #[test]
    fn up_with_nothing_selected_starts_at_the_bottom() {
        // Symmetrical with Down starting at the top: the key that walks
        // backwards should enter the list from the end it walks from.
        let mut a = app_with(&["one", "two", "three"]);
        key(&mut a, &press(Key::Up));
        assert_eq!(selected_title(&a).as_deref(), Some("three"));
    }

    #[test]
    fn home_and_end_reach_the_ends() {
        let mut a = app_with(&["one", "two", "three"]);
        key(&mut a, &press(Key::End));
        assert_eq!(selected_title(&a).as_deref(), Some("three"));
        key(&mut a, &press(Key::Home));
        assert_eq!(selected_title(&a).as_deref(), Some("one"));
    }

    #[test]
    fn a_page_is_more_than_a_row_and_no_more_than_the_list_holds() {
        let a = app();
        let l = a.layout();
        let capacity = scroll_window::capacity(l.list_row, a.list_body(&l).h);
        let page = a.page();
        assert!(page > 1, "a page of {page} rows is not a page");
        assert!(
            usize::try_from(page).unwrap_or(usize::MAX) <= capacity,
            "a page of {page} is bigger than the {capacity} rows on screen"
        );
    }

    #[test]
    fn page_down_moves_a_page_and_page_up_brings_it_back() {
        let names: Vec<String> = (0..60).map(|i| format!("s{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut a = app_with(&refs);
        key(&mut a, &press(Key::Home));
        key(&mut a, &press(Key::PageDown));
        let after = a.selected_row().unwrap();
        assert_eq!(isize::try_from(after).unwrap(), a.page());
        key(&mut a, &press(Key::PageUp));
        assert_eq!(a.selected_row(), Some(0));
    }

    #[test]
    fn moving_the_selection_on_an_empty_list_is_ignored() {
        let mut a = app_with(&[]);
        assert_eq!(key(&mut a, &press(Key::Down)), EventResult::Ignored);
        assert_eq!(key(&mut a, &press(Key::Up)), EventResult::Ignored);
        assert!(a.selected_snippet_id.is_none());
    }

    #[test]
    fn the_selection_follows_the_order_the_list_is_sorted_in() {
        // Not the order the library is stored in — the arrow keys walk what is
        // on screen, or they walk somewhere the user is not looking.
        let mut a = app_with(&["b", "a", "c"]);
        a.sort_order = SortOrder::NameAsc;
        key(&mut a, &press(Key::Down));
        assert_eq!(selected_title(&a).as_deref(), Some("a"));
        key(&mut a, &press(Key::Down));
        assert_eq!(selected_title(&a).as_deref(), Some("b"));
    }

    #[test]
    fn walking_the_list_scrolls_the_row_into_view() {
        // The walk has to be a *walk*. End reaches the same row, but through
        // `select_end`, which brings the row into view with a call of its own,
        // so a test that pressed End could not tell whether `move_selection`
        // had kept its call or dropped it.
        let names: Vec<String> = (0..80).map(|i| format!("s{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut a = app_with(&refs);
        let l = a.layout();
        let capacity = scroll_window::capacity(l.list_row, a.list_body(&l).h);
        assert!(
            (2..80).contains(&capacity),
            "the fixture needs a window shorter than the list and taller than \
             two rows; it holds {capacity}"
        );
        // The first Down picks row 0, so `capacity + 1` of them land on row
        // `capacity` — the first row past the screen the walk started on.
        for _ in 0..=capacity {
            key(&mut a, &press(Key::Down));
        }
        let row = a.selected_row().expect("the walk picked a row");
        assert_eq!(
            row, capacity,
            "the walk did not reach the row past the screen"
        );
        assert!(row >= a.list_scroll, "row {row} is above the window");
        assert!(
            row < a.list_scroll.saturating_add(capacity),
            "row {row} is below a window of {capacity} starting at {}",
            a.list_scroll
        );
        assert!(rect_of(&a, Target::Row(a.selected_snippet_id.unwrap())).is_some());
        // Brought to the *bottom* edge, not the top: a walk that steps one row
        // off the screen scrolls by one row, and the row it stepped off is
        // still on show. Anchoring the row at the top instead would throw away
        // the whole page the user had just walked through.
        assert_eq!(
            a.list_scroll,
            row.saturating_add(1).saturating_sub(capacity),
            "the row was not brought to the bottom edge"
        );
        let before = a.filtered_ids()[row.saturating_sub(1)];
        assert!(
            rect_of(&a, Target::Row(before)).is_some(),
            "the row walked from was scrolled away"
        );
    }

    #[test]
    fn walking_past_the_end_brings_the_end_back_on_screen() {
        // The only case in which clamping the walk to the last row does
        // anything: press Down on the last row and the row it re-picks is the
        // one already picked, so nothing is observable — unless that row has
        // been scrolled out of sight, when re-picking it is what fetches it
        // back (`known-issues.md` lesson 70).
        let names: Vec<String> = (0..80).map(|i| format!("s{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut a = app_with(&refs);
        key(&mut a, &press(Key::End));
        let last = a.selected_snippet_id.expect("End picks the last row");
        for _ in 0..40 {
            scroll_at_point(&mut a, Target::Code, 10.0);
        }
        a.list_scroll = 0;
        assert!(
            rect_of(&a, Target::Row(last)).is_none(),
            "the last row is still on screen, so there is nothing to fetch back"
        );
        assert_eq!(key(&mut a, &press(Key::Down)), EventResult::Consumed);
        assert_eq!(a.selected_snippet_id, Some(last), "the selection moved");
        assert!(
            rect_of(&a, Target::Row(last)).is_some(),
            "Down on the last row did not bring it back on screen"
        );
    }

    #[test]
    fn enter_uses_the_selected_snippet() {
        let mut a = app_with(&["one"]);
        a.select(id_of(&a, "one"));
        key(&mut a, &press(Key::Enter));
        assert_eq!(a.snippets[0].use_count, 1);
    }

    #[test]
    fn delete_deletes_the_selected_snippet() {
        let mut a = app_with(&["one", "two"]);
        a.select(id_of(&a, "two"));
        key(&mut a, &press(Key::Delete));
        assert_eq!(titles(&a), vec!["one".to_string()]);
    }

    #[test]
    fn f_favourites_the_selected_snippet() {
        let mut a = app_with(&["one"]);
        a.select(id_of(&a, "one"));
        key(&mut a, &press(Key::F));
        assert!(a.snippets[0].favorite);
        key(&mut a, &press(Key::F));
        assert!(!a.snippets[0].favorite);
    }

    #[test]
    fn s_opens_the_statistics_and_closes_them_again() {
        let mut a = app();
        key(&mut a, &press(Key::S));
        assert!(a.show_stats);
        key(&mut a, &press(Key::S));
        assert!(!a.show_stats);
    }

    #[test]
    fn escape_shuts_the_statistics() {
        let mut a = app();
        a.show_stats = true;
        key(&mut a, &press(Key::Escape));
        assert!(!a.show_stats);
    }

    #[test]
    fn the_statistics_overlay_swallows_the_keys_behind_it() {
        // A modal that lets Delete through is a modal that deletes what is
        // behind it while the user is reading a dialog.
        let mut a = app_with(&["one"]);
        a.select(id_of(&a, "one"));
        a.show_stats = true;
        assert_eq!(key(&mut a, &press(Key::Delete)), EventResult::Ignored);
        assert_eq!(key(&mut a, &press(Key::N)), EventResult::Ignored);
        assert_eq!(a.snippets.len(), 1);
    }

    #[test]
    fn n_makes_a_snippet() {
        let mut a = app_with(&["one"]);
        key(&mut a, &press(Key::N));
        assert_eq!(a.snippets.len(), 2);
    }

    #[test]
    fn o_steps_the_sort_order() {
        let mut a = app();
        let before = a.sort_order;
        key(&mut a, &press(Key::O));
        assert_ne!(a.sort_order, before);
    }

    #[test]
    fn tab_and_shift_tab_step_the_sidebar_the_two_ways() {
        let mut a = app();
        let first = a.sidebar_view;
        key(&mut a, &press(Key::Tab));
        assert_ne!(a.sidebar_view, first);
        key(&mut a, &shift(Key::Tab));
        assert_eq!(a.sidebar_view, first);
    }

    #[test]
    fn tab_reaches_every_view_and_comes_back_round() {
        let mut a = app();
        let first = a.sidebar_view;
        let mut seen = vec![first];
        for _ in 0..(SidebarView::ALL.len() - 1) {
            key(&mut a, &press(Key::Tab));
            seen.push(a.sidebar_view);
        }
        key(&mut a, &press(Key::Tab));
        assert_eq!(a.sidebar_view, first);
        seen.sort_by_key(|v| format!("{v:?}"));
        seen.dedup();
        assert_eq!(seen.len(), SidebarView::ALL.len(), "a view was skipped");
    }

    #[test]
    fn shift_tab_reaches_every_view_too() {
        let mut a = app();
        let mut seen = vec![a.sidebar_view];
        for _ in 0..(SidebarView::ALL.len() - 1) {
            key(&mut a, &shift(Key::Tab));
            seen.push(a.sidebar_view);
        }
        seen.sort_by_key(|v| format!("{v:?}"));
        seen.dedup();
        assert_eq!(seen.len(), SidebarView::ALL.len());
    }

    #[test]
    fn changing_view_puts_the_list_back_at_the_top() {
        // The new view is a different list; an offset carried over from the
        // old one points at a row that is not there.
        let names: Vec<String> = (0..80).map(|i| format!("s{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut a = app_with(&refs);
        key(&mut a, &press(Key::End));
        assert!(a.list_scroll > 0);
        key(&mut a, &press(Key::Tab));
        assert_eq!(a.list_scroll, 0);
    }

    #[test]
    fn a_key_nothing_is_bound_to_is_ignored() {
        let mut a = app();
        assert_eq!(key(&mut a, &press(Key::F9)), EventResult::Ignored);
    }

    #[test]
    fn a_key_coming_back_up_is_not_a_second_keystroke() {
        // Both halves of a keystroke arrive. Acting on the release too would
        // run every shortcut twice — Tab would step two views on, Delete would
        // take two snippets — so the release has to do nothing at all.
        let mut a = app();
        let before = a.sidebar_view;
        assert_eq!(key(&mut a, &release(Key::Tab)), EventResult::Ignored);
        assert_eq!(a.sidebar_view, before, "the release stepped the view on");
        assert_eq!(key(&mut a, &release(Key::Slash)), EventResult::Ignored);
        assert!(!a.search_focus, "the release opened the search box");
        // And with the box open, where a different handler reads the key.
        key(&mut a, &press(Key::Slash));
        assert!(a.search_focus);
        assert_eq!(key(&mut a, &release(Key::Escape)), EventResult::Ignored);
        assert!(a.search_focus, "the release shut the search box");
    }

    // ── The search box ──────────────────────────────────────────────────

    #[test]
    fn slash_and_ctrl_f_both_reach_the_search_box() {
        for opener in [press(Key::Slash), ctrl(Key::F)] {
            let mut a = app();
            key(&mut a, &opener);
            assert!(a.search_focus, "{opener:?} did not focus the search box");
        }
    }

    #[test]
    fn the_key_that_opens_the_search_box_is_not_also_typed_into_it() {
        let mut a = app();
        key(&mut a, &press(Key::Slash));
        assert!(a.search_query.is_empty(), "got {:?}", a.search_query);
    }

    #[test]
    fn the_search_box_takes_the_text_a_key_types_and_not_the_rest() {
        // A keystroke carries the text the layout produced, which for Tab,
        // Escape and the rest is a control character rather than nothing at
        // all. A box that appended whatever arrived would fill with characters
        // that are invisible on screen and match no snippet — and would claim
        // the keystroke while doing it.
        let mut a = app_with(&["alpha"]);
        click(&mut a, Target::Search);
        type_str(&mut a, "al");
        a.list_scroll = 3;

        let mut tab = press(Key::Tab);
        tab.text = "\t".into();
        assert_eq!(
            key(&mut a, &tab),
            EventResult::Ignored,
            "a keystroke that types nothing was claimed anyway"
        );
        assert_eq!(a.search_query, "al", "a control character reached the box");
        assert_eq!(a.list_scroll, 3, "the list was scrolled back for nothing");

        // The same key with a real character on it does reach the box.
        let mut letter = press(Key::Tab);
        letter.text = "p\u{7}".into();
        assert_eq!(key(&mut a, &letter), EventResult::Consumed);
        assert_eq!(
            a.search_query, "alp",
            "the bell character was taken along with the letter"
        );
    }

    #[test]
    fn typing_into_the_search_box_narrows_the_list() {
        let mut a = app_with(&["alpha", "beta"]);
        click(&mut a, Target::Search);
        type_str(&mut a, "alp");
        assert_eq!(a.search_query, "alp");
        assert_eq!(titles(&a), vec!["alpha".to_string()]);
    }

    #[test]
    fn typing_with_the_search_box_shut_still_works_the_shortcuts() {
        // A real keystroke carries both a key and the text it types. With the
        // box shut the letter has to run the shortcut and leave the query
        // alone; with it open, the other test above, the opposite.
        let mut a = app_with(&["one"]);
        a.search_focus = false;
        let mut ev = press(Key::N);
        ev.text = "n".to_string();
        key(&mut a, &ev);
        assert_eq!(a.snippets.len(), 2, "N should have made a snippet");
        assert!(a.search_query.is_empty());
    }

    #[test]
    fn backspace_takes_a_character_back_off_the_query() {
        let mut a = app();
        a.search_focus = true;
        type_str(&mut a, "abc");
        key(&mut a, &press(Key::Backspace));
        assert_eq!(a.search_query, "ab");
    }

    #[test]
    fn backspace_on_an_empty_query_is_ignored_rather_than_pretending() {
        let mut a = app();
        a.search_focus = true;
        a.search_query.clear();
        assert_eq!(key(&mut a, &press(Key::Backspace)), EventResult::Ignored);
    }

    #[test]
    fn backspace_takes_back_a_character_not_a_byte() {
        // A query is text, and text is not bytes: popping a byte off "é"
        // leaves half a character behind.
        let mut a = app();
        a.search_focus = true;
        type_str(&mut a, "aé");
        key(&mut a, &press(Key::Backspace));
        assert_eq!(a.search_query, "a");
    }

    #[test]
    fn enter_and_escape_both_leave_the_search_box() {
        for closer in [press(Key::Enter), press(Key::Escape)] {
            let mut a = app();
            a.search_focus = true;
            key(&mut a, &closer);
            assert!(!a.search_focus, "{closer:?} did not leave the box");
        }
    }

    #[test]
    fn the_arrows_still_walk_the_list_while_the_search_box_has_the_keyboard() {
        // Typing a query and then picking from the results is one motion; a
        // search box that swallowed Down would break it in half.
        let mut a = app_with(&["one", "two"]);
        a.search_focus = true;
        key(&mut a, &press(Key::Down));
        assert_eq!(selected_title(&a).as_deref(), Some("one"));
        assert!(a.search_focus, "walking the list should not close the box");
    }

    #[test]
    fn escape_outside_the_search_box_clears_a_query() {
        let mut a = app();
        a.search_focus = false;
        a.search_query = "abc".into();
        key(&mut a, &press(Key::Escape));
        assert!(a.search_query.is_empty());
    }

    #[test]
    fn escape_with_nothing_to_clear_is_ignored() {
        let mut a = app();
        a.search_focus = false;
        a.search_query.clear();
        assert_eq!(key(&mut a, &press(Key::Escape)), EventResult::Ignored);
    }

    #[test]
    fn a_narrowed_query_puts_the_list_back_at_the_top() {
        let names: Vec<String> = (0..80).map(|i| format!("s{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut a = app_with(&refs);
        key(&mut a, &press(Key::End));
        assert!(a.list_scroll > 0);
        a.search_focus = true;
        type_str(&mut a, "s0");
        assert_eq!(a.list_scroll, 0);
    }

    // ── The wheel ───────────────────────────────────────────────────────

    /// A point in the list panel that no row covers.
    ///
    /// `capacity` is a floor, so the rows never quite fill the panel: what is
    /// left below the last one is the strip `Target::List` exists to catch. The
    /// strip is why the panel records a hit box at all, and it is the only place
    /// in the window where that box is what `hit_test` answers — everywhere else
    /// a row, drawn after it, is on top.
    fn point_below_the_last_row(a: &App) -> (f32, f32) {
        let l = a.layout();
        let body = a.list_body(&l);
        let rows = scroll_window::capacity(l.list_row, body.h);
        let used = l.list_row * rows as f32;
        assert!(
            body.h - used > 1.0,
            "no strip below the last row at this size: body {:.2} holds {rows} rows of {:.2}",
            body.h,
            l.list_row
        );
        (body.x + body.w / 2.0, body.bottom() - 0.5)
    }

    #[test]
    fn the_wheel_over_the_list_scrolls_the_list_and_not_the_code() {
        let names: Vec<String> = (0..80).map(|i| format!("s{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut a = app_with(&refs);
        // Over a *row*, not through `scroll` with the answer already in hand:
        // the routing is half of what the wheel does, and a test that supplies
        // the target has tested only the other half.
        let row = rect_of(&a, Target::Row(id_of(&a, "s00"))).expect("the first row is drawn");
        let (rx, ry) = row.centre();
        assert_eq!(
            a.scroll_at(rx, ry, -3.0, App::SIZE),
            Some(EventResult::Consumed)
        );
        assert!(a.list_scroll > 0, "the list did not move");
        assert_eq!(a.code_scroll, 0, "the code moved instead");
    }

    #[test]
    fn the_wheel_below_the_last_row_still_scrolls_the_list() {
        // The panel's own hit box, which nothing else in the window can reach.
        // Deleting `f.hit(Target::List, body)` left every other wheel test
        // passing, because each of them went in over a row.
        let names: Vec<String> = (0..80).map(|i| format!("s{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut a = app_with(&refs);
        let (x, y) = point_below_the_last_row(&a);
        assert_eq!(a.draw(App::SIZE).hit_test(x, y), Some(Target::List));
        assert_eq!(
            a.scroll_at(x, y, -3.0, App::SIZE),
            Some(EventResult::Consumed)
        );
        assert!(a.list_scroll > 0, "the list did not move");
    }

    #[test]
    fn the_wheel_over_the_code_scrolls_the_code_and_not_the_list() {
        let mut a = app_with(&["long"]);
        a.snippets[0].content = numbered_lines(400);
        a.select(id_of(&a, "long"));
        assert_eq!(
            scroll_at_point(&mut a, Target::Code, -3.0),
            EventResult::Consumed
        );
        assert!(a.code_scroll > 0, "the code did not move");
        assert_eq!(a.list_scroll, 0, "the list moved instead");
    }

    #[test]
    fn the_wheel_stops_at_the_top() {
        let names: Vec<String> = (0..80).map(|i| format!("s{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut a = app_with(&refs);
        a.scroll(Target::List, 30.0);
        assert_eq!(a.list_scroll, 0);
    }

    #[test]
    fn the_wheel_stops_where_the_last_row_is_at_the_bottom() {
        // Neither offset had any upper bound at all, because neither was ever
        // assigned to; an unbounded one scrolls into blank space for ever.
        let names: Vec<String> = (0..80).map(|i| format!("s{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut a = app_with(&refs);
        for _ in 0..50 {
            a.scroll(Target::List, -10.0);
        }
        let l = a.layout();
        let capacity = scroll_window::capacity(l.list_row, a.list_body(&l).h);
        assert_eq!(a.list_scroll, 80 - capacity);
        assert!(rect_of(&a, Target::Row(id_of(&a, "s79"))).is_some());
    }

    #[test]
    fn a_list_that_fits_does_not_scroll_at_all() {
        let mut a = app_with(&["one", "two"]);
        for _ in 0..10 {
            a.scroll(Target::List, -10.0);
        }
        assert_eq!(a.list_scroll, 0);
    }

    #[test]
    fn the_wheel_banks_the_fractions_a_trackpad_sends() {
        // A tenth of a notch rounds to no rows, but ten of them are a notch,
        // and a wheel that dropped each one would never move under a finger.
        let names: Vec<String> = (0..80).map(|i| format!("s{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut a = app_with(&refs);
        for _ in 0..10 {
            a.scroll(Target::List, -0.1);
        }
        assert!(a.list_scroll > 0, "ten tenths of a notch moved nothing");
    }

    #[test]
    fn the_wheel_over_nothing_scrolls_nothing() {
        let mut a = app();
        assert_eq!(a.scroll(Target::New, -3.0), EventResult::Ignored);
    }

    #[test]
    fn picking_a_snippet_starts_it_at_its_first_line() {
        let mut a = app_with(&["a", "b"]);
        let body = numbered_lines(400);
        a.snippets[0].content = body.clone();
        a.snippets[1].content = body;
        a.select(id_of(&a, "a"));
        a.scroll(Target::Code, -20.0);
        assert!(a.code_scroll > 0);
        a.select(id_of(&a, "b"));
        assert_eq!(a.code_scroll, 0);
    }

    // ── What is drawn ───────────────────────────────────────────────────

    /// Every string the app draws, with the box it was told to stop at.
    ///
    /// `texts` answers "what does it say"; this answers "where, in what
    /// colour, and how wide", which is what the elision and the colouring
    /// tests need and what a bare string cannot tell them.
    fn drawn(a: &App, size: (f32, f32)) -> Vec<(String, f32, f32, f32, Color)> {
        a.frame(size.0, size.1)
            .into_tree()
            .commands
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    text,
                    x,
                    y,
                    max_width,
                    color,
                    ..
                } => Some((text, x, y, max_width.unwrap_or(f32::INFINITY), color)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn every_string_is_drawn_with_a_width_to_stop_at() {
        // A `max_width` of `None` is a title that runs out of its panel and
        // over the one beside it. Every label in this program goes through
        // `push_text`, which is the one place the limit is set, so this
        // asserts that no drawing site has since grown its own way round it.
        let mut a = app();
        a.show_stats = true;
        for (text, _, _, limit, _) in drawn(&a, W) {
            assert!(
                limit.is_finite() && limit > 0.0,
                "{text:?} is drawn with no width to stop at"
            );
        }
    }

    #[test]
    fn nothing_is_drawn_off_the_right_edge() {
        // The limit is where the renderer stops, so `x + limit` is the
        // rightmost pixel a string can reach. Rounding in the layout can put
        // it a hair over; a whole character over is a label in the next
        // column.
        let mut a = app();
        a.show_stats = true;
        for (text, x, _, limit, _) in drawn(&a, W) {
            assert!(
                x + limit <= W.0 + 1.0,
                "{text:?} reaches {} in a {} window",
                x + limit,
                W.0
            );
        }
    }

    #[test]
    fn the_status_line_says_how_long_the_selected_snippet_is() {
        let mut a = app_with(&["one"]);
        let id = id_of(&a, "one");
        a.snippets[0].content = "a\nb\nc".to_string();
        a.select(id);
        assert!(shows(&a, "3 lines"), "{:?}", texts(&a, W));
    }

    #[test]
    fn with_nothing_selected_the_status_line_counts_nothing() {
        let a = app_with(&["one"]);
        assert!(a.selected_snippet_id.is_none());
        assert!(!texts(&a, W).iter().any(|t| t.ends_with(" lines")));
    }

    #[test]
    fn an_export_that_worked_says_so_in_green() {
        let dir = std::env::temp_dir().join("snippets-export-ok");
        std::fs::create_dir_all(&dir).unwrap();
        let mut a = app_with(&["one"]);
        a.export_path = dir.join("out.json");
        click(&mut a, Target::Export);
        let note = drawn(&a, W)
            .into_iter()
            .find(|(t, ..)| t.starts_with("Exported "))
            .unwrap_or_else(|| panic!("no export note in {:?}", texts(&a, W)));
        assert_eq!(note.4, GREEN);
        assert!(a.export_path.exists());
        let _ = std::fs::remove_file(&a.export_path);
    }

    #[test]
    fn an_export_that_failed_says_so_in_red() {
        // The note used to be a bare `String` set on both paths, so a write
        // that failed reported the same cheerful "Exported 6 snippets" as one
        // that worked — the `Result` was dropped with a `let _ =`.
        let mut a = app_with(&["one"]);
        a.export_path = std::env::temp_dir()
            .join("snippets-no-such-directory")
            .join("out.json");
        click(&mut a, Target::Export);
        let note = drawn(&a, W)
            .into_iter()
            .find(|(t, ..)| t.starts_with("Could not write "))
            .unwrap_or_else(|| panic!("no failure note in {:?}", texts(&a, W)));
        assert_eq!(note.4, RED);
        assert!(!a.export_path.exists());
    }

    #[test]
    fn the_export_note_takes_the_line_the_tags_were_on() {
        // Both want the status line. The note is news and the tags are not,
        // so while there is a note the tags stand down — and a test that only
        // asked "is the note drawn" would pass with the two overprinted.
        //
        // Asked of the status line's own rectangle, not of the whole window:
        // the list row draws the same tag, so `shows(&a, "#alpha")` answers
        // "somewhere" and would be true either way.
        let mut a = app_with(&["one"]);
        let id = id_of(&a, "one");
        a.snippets[0].tags = vec!["alpha".to_string()];
        a.select(id);
        let status = a.editor_parts(&a.layout()).status;
        let tags_on_the_status_line = |a: &App| {
            drawn(a, W)
                .into_iter()
                .filter(|&(ref t, x, y, ..)| t.starts_with('#') && status.contains(x, y))
                .count()
        };
        assert_eq!(tags_on_the_status_line(&a), 1);
        a.export_path = std::env::temp_dir()
            .join("snippets-no-such-directory")
            .join("out.json");
        click(&mut a, Target::Export);
        assert_eq!(tags_on_the_status_line(&a), 0, "{:?}", texts(&a, W));
    }

    #[test]
    fn a_list_row_shows_no_more_tags_than_fit_on_it() {
        let mut a = app_with(&["one"]);
        a.snippets[0].tags = (0..12).map(|i| format!("t{i}")).collect();
        let row = texts(&a, W)
            .into_iter()
            .find(|t| t.starts_with("#t0"))
            .unwrap_or_else(|| panic!("the row drew no tags"));
        assert_eq!(row.split_whitespace().count(), TAGS_ON_A_ROW);
    }

    #[test]
    fn the_overlay_lists_every_statistic() {
        // The names are written out here rather than read back from
        // `stat_rows`. Walking the same list the drawing walks asks the code
        // whether it drew what it drew, and answers yes however many rows have
        // been dropped from it — a statistic could be deleted outright and
        // this test would only check the ones that were left.
        const WANTED: [&str; 7] = [
            "Snippets",
            "Folders",
            "Tags",
            "Favorites",
            "Templates",
            "Total Lines",
            "Total Size",
        ];
        let mut a = app();
        a.show_stats = true;
        let seen = texts(&a, W);
        for name in WANTED {
            assert!(seen.contains(&name.to_string()), "{name} is not on it");
        }
        let stats = a.stats();
        let rows = a.stat_rows(&stats);
        assert_eq!(
            rows.len(),
            WANTED.len(),
            "the overlay has gained or lost a statistic: {rows:?}"
        );
        for (name, value) in rows {
            assert!(seen.contains(&value), "{name}'s value {value} is not on it");
        }
    }

    #[test]
    fn the_overlay_lists_no_more_languages_than_it_has_room_for() {
        // Every language in use, so the cap has something to cut. The starting
        // library uses fewer languages than the overlay has room for, and
        // against that library the cap and no cap at all draw the same list.
        let mut a = app();
        for (i, lang) in Language::all().iter().enumerate() {
            a.create_snippet(&format!("s{i}"), "x", *lang)
                .expect("a named snippet is made");
        }
        assert!(
            a.languages_in_use().len() > LANGUAGES_ON_OVERLAY,
            "the fixture leaves the cap nothing to do: {} languages in use",
            a.languages_in_use().len()
        );
        a.show_stats = true;
        let counted = texts(&a, W)
            .iter()
            .filter(|t| {
                Language::all()
                    .iter()
                    .any(|l| t.starts_with(&format!("{}: ", l.name())))
            })
            .count();
        assert!(
            counted <= LANGUAGES_ON_OVERLAY,
            "{counted} languages on an overlay sized for {LANGUAGES_ON_OVERLAY}"
        );
        assert!(counted > 0, "the overlay named no languages at all");
    }

    #[test]
    fn the_overlay_fits_in_the_window_it_covers() {
        // Sized from what it holds, then held to the window — it used to be
        // 400x300 whatever it was in, so in a 320-wide window it hung off
        // both edges.
        let mut a = app();
        a.show_stats = true;
        for size in [(320.0, 240.0), W, (2000.0, 1400.0)] {
            a.resize(size.0, size.1);
            for (text, x, y, limit, _) in drawn(&a, size) {
                assert!(
                    x >= -1.0 && y >= -1.0 && x + limit <= size.0 + 1.0,
                    "{text:?} at ({x}, {y}) +{limit} in a {size:?} window"
                );
            }
        }
    }

    #[test]
    fn an_empty_list_says_so_rather_than_showing_nothing() {
        let a = app_with(&[]);
        assert!(shows(&a, EMPTY_LIST));
    }

    #[test]
    fn an_empty_editor_says_what_to_do_next() {
        let a = app_with(&["one"]);
        assert!(a.selected_snippet_id.is_none());
        assert!(shows(&a, EMPTY_HEADLINE));
        assert!(shows(&a, EMPTY_SUBLINE));
    }

    #[test]
    fn the_empty_editor_gives_way_to_a_snippet() {
        let mut a = app_with(&["one"]);
        click_matching(&mut a, |t| matches!(t, Target::Row(_)), "a row");
        assert!(!shows(&a, EMPTY_HEADLINE));
        assert!(shows(&a, "one"));
    }

    #[test]
    fn the_template_badge_is_only_on_a_template() {
        let mut a = app_with(&["plain", "shaped"]);
        let plain = id_of(&a, "plain");
        let shaped = id_of(&a, "shaped");
        a.snippets[1].is_template = true;
        a.select(plain);
        assert!(!shows(&a, TEMPLATE_LABEL));
        a.select(shaped);
        assert!(shows(&a, TEMPLATE_LABEL));
    }

    #[test]
    fn the_editor_header_names_the_language_and_the_extension_it_is_guessed_from() {
        let mut a = app_with(&["one"]);
        let id = id_of(&a, "one");
        a.snippets[0].language = Language::Rust;
        a.select(id);
        assert!(shows(&a, "Rust .rs"), "{:?}", texts(&a, W));
    }

    #[test]
    fn the_gutter_numbers_the_lines_from_one() {
        let mut a = app_with(&["one"]);
        let id = id_of(&a, "one");
        a.snippets[0].content = "a\nb\nc".to_string();
        a.select(id);
        let seen = texts(&a, W);
        for n in 1..=3 {
            assert!(seen.contains(&n.to_string()), "line {n} is unnumbered");
        }
        assert!(!seen.contains(&"4".to_string()), "a fourth line was drawn");
    }

    // ── The window ──────────────────────────────────────────────────────

    #[test]
    fn a_resize_is_what_the_next_click_is_read_against() {
        // The model holds a size for exactly one reason: a click arrives as a
        // point with no window attached. If the resize does not reach it, every
        // click after the first drag is read against the old geometry.
        let mut a = app();
        handle_event(
            &mut a,
            &Event::Resize {
                width: 1600,
                height: 900,
            },
        );
        assert_eq!(a.size, (1600.0, 900.0));
        let l = a.layout();
        assert_eq!(l.window, Rect::new(0.0, 0.0, 1600.0, 900.0));
    }

    #[test]
    fn a_click_lands_where_the_window_it_was_resized_to_put_the_control() {
        // Not just "the number was stored": the hit box has to move with it.
        // A test that only checked `a.size` would pass with `frame` still
        // drawing at the old size.
        //
        // The control has to be one that really travels. This asked the Stats
        // button, which is laid out from the *left* edge and shifts only by
        // the padding between these two window sizes — a few pixels, less than
        // its own width. A click at its wide position still landed inside its
        // narrow box, so the test passed with the resize thrown away. The
        // search box is measured from the right edge and moves by nearly the
        // whole difference, which is what makes the two sizes tell apart.
        let mut a = app();
        handle_event(
            &mut a,
            &Event::Resize {
                width: 1600,
                height: 900,
            },
        );
        let wide = a
            .frame(1600.0, 900.0)
            .rect_of(|t| *t == Target::Search)
            .expect("the wide window has a search box");
        let narrow = a
            .frame(W.0, W.1)
            .rect_of(|t| *t == Target::Search)
            .expect("the starting window has a search box");
        assert!(
            wide.x >= narrow.right(),
            "the two boxes overlap, so a click cannot tell them apart: \
             {wide:?} against {narrow:?}"
        );
        let (cx, cy) = wide.centre();
        assert_eq!(
            a.handle_mouse(&MouseEvent {
                x: cx,
                y: cy,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
            EventResult::Consumed
        );
        assert!(a.search_focus, "the click did not reach the search box");
    }

    #[test]
    fn the_window_forwards_the_events_it_has_a_use_for() {
        let mut a = app_with(&["one"]);
        assert_eq!(
            handle_event(&mut a, &Event::Key(press(Key::Down))),
            EventResult::Consumed
        );
        assert_eq!(selected_title(&a).as_deref(), Some("one"));
    }

    #[test]
    fn the_window_ignores_the_events_it_has_no_use_for() {
        let mut a = app();
        assert_eq!(
            handle_event(&mut a, &Event::Tick { elapsed_ms: 16 }),
            EventResult::Ignored
        );
        assert_eq!(
            handle_event(&mut a, &Event::CloseRequested),
            EventResult::Ignored
        );
    }

    #[test]
    fn the_close_button_ends_the_program() {
        // `Ignored` from `handle_event` and `Exit` from the window are not in
        // conflict: the model has nothing to do with a close, and the window
        // has everything to do with it.
        let mut a = app();
        assert_eq!(
            WindowApp::on_event(&mut a, &Event::CloseRequested),
            Response::Exit
        );
    }

    #[test]
    fn a_keystroke_that_changes_nothing_does_not_ask_for_a_redraw() {
        let mut a = app_with(&[]);
        assert_eq!(
            WindowApp::on_event(&mut a, &Event::Key(press(Key::Down))),
            Response::Idle
        );
        assert_eq!(
            WindowApp::on_event(&mut a, &Event::Key(press(Key::S))),
            Response::Redraw
        );
    }

    #[test]
    fn rendering_lays_the_frame_out_at_the_size_it_is_given() {
        // `render` resizes before it draws, which is what keeps a window that
        // was never sent a `Resize` — the first frame — from being drawn at
        // the default and clicked at the real size.
        let mut a = app();
        let tree = WindowApp::render(&mut a, 1600.0, 900.0);
        assert_eq!(a.size, (1600.0, 900.0));
        assert!(!tree.commands.is_empty());
    }

    #[test]
    fn the_window_is_named_and_identified() {
        let a = app();
        assert!(!WindowApp::title(&a).is_empty());
        assert!(!WindowApp::app_id(&a).is_empty());
        let (w, h) = WindowApp::initial_size(&a);
        assert_eq!(f32_from_u32(w), WINDOW_WIDTH);
        assert_eq!(f32_from_u32(h), WINDOW_HEIGHT);
    }

    #[test]
    fn nothing_here_animates_so_nothing_here_ticks() {
        let a = app();
        assert!(WindowApp::tick_interval(&a).is_none());
    }

    // ── Every control, once ─────────────────────────────────────────────

    #[test]
    fn every_kind_of_control_is_on_the_first_screen() {
        // The list is written out rather than derived, so a control that
        // stops being drawn fails here instead of quietly leaving the
        // program. `Target` variants that only exist in a state the opening
        // screen is not in are named in the second list, with the state.
        let a = app();
        let seen: Vec<String> = control_names(&a);
        for want in [
            "New",
            "Export",
            "Stats",
            "Sort",
            "Scope",
            "View",
            "Search",
            "Row",
            "Star",
            "Folder",
            "Twisty",
            "NewFolder",
            "List",
        ] {
            assert!(
                seen.iter().any(|s| s == want),
                "{want} is not on the opening screen; it drew {seen:?}"
            );
        }
    }

    #[test]
    fn the_controls_that_need_a_state_appear_in_that_state() {
        let mut a = app();
        assert!(!control_names(&a).iter().any(|s| s == "CloseStats"));
        a.show_stats = true;
        assert!(control_names(&a).iter().any(|s| s == "CloseStats"));

        // The sidebar shows one list at a time, so the tag and language rows
        // exist only in the view that is theirs.
        let mut a = app();
        assert!(!control_names(&a).iter().any(|s| s == "Tag"));
        a.sidebar_view = SidebarView::Tags;
        assert!(control_names(&a).iter().any(|s| s == "Tag"));
        a.sidebar_view = SidebarView::Languages;
        let seen = control_names(&a);
        assert!(seen.iter().any(|s| s == "Lang"));
        assert!(!seen.iter().any(|s| s == "Tag"));

        let mut a = app_with(&["one"]);
        assert!(!control_names(&a).iter().any(|s| s == "Use"));
        click_matching(&mut a, |t| matches!(t, Target::Row(_)), "a row");
        let seen = control_names(&a);
        assert!(seen.iter().any(|s| s == "Use"));
        assert!(seen.iter().any(|s| s == "Delete"));
        // The folder cross belongs to the *selected* folder, and picking a
        // snippet does not select one.
        assert!(!seen.iter().any(|s| s == "DeleteFolder"));
    }

    #[test]
    fn every_control_drawn_can_actually_be_clicked() {
        // Overlapping boxes are not wrong in themselves — the last one wins,
        // which is how the star sits inside its row and how the overlay
        // dismisses — but a box that resolves to *nothing but* another
        // target at its own centre is a control the user cannot reach. That
        // is the question worth asking, not "do any two rectangles touch".
        //
        // `List` and `Code` are the exceptions and are meant to be: they are
        // whole panels recorded so the wheel has something to aim at, and the
        // rows and lines are drawn over them on purpose.
        let a = app();
        let frame = a.frame(W.0, W.1);
        for &(target, rect) in frame
            .hits()
            .iter()
            .filter(|(t, _)| !matches!(t, Target::List | Target::Code))
        {
            let (cx, cy) = rect.centre();
            let reached = frame.hit_test(cx, cy);
            assert_eq!(
                reached,
                Some(target),
                "{target:?} at {rect:?} is covered by {reached:?}"
            );
        }
    }

    #[test]
    fn there_is_somewhere_to_click_that_is_not_a_control() {
        // `click_background` needs one, and so does a user who wants to
        // deselect. A layout with no gap at all would make every test that
        // clicks "nothing" silently click something.
        let a = app();
        assert!(bare_point(&a, W).is_some());
    }
}
