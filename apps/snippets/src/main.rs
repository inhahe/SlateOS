//! `OurOS` Code Snippet Manager
//!
//! A code snippet organizer and manager with:
//! - Snippet storage with language detection and syntax highlighting
//! - Folder/collection organization with nesting
//! - Tag-based categorization and search
//! - Full-text search across all snippets
//! - Language detection from content and file extension
//! - Syntax highlighting for 12 languages
//! - Quick copy to clipboard
//! - Import/export (JSON format)
//! - Favorites and recently used tracking
//! - Template snippets with placeholder variables
//! - Multi-panel UI with sidebar, list, and editor
//!
//! Uses the guitk library for UI rendering.

#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::similar_names)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::unreadable_literal)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::cognitive_complexity)]
#![allow(dead_code)]

use guitk::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

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

const WINDOW_WIDTH: f32 = 1100.0;
const WINDOW_HEIGHT: f32 = 750.0;
const SIDEBAR_WIDTH: f32 = 200.0;
const LIST_WIDTH: f32 = 280.0;
const TOOLBAR_HEIGHT: f32 = 44.0;
const PADDING: f32 = 8.0;
const LINE_HEIGHT: f32 = 20.0;
const CHAR_WIDTH: f32 = 8.0;
const SMALL_TEXT: f32 = 12.0;
const NORMAL_TEXT: f32 = 14.0;
const HEADER_TEXT: f32 = 16.0;
const TITLE_TEXT: f32 = 18.0;

const MAX_SNIPPETS: usize = 5000;
const MAX_FOLDERS: usize = 200;
const MAX_TAGS: usize = 500;
const MAX_CONTENT_LEN: usize = 65536;
const MAX_RECENT: usize = 20;

