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
//! **Eighteen blanket `#![allow(...)]` sat at the top of the file**,
//! `dead_code` among them — which is what let a program whose `main` discards
//! its own render compile without a word of complaint, along with the six
//! `edit_*` fields and the `editing` flag of an editor that could not be
//! entered, and `apply_template`, which nothing but a test has ever called.

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

/// How deep the folder tree is walked before it stops.
///
/// A folder's parent is an id, and nothing stops a cycle being built out of
/// two ids that name each other, so the walk needs an end that does not
/// depend on the data being sound.
const MAX_FOLDER_DEPTH: usize = 8;

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
    fn move_selection(&mut self, delta: isize) -> EventResult {
        let ids = self.filtered_ids();
        let Some(last) = ids.len().checked_sub(1) else {
            return EventResult::Ignored;
        };
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
            Key::Home => self.move_selection(isize::MIN),
            Key::End => self.move_selection(isize::MAX),
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
        // A folder that is its own ancestor would walk for ever. Nothing here
        // builds one today, but `parent_id` is a plain field and the recursion
        // is the one place a cycle turns into a hang rather than a wrong
        // answer.
        if depth >= MAX_FOLDER_DEPTH {
            return;
        }
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
fn label_centred(f: &mut Frame<Target>, l: &Label, r: Rect) {
    if r.is_empty() {
        return;
    }
    let w = text::measure(l.text, l.size, l.weight).min(r.w);
    let lh = text::line_height(l.size, l.weight);
    push_text(f, l, r.x + (r.w - w) / 2.0, r.y + (r.h - lh) / 2.0, r.w);
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
}

fn main() -> ExitCode {
    let mut app = App::new();
    app::launch("snippets", &mut app)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    // --- Language tests ---

    #[test]
    fn test_language_name() {
        assert_eq!(Language::Rust.name(), "Rust");
        assert_eq!(Language::Python.name(), "Python");
        assert_eq!(Language::PlainText.name(), "Plain Text");
    }

    #[test]
    fn test_language_extension() {
        assert_eq!(Language::Rust.extension(), "rs");
        assert_eq!(Language::Python.extension(), "py");
        assert_eq!(Language::JavaScript.extension(), "js");
    }

    #[test]
    fn test_language_from_extension() {
        assert_eq!(Language::from_extension("rs"), Language::Rust);
        assert_eq!(Language::from_extension("py"), Language::Python);
        assert_eq!(Language::from_extension("ts"), Language::TypeScript);
        assert_eq!(Language::from_extension("unknown"), Language::PlainText);
    }

    #[test]
    fn test_language_detect_rust() {
        let content = "fn main() {\n    let x = 5;\n    println!(\"hello\");\n}";
        assert_eq!(Language::detect_from_content(content), Language::Rust);
    }

    #[test]
    fn test_language_detect_python() {
        let content = "import os\ndef hello():\n    print('hello')";
        assert_eq!(Language::detect_from_content(content), Language::Python);
    }

    #[test]
    fn test_language_detect_python_shebang() {
        let content = "#!/usr/bin/env python3\nimport sys";
        assert_eq!(Language::detect_from_content(content), Language::Python);
    }

    #[test]
    fn test_language_detect_sql() {
        let content = "SELECT * FROM users WHERE id = 1";
        assert_eq!(Language::detect_from_content(content), Language::Sql);
    }

    #[test]
    fn test_language_detect_html() {
        let content = "<!DOCTYPE html>\n<html><head></head></html>";
        assert_eq!(Language::detect_from_content(content), Language::Html);
    }

    #[test]
    fn test_language_keywords_not_empty() {
        assert!(!Language::Rust.keywords().is_empty());
        assert!(!Language::Python.keywords().is_empty());
        assert!(Language::PlainText.keywords().is_empty());
    }

    #[test]
    fn test_language_all() {
        let all = Language::all();
        assert!(all.len() >= 12);
        assert!(all.contains(&Language::Rust));
        assert!(all.contains(&Language::PlainText));
    }

    // --- Tokenizer tests ---

    #[test]
    fn test_tokenize_empty() {
        let result = tokenize("", Language::PlainText);
        assert_eq!(result.len(), 1); // one empty line
    }

    #[test]
    fn test_tokenize_keyword() {
        let result = tokenize("fn main", Language::Rust);
        assert_eq!(result.len(), 1);
        assert!(
            result[0]
                .iter()
                .any(|t| t.kind == TokenKind::Keyword && t.text == "fn")
        );
    }

    #[test]
    fn test_tokenize_string() {
        let result = tokenize("let x = \"hello\"", Language::Rust);
        assert!(result[0].iter().any(|t| t.kind == TokenKind::String));
    }

    #[test]
    fn test_tokenize_number() {
        let result = tokenize("let x = 42", Language::Rust);
        assert!(
            result[0]
                .iter()
                .any(|t| t.kind == TokenKind::Number && t.text == "42")
        );
    }

    #[test]
    fn test_tokenize_comment() {
        let result = tokenize("// this is a comment", Language::Rust);
        assert!(result[0].iter().any(|t| t.kind == TokenKind::Comment));
    }

    #[test]
    fn test_tokenize_python_comment() {
        let result = tokenize("# python comment", Language::Python);
        assert!(result[0].iter().any(|t| t.kind == TokenKind::Comment));
    }

    #[test]
    fn test_tokenize_sql_comment() {
        let result = tokenize("-- sql comment", Language::Sql);
        assert!(result[0].iter().any(|t| t.kind == TokenKind::Comment));
    }

    #[test]
    fn test_tokenize_operator() {
        let result = tokenize("x + y", Language::Rust);
        assert!(result[0].iter().any(|t| t.kind == TokenKind::Operator));
    }

    #[test]
    fn test_tokenize_multiline() {
        let result = tokenize("fn main() {\n    println!(\"hello\");\n}", Language::Rust);
        assert_eq!(result.len(), 3);
    }

    // --- Search tests ---

    #[test]
    fn test_search_empty_query() {
        let snippets = vec![make_test_snippet(1, "Hello", "world", Language::Rust)];
        let results = search_snippets(&snippets, "", SearchScope::All);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_by_title() {
        let snippets = vec![
            make_test_snippet(1, "Hello World", "content", Language::Rust),
            make_test_snippet(2, "Goodbye", "other", Language::Python),
        ];
        let results = search_snippets(&snippets, "hello", SearchScope::Title);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_by_content() {
        let snippets = vec![
            make_test_snippet(1, "Test", "fn main() {}", Language::Rust),
            make_test_snippet(2, "Other", "print hello", Language::Python),
        ];
        let results = search_snippets(&snippets, "main", SearchScope::Content);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_case_insensitive() {
        let snippets = vec![make_test_snippet(1, "RUST Code", "content", Language::Rust)];
        let results = search_snippets(&snippets, "rust", SearchScope::All);
        assert_eq!(results.len(), 1);
    }

    // --- Template tests ---

    #[test]
    fn test_extract_template_vars() {
        let content = "fn ${name}(${params}) -> ${ret} {}";
        let vars = extract_template_vars(content);
        assert_eq!(vars.len(), 3);
        assert!(vars.contains(&"name".to_string()));
        assert!(vars.contains(&"params".to_string()));
        assert!(vars.contains(&"ret".to_string()));
    }

    #[test]
    fn test_extract_no_vars() {
        let vars = extract_template_vars("fn main() {}");
        assert!(vars.is_empty());
    }

    #[test]
    fn test_extract_duplicate_vars() {
        let vars = extract_template_vars("${x} and ${x} again");
        assert_eq!(vars.len(), 1);
    }

    #[test]
    fn test_apply_template() {
        let content = "Hello ${name}, you are ${age}";
        let vars = vec![
            ("name".to_string(), "Alice".to_string()),
            ("age".to_string(), "30".to_string()),
        ];
        let result = apply_template(content, &vars);
        assert_eq!(result, "Hello Alice, you are 30");
    }

    // --- Export tests ---

    #[test]
    fn test_export_json() {
        let snippets = vec![make_test_snippet(1, "Test", "fn main() {}", Language::Rust)];
        let json = export_snippets_json(&snippets);
        assert!(json.contains("\"title\""));
        assert!(json.contains("Test"));
        assert!(json.contains("Rust"));
    }

    #[test]
    fn test_json_escape() {
        assert_eq!(json_escape("hello"), "\"hello\"");
        assert_eq!(json_escape("he\"llo"), "\"he\\\"llo\"");
        assert_eq!(json_escape("line1\nline2"), "\"line1\\nline2\"");
    }

    // --- App state tests ---

    #[test]
    fn test_app_new() {
        let app = App::new();
        assert!(!app.snippets.is_empty()); // has sample snippets
        assert!(!app.folders.is_empty()); // has default folders
    }

    #[test]
    fn test_app_create_snippet() {
        let mut app = App::new();
        let initial = app.snippets.len();
        let id = app.create_snippet("Test", "fn test() {}", Language::Rust);
        assert!(id > 0);
        assert_eq!(app.snippets.len(), initial + 1);
    }

    #[test]
    fn test_app_delete_snippet() {
        let mut app = App::new();
        let id = app.create_snippet("Delete Me", "content", Language::PlainText);
        let count = app.snippets.len();
        app.delete_snippet(id);
        assert_eq!(app.snippets.len(), count - 1);
    }

    #[test]
    fn test_app_create_folder() {
        let mut app = App::new();
        let initial = app.folders.len();
        let id = app.create_folder("New Folder");
        assert!(id > 0);
        assert_eq!(app.folders.len(), initial + 1);
    }

    #[test]
    fn test_app_delete_folder() {
        let mut app = App::new();
        let id = app.create_folder("To Delete");
        let count = app.folders.len();
        app.delete_folder(id);
        assert_eq!(app.folders.len(), count - 1);
    }

    #[test]
    fn test_app_toggle_favorite() {
        let mut app = App::new();
        let id = app.create_snippet("Test", "content", Language::PlainText);
        assert!(!app.snippets.iter().find(|s| s.id == id).unwrap().favorite);
        app.toggle_favorite(id);
        assert!(app.snippets.iter().find(|s| s.id == id).unwrap().favorite);
        app.toggle_favorite(id);
        assert!(!app.snippets.iter().find(|s| s.id == id).unwrap().favorite);
    }

    #[test]
    fn test_app_use_snippet() {
        let mut app = App::new();
        let id = app.create_snippet("Test", "content", Language::PlainText);
        assert_eq!(
            app.snippets.iter().find(|s| s.id == id).unwrap().use_count,
            0
        );
        app.use_snippet(id);
        assert_eq!(
            app.snippets.iter().find(|s| s.id == id).unwrap().use_count,
            1
        );
        assert_eq!(app.recently_used[0], id);
    }

    #[test]
    fn test_app_filtered_snippets_all() {
        let app = App::new();
        let filtered = app.filtered_snippets();
        assert!(!filtered.is_empty());
    }

    #[test]
    fn test_app_filtered_favorites() {
        let mut app = App::new();
        app.sidebar_view = SidebarView::Favorites;
        let filtered = app.filtered_snippets();
        assert!(filtered.iter().all(|s| s.favorite));
    }

    #[test]
    fn test_app_filtered_templates() {
        let mut app = App::new();
        app.sidebar_view = SidebarView::Templates;
        let filtered = app.filtered_snippets();
        assert!(filtered.iter().all(|s| s.is_template));
    }

    #[test]
    fn test_app_all_tags() {
        let app = App::new();
        let tags = app.all_tags();
        assert!(!tags.is_empty());
    }

    #[test]
    fn test_app_stats() {
        let app = App::new();
        let stats = app.stats();
        assert!(stats.total_snippets > 0);
        assert!(stats.total_folders > 0);
    }

    #[test]
    fn test_app_render() {
        let app = App::new();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_with_selection() {
        let mut app = App::new();
        app.selected_snippet_id = Some(app.snippets[0].id);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_stats_overlay() {
        let mut app = App::new();
        app.show_stats = true;
        let cmds = app.render();
        assert!(cmds.len() > 20); // Overlay adds many commands
    }

    #[test]
    fn test_app_create_empty_folder_rejected() {
        let mut app = App::new();
        let initial = app.folders.len();
        app.create_folder("");
        assert_eq!(app.folders.len(), initial);
    }

    #[test]
    fn test_app_create_large_snippet_rejected() {
        let mut app = App::new();
        let large = "x".repeat(MAX_CONTENT_LEN + 1);
        let id = app.create_snippet("Big", &large, Language::PlainText);
        assert_eq!(id, 0);
    }

    // --- Utility tests ---

    /// A snippet title is cut to the width of the list column it is drawn in.
    /// The old helper compared `s.len()` — bytes — against a budget of 32
    /// "characters", so an accented title was cut short while it still fitted
    /// the column, and a title of wide glyphs ran past it.
    #[test]
    fn a_long_title_is_cut_to_the_list_column() {
        let room = LIST_WIDTH - 20.0;
        for title in [
            "a snippet title far too long to fit the list column beside it",
            "un titre de fragment beaucoup trop long pour la colonne de gauche",
        ] {
            let out = text::elide(title, room, "...", NORMAL_TEXT, FontWeightHint::Bold);
            let w = text::measure(&out, NORMAL_TEXT, FontWeightHint::Bold);
            assert!(w <= room + 0.01, "{out:?} is {w} px in {room} px of room");
            assert!(out.ends_with("..."), "a cut title should say so");
        }
    }

    #[test]
    fn a_short_title_is_left_alone() {
        let title = "Quick sort";
        let out = text::elide(
            title,
            LIST_WIDTH - 20.0,
            "...",
            NORMAL_TEXT,
            FontWeightHint::Bold,
        );
        assert_eq!(out, title);
    }

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(500), "500 B");
    }

    #[test]
    fn test_format_size_kb() {
        let result = format_size(2048);
        assert!(result.contains("KiB"));
    }

    #[test]
    fn test_format_size_mb() {
        let result = format_size(2 * 1024 * 1024);
        assert!(result.contains("MiB"));
    }

    #[test]
    fn test_sidebar_view_label() {
        assert_eq!(SidebarView::Folders.label(), "Folders");
        assert_eq!(SidebarView::Tags.label(), "Tags");
    }

    #[test]
    fn test_sort_order_label() {
        assert_eq!(SortOrder::NameAsc.label(), "Name A-Z");
        assert_eq!(SortOrder::DateDesc.label(), "Newest");
    }

    #[test]
    fn test_search_scope_label() {
        assert_eq!(SearchScope::All.label(), "All");
        assert_eq!(SearchScope::Content.label(), "Content");
    }

    #[test]
    fn test_token_kind_color() {
        // Just verify colors are assigned
        let _ = TokenKind::Keyword.color();
        let _ = TokenKind::String.color();
        let _ = TokenKind::Comment.color();
    }

    #[test]
    fn test_compute_stats() {
        let snippets = vec![
            make_test_snippet(1, "A", "content", Language::Rust),
            make_test_snippet(2, "B", "content", Language::Python),
        ];
        let folders = vec![];
        let stats = compute_stats(&snippets, &folders);
        assert_eq!(stats.total_snippets, 2);
        assert_eq!(stats.by_language.len(), 2);
    }

    // --- Helper ---

    fn make_test_snippet(id: u64, title: &str, content: &str, lang: Language) -> Snippet {
        Snippet {
            id,
            title: title.into(),
            content: content.into(),
            language: lang,
            folder_id: None,
            tags: vec!["test".into()],
            favorite: false,
            created_at: id,
            use_count: 0,
            description: String::new(),
            is_template: false,
            template_vars: Vec::new(),
        }
    }

    // --- Text measurement ---

    /// The width of one mono cell. Production code no longer has this idea —
    /// the pen measures — but the tests below still compare against it to say
    /// what "a grid" would have meant, and why assuming one was survivable for
    /// so long.
    fn cell() -> f32 {
        text::cell_advance(NORMAL_TEXT, FontWeightHint::Regular)
    }

    /// A tab is the concrete bug the nominal cell count had, and it is not
    /// exotic: it is how most of the source anyone would paste into a snippet
    /// is indented.
    ///
    /// A tab is one `char`, so the old pen advanced one cell for it — while the
    /// face draws it four cells wide. Every token on a tab-indented line was
    /// therefore drawn three cells left of where the indentation actually
    /// ended, i.e. *on top of* the whitespace it was supposed to follow, and
    /// the further in the code was nested the worse it got. Measured on the
    /// built-in mono face at 14 px: drawn 33.6 px against a nominal 8.4 px.
    #[test]
    fn a_tab_advances_by_the_width_it_is_drawn_at_not_by_one_cell() {
        let drawn = text::measure_in("\t", NORMAL_TEXT, FontWeightHint::Regular, FontFamily::Mono);
        assert!(
            drawn > cell() * 1.5,
            "a tab drawn {drawn} against a {} cell — if a tab really is one \
             cell wide on this face, this test has stopped testing anything",
            cell()
        );
    }

    /// The claim that replaced the cell count: wherever the pen stops is
    /// exactly where the token before it finished being drawn — by
    /// construction, since it is the same measurement the renderer makes.
    ///
    /// Stated over the kinds of text a cell count gets wrong: a tab, an
    /// ideograph the face renders wide or substitutes, a combining mark that
    /// advances nothing. Plain Latin tokens are included to show where the two
    /// answers *do* agree, which is why the old code survived review.
    #[test]
    fn the_pen_advances_by_what_is_drawn() {
        for token in ["let", "  ", "héllo", "日本語", "e\u{0301}", "\t", "x"] {
            for weight in [FontWeightHint::Regular, FontWeightHint::Bold] {
                let drawn = text::measure_in(token, NORMAL_TEXT, weight, FontFamily::Mono);
                assert!(
                    drawn >= 0.0,
                    "{token:?} measures negative, which would step the pen backwards"
                );
                // Concatenation is what the pen actually does: it draws one
                // token, advances, draws the next. If measuring a run were not
                // additive the panel would drift across a line no matter which
                // width the pen used, so this is the property the fix rests on.
                let joined = format!("{token}{token}");
                let both = text::measure_in(&joined, NORMAL_TEXT, weight, FontFamily::Mono);
                assert!(
                    (both - drawn * 2.0).abs() < 0.01,
                    "{token:?} twice measures {both}, but two pen steps land at {}",
                    drawn * 2.0
                );
            }
        }
        // And where a cell count and the truth diverge, the pen follows the
        // truth: one `char`, four cells.
        assert!(
            text::measure_in("\t", NORMAL_TEXT, FontWeightHint::Regular, FontFamily::Mono)
                > cell() * 1.5
        );
    }

    /// The cell comes from the face, so it stays true if the face changes.
    #[test]
    fn the_code_cell_is_derived_from_the_face() {
        let cell = cell();
        assert!(cell > 0.0, "an empty cell would collapse the code panel");
        assert!(
            cell <= NORMAL_TEXT,
            "a cell wider than the em box would space the code out absurdly"
        );
    }

    /// Why the old cell count looked correct for so long: for Latin source in
    /// the mono face, a character really does fit a cell. This is the premise
    /// that failed silently for everything else.
    #[test]
    fn a_code_character_fits_a_cell() {
        let cell = cell();
        for ch in ['0', 'W', 'i', '#', 'é', 'M', '@', '_', '{', ' '] {
            let w = text::measure_in(
                &ch.to_string(),
                NORMAL_TEXT,
                FontWeightHint::Regular,
                FontFamily::Mono,
            );
            assert!(w <= cell + 0.01, "{ch:?} measures {w} in a {cell} cell");
        }
    }

    /// Keywords are drawn bold and measured bold — the pen asks for the same
    /// weight it draws in, so this no longer has to hold for the layout to be
    /// correct. It is kept because a face where bold advanced differently would
    /// make the panel's columns stop lining up between a keyword line and a
    /// plain one, which is a legibility claim rather than a positioning one.
    #[test]
    fn a_bold_keyword_character_fits_the_same_cell() {
        let cell = cell();
        for ch in ['0', 'W', 'M', 'f', 'n'] {
            let w = text::measure_in(
                &ch.to_string(),
                NORMAL_TEXT,
                FontWeightHint::Bold,
                FontFamily::Mono,
            );
            assert!(
                w <= cell + 0.01,
                "bold {ch:?} measures {w} in a {cell} cell"
            );
        }
    }

    /// The code area is placed on a mono cell, so it must be drawn in the mono
    /// face. Everything around it — toolbar, sidebar, list, tags — must not be.
    #[test]
    fn the_code_area_is_drawn_in_the_family_it_was_measured_in() {
        let mut app = App::new();
        let id = app.create_snippet(
            "Wide",
            "fn main() {\n    let WWWW = iiii;\n}\n",
            Language::Rust,
        );
        app.selected_snippet_id = Some(id);
        let cmds = app.render();

        let mut depth = 0_i32;
        let mut deepest = 0_i32;
        let mut inside = 0_usize;
        for cmd in &cmds {
            match cmd {
                RenderCommand::PushFont { family } => {
                    assert_eq!(family, &FontFamily::Mono, "only the code area pushes");
                    depth += 1;
                    deepest = deepest.max(depth);
                }
                RenderCommand::PopFont => {
                    depth -= 1;
                    assert!(depth >= 0, "a PopFont without a matching PushFont");
                }
                RenderCommand::Text { .. } if depth > 0 => inside += 1,
                _ => {}
            }
        }
        assert_eq!(depth, 0, "the font scopes do not balance");
        assert_eq!(deepest, 1, "the code area's scope was never opened");
        assert!(inside > 0, "no code was drawn inside the mono scope");
    }

    /// Toolbar labels are drawn bold, so they are measured bold — measuring a
    /// bold label at regular weight is exactly how a button overflows.
    #[test]
    fn toolbar_labels_fit_their_buttons() {
        for label in ["+ New", "Import", "Export", "Stats"] {
            let bw = text::measure(label, SMALL_TEXT, FontWeightHint::Bold) + 16.0;
            let drawn = text::measure(label, SMALL_TEXT, FontWeightHint::Bold);
            assert!(drawn + 16.0 <= bw + 0.01, "{label:?} overflows its button");
            assert!(bw > 16.0, "{label:?} produced an empty button");
        }
    }

    /// Every language badge has to fit the pill drawn behind it.
    #[test]
    fn language_badges_fit_their_pills() {
        for lang in Language::all() {
            let name = lang.name();
            let badge_w = text::measure(name, BADGE_TEXT, FontWeightHint::Bold) + 8.0;
            assert!(
                text::measure(name, BADGE_TEXT, FontWeightHint::Bold) + 8.0 <= badge_w + 0.01,
                "{name:?} does not fit its badge"
            );
        }
    }

    /// A tag pill is drawn as `#tag`, so it has to be measured that way. The
    /// old estimate sized it from the bare tag and left the last character
    /// sitting on the pill's rounded edge.
    #[test]
    fn a_tag_pill_is_measured_with_its_hash() {
        let bare = text::measure("rust", BADGE_TEXT, FontWeightHint::Regular);
        let hashed = text::measure("#rust", BADGE_TEXT, FontWeightHint::Regular);
        assert!(hashed > bare, "the hash is drawn, so it has to be measured");
    }
}