// ============================================================================
// Language Support
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Language {
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

    while i < chars.len() {
        let c = chars[i];

        // Comment detection
        if c == '/' && chars.get(i.saturating_add(1)) == Some(&'/') {
            let rest: String = chars[i..].iter().collect();
            tokens.push(Token {
                text: rest,
                kind: TokenKind::Comment,
            });
            break;
        }
        if c == '#' && matches!(language, Language::Python | Language::Shell) {
            let rest: String = chars[i..].iter().collect();
            tokens.push(Token {
                text: rest,
                kind: TokenKind::Comment,
            });
            break;
        }
        if c == '-' && chars.get(i.saturating_add(1)) == Some(&'-') && language == Language::Sql {
            let rest: String = chars[i..].iter().collect();
            tokens.push(Token {
                text: rest,
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
            while i < chars.len() {
                let sc = chars[i];
                s.push(sc);
                if sc == '\\' {
                    i = i.saturating_add(1);
                    if i < chars.len() {
                        s.push(chars[i]);
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
            while i < chars.len()
                && (chars[i].is_ascii_digit()
                    || chars[i] == '.'
                    || chars[i] == 'x'
                    || chars[i] == 'b'
                    || (chars[i].is_ascii_hexdigit() && n.contains("0x")))
            {
                n.push(chars[i]);
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
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                ident.push(chars[i]);
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
            if i < chars.len() && "=>&|+-".contains(chars[i]) {
                op.push(chars[i]);
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
    created_at: u64,
    modified_at: u64,
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

fn json_escape(s: &str) -> String {
    use std::fmt::Write as _;
    let mut escaped = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(escaped, "\\u{:04x}", c as u32);
            }
            c => escaped.push(c),
        }
    }
    escaped.push('"');
    escaped
}

// ============================================================================
// Template Processing
// ============================================================================

fn extract_template_vars(content: &str) -> Vec<String> {
    let mut vars = Vec::new();
    let chars: Vec<char> = content.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '$' && chars.get(i.saturating_add(1)) == Some(&'{') {
            i = i.saturating_add(2);
            let mut var = String::new();
            while i < chars.len() && chars[i] != '}' {
                var.push(chars[i]);
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

struct LibraryStats {
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
enum SidebarView {
    Folders,
    Tags,
    Languages,
    Favorites,
    Recent,
    Templates,
}

impl SidebarView {
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
}

struct App {
    // Data
    snippets: Vec<Snippet>,
    folders: Vec<Folder>,
    id_gen: IdGen,

    // Selection
    selected_snippet_id: Option<SnippetId>,
    selected_folder_id: Option<FolderId>,

    // View state
    sidebar_view: SidebarView,
    sort_order: SortOrder,
    search_query: String,
    search_scope: SearchScope,

    // Editor
    editing: bool,
    edit_title: String,
    edit_content: String,
    edit_language: Language,
    edit_tags: String,
    edit_description: String,

    // UI
    scroll_offset: f32,
    list_scroll: f32,
    recently_used: Vec<SnippetId>,
    show_stats: bool,
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
            modified_at: 1000,
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
            modified_at: 2000,
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
            modified_at: 3000,
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
            modified_at: 4000,
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
            modified_at: 5000,
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
            sidebar_view: SidebarView::Folders,
            sort_order: SortOrder::DateDesc,
            search_query: String::new(),
            search_scope: SearchScope::All,
            editing: false,
            edit_title: String::new(),
            edit_content: String::new(),
            edit_language: Language::PlainText,
            edit_tags: String::new(),
            edit_description: String::new(),
            scroll_offset: 0.0,
            list_scroll: 0.0,
            recently_used: Vec::new(),
            show_stats: false,
        }
    }

    fn create_snippet(&mut self, title: &str, content: &str, language: Language) -> SnippetId {
        if self.snippets.len() >= MAX_SNIPPETS || content.len() > MAX_CONTENT_LEN {
            return 0;
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
            modified_at: id,
            use_count: 0,
            description: String::new(),
            is_template,
            template_vars,
        });

        id
    }

    fn delete_snippet(&mut self, id: SnippetId) {
        self.snippets.retain(|s| s.id != id);
        if self.selected_snippet_id == Some(id) {
            self.selected_snippet_id = None;
        }
        self.recently_used.retain(|&rid| rid != id);
    }

    fn create_folder(&mut self, name: &str) -> FolderId {
        if self.folders.len() >= MAX_FOLDERS || name.is_empty() {
            return 0;
        }

        let id = self.id_gen.next_id();
        self.folders.push(Folder {
            id,
            name: name.into(),
            parent_id: self.selected_folder_id,
            expanded: true,
            color: BLUE,
        });
        id
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

    fn use_snippet(&mut self, id: SnippetId) {
        if let Some(snippet) = self.snippets.iter_mut().find(|s| s.id == id) {
            snippet.use_count = snippet.use_count.saturating_add(1);
        }
        self.recently_used.retain(|&rid| rid != id);
        self.recently_used.insert(0, id);
        if self.recently_used.len() > MAX_RECENT {
            self.recently_used.truncate(MAX_RECENT);
        }
    }

    fn filtered_snippets(&self) -> Vec<&Snippet> {
        let mut results = search_snippets(&self.snippets, &self.search_query, self.search_scope);

        // Apply sidebar filter
        match self.sidebar_view {
            SidebarView::Folders => {
                if let Some(fid) = self.selected_folder_id {
                    results.retain(|s| s.folder_id == Some(fid));
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
                // Sort by recency
                results.sort_by_key(|s| {
                    recent
                        .iter()
                        .position(|&id| id == s.id)
                        .unwrap_or(usize::MAX)
                });
                return results;
            }
            _ => {}
        }

        // Sort
        match self.sort_order {
            SortOrder::NameAsc => results.sort_by(|a, b| a.title.cmp(&b.title)),
            SortOrder::NameDesc => results.sort_by(|a, b| b.title.cmp(&a.title)),
            SortOrder::DateAsc => results.sort_by_key(|s| s.created_at),
            SortOrder::DateDesc => results.sort_by_key(|s| std::cmp::Reverse(s.created_at)),
            SortOrder::UsageDesc => results.sort_by_key(|s| std::cmp::Reverse(s.use_count)),
            SortOrder::LanguageAsc => {
                results.sort_by(|a, b| a.language.name().cmp(b.language.name()))
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
        tag_counts.sort_by_key(|&(_, count)| std::cmp::Reverse(count));
        tag_counts
    }

    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Toolbar
        self.render_toolbar(&mut cmds);

        let content_y = TOOLBAR_HEIGHT;

        // Sidebar
        self.render_sidebar(&mut cmds, content_y);

        // Snippet list
        let list_x = SIDEBAR_WIDTH;
        self.render_snippet_list(&mut cmds, list_x, content_y);

        // Editor/viewer
        let editor_x = SIDEBAR_WIDTH + LIST_WIDTH;
        let editor_w = WINDOW_WIDTH - editor_x;
        self.render_editor(&mut cmds, editor_x, content_y, editor_w);

        // Stats overlay
        if self.show_stats {
            self.render_stats_overlay(&mut cmds);
        }

        cmds
    }

    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>) {
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: TOOLBAR_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: 13.0,
            text: "Snippet Manager".into(),
            font_size: TITLE_TEXT,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(160.0),
        });

        // Action buttons
        let buttons = [
            ("+ New", BLUE),
            ("Import", TEAL),
            ("Export", GREEN),
            ("Stats", MAUVE),
        ];
        let mut bx = 180.0;
        for (label, color) in &buttons {
            let bw = (label.len() as f32) * CHAR_WIDTH + 16.0;
            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: 8.0,
                width: bw,
                height: 28.0,
                color: *color,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: bx + 8.0,
                y: 14.0,
                text: (*label).into(),
                font_size: SMALL_TEXT,
                color: CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: Some(bw),
            });
            bx += bw + 6.0;
        }

        // Search bar
        let search_x = WINDOW_WIDTH - 320.0;
        let search_w = 250.0;
        cmds.push(RenderCommand::FillRect {
            x: search_x,
            y: 8.0,
            width: search_w,
            height: 28.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(14.0),
        });

        let search_display = if self.search_query.is_empty() {
            "Search snippets..."
        } else {
            &self.search_query
        };
        let search_color = if self.search_query.is_empty() {
            OVERLAY0
        } else {
            TEXT
        };
        cmds.push(RenderCommand::Text {
            x: search_x + 12.0,
            y: 15.0,
            text: search_display.into(),
            font_size: SMALL_TEXT,
            color: search_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(search_w - 24.0),
        });

        // Snippet count
        cmds.push(RenderCommand::Text {
            x: WINDOW_WIDTH - 60.0,
            y: 15.0,
            text: format!("{}", self.snippets.len()),
            font_size: SMALL_TEXT,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(50.0),
        });
    }

    fn render_sidebar(&self, cmds: &mut Vec<RenderCommand>, y: f32) {
        let height = WINDOW_HEIGHT - y;

        // Sidebar background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: SIDEBAR_WIDTH,
            height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // View categories
        let views = [
            SidebarView::Folders,
            SidebarView::Tags,
            SidebarView::Languages,
            SidebarView::Favorites,
            SidebarView::Recent,
            SidebarView::Templates,
        ];

        for (vi, view) in views.iter().enumerate() {
            let vy = y + 4.0 + (vi as f32) * 30.0;
            let selected = *view == self.sidebar_view;

            if selected {
                cmds.push(RenderCommand::FillRect {
                    x: 4.0,
                    y: vy,
                    width: SIDEBAR_WIDTH - 8.0,
                    height: 26.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: 12.0,
                y: vy + 5.0,
                text: view.icon().into(),
                font_size: SMALL_TEXT,
                color: if selected { BLUE } else { OVERLAY0 },
                font_weight: FontWeightHint::Regular,
                max_width: Some(30.0),
            });

            cmds.push(RenderCommand::Text {
                x: 44.0,
                y: vy + 6.0,
                text: view.label().into(),
                font_size: NORMAL_TEXT,
                color: if selected { TEXT } else { SUBTEXT0 },
                font_weight: if selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(SIDEBAR_WIDTH - 56.0),
            });
        }

        // Separator
        let sep_y = y + 4.0 + (views.len() as f32) * 30.0 + 4.0;
        cmds.push(RenderCommand::FillRect {
            x: 8.0,
            y: sep_y,
            width: SIDEBAR_WIDTH - 16.0,
            height: 1.0,
            color: SURFACE1,
            corner_radii: CornerRadii::ZERO,
        });

        // Show folder list or tag list based on sidebar_view
        let items_y = sep_y + 8.0;
        match self.sidebar_view {
            SidebarView::Folders => {
                for (fi, folder) in self.folders.iter().enumerate() {
                    if folder.parent_id.is_some() {
                        continue;
                    } // only top-level for now
                    let fy = items_y + (fi as f32) * 26.0;
                    if fy > WINDOW_HEIGHT - 20.0 {
                        break;
                    }

                    let selected = self.selected_folder_id == Some(folder.id);

                    if selected {
                        cmds.push(RenderCommand::FillRect {
                            x: 4.0,
                            y: fy,
                            width: SIDEBAR_WIDTH - 8.0,
                            height: 22.0,
                            color: SURFACE0,
                            corner_radii: CornerRadii::all(3.0),
                        });
                    }

                    // Folder color dot
                    cmds.push(RenderCommand::FillRect {
                        x: 14.0,
                        y: fy + 6.0,
                        width: 10.0,
                        height: 10.0,
                        color: folder.color,
                        corner_radii: CornerRadii::all(5.0),
                    });

                    cmds.push(RenderCommand::Text {
                        x: 30.0,
                        y: fy + 4.0,
                        text: folder.name.clone(),
                        font_size: SMALL_TEXT,
                        color: if selected { TEXT } else { SUBTEXT0 },
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(SIDEBAR_WIDTH - 42.0),
                    });

                    // Count
                    let count = self
                        .snippets
                        .iter()
                        .filter(|s| s.folder_id == Some(folder.id))
                        .count();
                    cmds.push(RenderCommand::Text {
                        x: SIDEBAR_WIDTH - 30.0,
                        y: fy + 4.0,
                        text: format!("{count}"),
                        font_size: SMALL_TEXT,
                        color: OVERLAY0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(25.0),
                    });
                }
            }
            SidebarView::Tags => {
                let tags = self.all_tags();
                for (ti, (tag, count)) in tags.iter().enumerate() {
                    let ty = items_y + (ti as f32) * 22.0;
                    if ty > WINDOW_HEIGHT - 20.0 {
                        break;
                    }

                    cmds.push(RenderCommand::Text {
                        x: 14.0,
                        y: ty + 2.0,
                        text: format!("#{tag}"),
                        font_size: SMALL_TEXT,
                        color: TEAL,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(SIDEBAR_WIDTH - 50.0),
                    });
                    cmds.push(RenderCommand::Text {
                        x: SIDEBAR_WIDTH - 30.0,
                        y: ty + 2.0,
                        text: format!("{count}"),
                        font_size: SMALL_TEXT,
                        color: OVERLAY0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(25.0),
                    });
                }
            }
            SidebarView::Languages => {
                for (li, lang) in Language::all().iter().enumerate() {
                    let count = self.snippets.iter().filter(|s| s.language == *lang).count();
                    if count == 0 {
                        continue;
                    }
                    let ly = items_y + (li as f32) * 22.0;
                    if ly > WINDOW_HEIGHT - 20.0 {
                        break;
                    }

                    cmds.push(RenderCommand::FillRect {
                        x: 14.0,
                        y: ly + 5.0,
                        width: 8.0,
                        height: 8.0,
                        color: lang.color(),
                        corner_radii: CornerRadii::all(4.0),
                    });
                    cmds.push(RenderCommand::Text {
                        x: 28.0,
                        y: ly + 2.0,
                        text: lang.name().into(),
                        font_size: SMALL_TEXT,
                        color: SUBTEXT0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(SIDEBAR_WIDTH - 60.0),
                    });
                    cmds.push(RenderCommand::Text {
                        x: SIDEBAR_WIDTH - 30.0,
                        y: ly + 2.0,
                        text: format!("{count}"),
                        font_size: SMALL_TEXT,
                        color: OVERLAY0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(25.0),
                    });
                }
            }
            _ => {}
        }
    }

    fn render_snippet_list(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32) {
        let height = WINDOW_HEIGHT - y;

        // List background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: LIST_WIDTH,
            height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Border
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: 1.0,
            height,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        // Sort header
        cmds.push(RenderCommand::FillRect {
            x: x + 1.0,
            y,
            width: LIST_WIDTH - 1.0,
            height: 28.0,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y: y + 7.0,
            text: format!("Sort: {}", self.sort_order.label()),
            font_size: SMALL_TEXT,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(LIST_WIDTH - 16.0),
        });

        // Snippet entries
        let filtered = self.filtered_snippets();
        let list_y = y + 30.0;
        let item_height = 58.0;

        for (si, snippet) in filtered.iter().enumerate() {
            let sy = list_y + (si as f32) * item_height - self.list_scroll;
            if sy < list_y - item_height || sy > WINDOW_HEIGHT {
                continue;
            }

            let selected = self.selected_snippet_id == Some(snippet.id);

            // Item background
            cmds.push(RenderCommand::FillRect {
                x: x + 4.0,
                y: sy,
                width: LIST_WIDTH - 8.0,
                height: item_height - 4.0,
                color: if selected { SURFACE0 } else { BASE },
                corner_radii: CornerRadii::all(4.0),
            });

            // Language badge
            let badge_w = (snippet.language.name().len() as f32) * 6.0 + 8.0;
            cmds.push(RenderCommand::FillRect {
                x: x + 8.0,
                y: sy + 4.0,
                width: badge_w,
                height: 16.0,
                color: snippet.language.color(),
                corner_radii: CornerRadii::all(8.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: sy + 6.0,
                text: snippet.language.name().into(),
                font_size: 10.0,
                color: CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: Some(badge_w),
            });

            // Favorite star
            if snippet.favorite {
                cmds.push(RenderCommand::Text {
                    x: x + LIST_WIDTH - 24.0,
                    y: sy + 4.0,
                    text: "*".into(),
                    font_size: NORMAL_TEXT,
                    color: YELLOW,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(16.0),
                });
            }

            // Title
            cmds.push(RenderCommand::Text {
                x: x + 8.0,
                y: sy + 22.0,
                text: truncate_str(&snippet.title, 32),
                font_size: NORMAL_TEXT,
                color: if selected { TEXT } else { SUBTEXT1 },
                font_weight: FontWeightHint::Bold,
                max_width: Some(LIST_WIDTH - 20.0),
            });

            // Tags
            if !snippet.tags.is_empty() {
                let tags_str: String = snippet
                    .tags
                    .iter()
                    .take(3)
                    .map(|t| format!("#{t}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                cmds.push(RenderCommand::Text {
                    x: x + 8.0,
                    y: sy + 40.0,
                    text: tags_str,
                    font_size: 10.0,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(LIST_WIDTH - 20.0),
                });
            }
        }

        if filtered.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + (LIST_WIDTH / 2.0) - 50.0,
                y: list_y + 40.0,
                text: "No snippets".into(),
                font_size: NORMAL_TEXT,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(LIST_WIDTH - 20.0),
            });
        }
    }

    fn render_editor(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let height = WINDOW_HEIGHT - y;

        // Editor background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Left border
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: 1.0,
            height,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        if let Some(snippet) = self.selected_snippet() {
            // Header
            cmds.push(RenderCommand::FillRect {
                x: x + 1.0,
                y,
                width: width - 1.0,
                height: 40.0,
                color: CRUST,
                corner_radii: CornerRadii::ZERO,
            });

            // Title
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: y + 5.0,
                text: snippet.title.clone(),
                font_size: HEADER_TEXT,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 120.0),
            });

            // Template indicator
            if snippet.is_template {
                cmds.push(RenderCommand::FillRect {
                    x: x + width - 100.0,
                    y: y + 6.0,
                    width: 70.0,
                    height: 18.0,
                    color: YELLOW,
                    corner_radii: CornerRadii::all(9.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + width - 92.0,
                    y: y + 9.0,
                    text: "Template".into(),
                    font_size: 10.0,
                    color: CRUST,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(60.0),
                });
            }

            // Language badge
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: y + 25.0,
                text: snippet.language.name().into(),
                font_size: SMALL_TEXT,
                color: snippet.language.color(),
                font_weight: FontWeightHint::Bold,
                max_width: Some(80.0),
            });

            // Use count
            cmds.push(RenderCommand::Text {
                x: x + 100.0,
                y: y + 25.0,
                text: format!("Used {} times", snippet.use_count),
                font_size: SMALL_TEXT,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(120.0),
            });

            // Description
            if !snippet.description.is_empty() {
                cmds.push(RenderCommand::Text {
                    x: x + 12.0,
                    y: y + 45.0,
                    text: snippet.description.clone(),
                    font_size: SMALL_TEXT,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 24.0),
                });
            }

            // Code area
            let code_y = y + 64.0;
            let code_h = height - 64.0 - 30.0; // leave room for status bar

            cmds.push(RenderCommand::FillRect {
                x: x + 8.0,
                y: code_y,
                width: width - 16.0,
                height: code_h,
                color: BASE,
                corner_radii: CornerRadii::all(6.0),
            });

            // Syntax highlighted code
            let tokens = tokenize(&snippet.content, snippet.language);
            let max_lines = ((code_h - 16.0) / LINE_HEIGHT) as usize;
            let scroll = (self.scroll_offset / LINE_HEIGHT) as usize;

            for (li, line_tokens) in tokens.iter().enumerate().skip(scroll).take(max_lines) {
                let ly = code_y + 8.0 + ((li - scroll) as f32) * LINE_HEIGHT;

                // Line number
                cmds.push(RenderCommand::Text {
                    x: x + 14.0,
                    y: ly,
                    text: format!("{:>3}", li.saturating_add(1)),
                    font_size: SMALL_TEXT,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(28.0),
                });

                // Tokens
                let mut tx = x + 48.0;
                for token in line_tokens {
                    let tw = (token.text.len() as f32) * CHAR_WIDTH;
                    cmds.push(RenderCommand::Text {
                        x: tx,
                        y: ly,
                        text: token.text.clone(),
                        font_size: NORMAL_TEXT,
                        color: token.kind.color(),
                        font_weight: if token.kind == TokenKind::Keyword {
                            FontWeightHint::Bold
                        } else {
                            FontWeightHint::Regular
                        },
                        max_width: Some(width - (tx - x) - 12.0),
                    });
                    tx += tw;
                }
            }

            // Tags bar at bottom
            let tags_y = y + height - 28.0;
            cmds.push(RenderCommand::FillRect {
                x: x + 1.0,
                y: tags_y,
                width: width - 1.0,
                height: 28.0,
                color: CRUST,
                corner_radii: CornerRadii::ZERO,
            });

            let mut tag_x = x + 8.0;
            for tag in &snippet.tags {
                let tw = (tag.len() as f32) * 7.0 + 16.0;
                cmds.push(RenderCommand::FillRect {
                    x: tag_x,
                    y: tags_y + 4.0,
                    width: tw,
                    height: 20.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(10.0),
                });
                cmds.push(RenderCommand::Text {
                    x: tag_x + 8.0,
                    y: tags_y + 8.0,
                    text: format!("#{tag}"),
                    font_size: 10.0,
                    color: TEAL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(tw),
                });
                tag_x += tw + 4.0;
            }

            // Line count
            let line_count = snippet.content.lines().count();
            cmds.push(RenderCommand::Text {
                x: x + width - 80.0,
                y: tags_y + 8.0,
                text: format!("{line_count} lines"),
                font_size: SMALL_TEXT,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(70.0),
            });
        } else {
            // Empty state
            cmds.push(RenderCommand::Text {
                x: x + width / 2.0 - 80.0,
                y: y + height / 2.0 - 20.0,
                text: "Select a snippet".into(),
                font_size: HEADER_TEXT,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(200.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + width / 2.0 - 100.0,
                y: y + height / 2.0 + 10.0,
                text: "or create a new one".into(),
                font_size: NORMAL_TEXT,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(200.0),
            });
        }
    }

    fn render_stats_overlay(&self, cmds: &mut Vec<RenderCommand>) {
        let stats = self.stats();
        let overlay_w = 400.0;
        let overlay_h = 300.0;
        let ox = (WINDOW_WIDTH - overlay_w) / 2.0;
        let oy = (WINDOW_HEIGHT - overlay_h) / 2.0;

        // Backdrop
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            color: Color::rgba(0, 0, 0, 128),
            corner_radii: CornerRadii::ZERO,
        });

        // Dialog
        cmds.push(RenderCommand::FillRect {
            x: ox,
            y: oy,
            width: overlay_w,
            height: overlay_h,
            color: MANTLE,
            corner_radii: CornerRadii::all(12.0),
        });

        cmds.push(RenderCommand::Text {
            x: ox + 16.0,
            y: oy + 16.0,
            text: "Library Statistics".into(),
            font_size: TITLE_TEXT,
            color: BLUE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(overlay_w - 32.0),
        });

        let stat_items = [
            ("Snippets", format!("{}", stats.total_snippets)),
            ("Folders", format!("{}", stats.total_folders)),
            ("Tags", format!("{}", stats.total_tags)),
            ("Favorites", format!("{}", stats.favorites)),
            ("Templates", format!("{}", stats.templates)),
            ("Total Lines", format!("{}", stats.total_lines)),
            ("Total Size", format_size(stats.total_chars)),
        ];

        for (si, (label, value)) in stat_items.iter().enumerate() {
            let sy = oy + 50.0 + (si as f32) * 24.0;
            cmds.push(RenderCommand::Text {
                x: ox + 20.0,
                y: sy,
                text: (*label).into(),
                font_size: NORMAL_TEXT,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(150.0),
            });
            cmds.push(RenderCommand::Text {
                x: ox + 180.0,
                y: sy,
                text: value.clone(),
                font_size: NORMAL_TEXT,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(200.0),
            });
        }

        // Language breakdown
        let lang_y = oy + 50.0 + (stat_items.len() as f32) * 24.0 + 10.0;
        cmds.push(RenderCommand::Text {
            x: ox + 16.0,
            y: lang_y,
            text: "By Language:".into(),
            font_size: SMALL_TEXT,
            color: SUBTEXT1,
            font_weight: FontWeightHint::Bold,
            max_width: Some(overlay_w - 32.0),
        });

        for (li, (lang, count)) in stats.by_language.iter().take(5).enumerate() {
            let ly = lang_y + 20.0 + (li as f32) * 18.0;
            cmds.push(RenderCommand::FillRect {
                x: ox + 20.0,
                y: ly + 3.0,
                width: 8.0,
                height: 8.0,
                color: lang.color(),
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: ox + 34.0,
                y: ly,
                text: format!("{}: {count}", lang.name()),
                font_size: SMALL_TEXT,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(200.0),
            });
        }
    }
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.into()
    } else {
        let truncated: String = s.chars().take(max_chars.saturating_sub(3)).collect();
        format!("{truncated}...")
    }
}

fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let app = App::new();
    let _cmds = app.render();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
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

    #[test]
    fn test_truncate_str_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_str_long() {
        let result = truncate_str("hello world this is long", 10);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(500), "500 B");
    }

    #[test]
    fn test_format_size_kb() {
        let result = format_size(2048);
        assert!(result.contains("KB"));
    }

    #[test]
    fn test_format_size_mb() {
        let result = format_size(2 * 1024 * 1024);
        assert!(result.contains("MB"));
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
            modified_at: id,
            use_count: 0,
            description: String::new(),
            is_template: false,
            template_vars: Vec::new(),
        }
    }
}
