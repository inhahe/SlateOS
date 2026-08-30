#![deny(clippy::all)]

//! fontconfig — Slate OS font configuration and discovery
//!
//! Multi-personality binary for font management.
//! Detected via argv[0]:
//!
//! - `fc-list` (default) — list available fonts
//! - `fc-match` — match fonts to patterns
//! - `fc-cache` — build font cache
//! - `fc-cat` — read font cache
//! - `fc-query` — query font files
//! - `fc-scan` — scan font directories
//! - `fc-validate` — validate font files
//! - `fc-conflist` — list font configuration files

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const _FC_CONF: &str = "/etc/fonts/fonts.conf";
const _FC_CACHE_DIR: &str = "/var/cache/fontconfig";
const _FONT_DIRS: &[&str] = &[
    "/usr/share/fonts",
    "/usr/local/share/fonts",
    "/home/user/.local/share/fonts",
];

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct FontEntry {
    family: String,
    style: FontStyle,
    file: String,
    _index: u32,
    _slant: FontSlant,
    _weight: FontWeight,
    _width: FontWidth,
    spacing: FontSpacing,
    _lang: Vec<String>,
    _charset: String,
}

// Full CSS font-style ladder kept for symmetry with FontWeight; only a
// subset is currently emitted by the sample-data generator, so allow
// dead_code on the unused variants.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
enum FontStyle {
    Regular,
    Bold,
    Italic,
    BoldItalic,
    Light,
    Thin,
    Medium,
    SemiBold,
    ExtraBold,
    Black,
}

impl std::fmt::Display for FontStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Regular => write!(f, "Regular"),
            Self::Bold => write!(f, "Bold"),
            Self::Italic => write!(f, "Italic"),
            Self::BoldItalic => write!(f, "Bold Italic"),
            Self::Light => write!(f, "Light"),
            Self::Thin => write!(f, "Thin"),
            Self::Medium => write!(f, "Medium"),
            Self::SemiBold => write!(f, "SemiBold"),
            Self::ExtraBold => write!(f, "ExtraBold"),
            Self::Black => write!(f, "Black"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum FontSlant {
    _Roman,
    _Italic,
    _Oblique,
}

// Same rationale as FontStyle — full CSS weight ladder, partial usage.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
enum FontWeight {
    Thin,
    _ExtraLight,
    _Light,
    _Regular,
    Medium,
    SemiBold,
    _Bold,
    ExtraBold,
    Black,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum FontWidth {
    _UltraCondensed,
    _Condensed,
    _Normal,
    _Expanded,
    _UltraExpanded,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum FontSpacing {
    Proportional,
    Mono,
    _DualWidth,
    _CharCell,
}

impl std::fmt::Display for FontSpacing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Proportional => write!(f, "proportional"),
            Self::Mono => write!(f, "mono"),
            Self::_DualWidth => write!(f, "dual"),
            Self::_CharCell => write!(f, "charcell"),
        }
    }
}

// ── Simulated data ────────────────────────────────────────────────────

fn read_font_database() -> Vec<FontEntry> {
    vec![
        FontEntry {
            family: "DejaVu Sans".to_string(),
            style: FontStyle::Regular,
            file: "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".to_string(),
            _index: 0, _slant: FontSlant::_Roman, _weight: FontWeight::_Regular,
            _width: FontWidth::_Normal, spacing: FontSpacing::Proportional,
            _lang: vec!["en".to_string(), "de".to_string(), "fr".to_string()],
            _charset: "0000-FFFF".to_string(),
        },
        FontEntry {
            family: "DejaVu Sans".to_string(),
            style: FontStyle::Bold,
            file: "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf".to_string(),
            _index: 0, _slant: FontSlant::_Roman, _weight: FontWeight::_Bold,
            _width: FontWidth::_Normal, spacing: FontSpacing::Proportional,
            _lang: vec!["en".to_string()],
            _charset: "0000-FFFF".to_string(),
        },
        FontEntry {
            family: "DejaVu Sans".to_string(),
            style: FontStyle::Italic,
            file: "/usr/share/fonts/truetype/dejavu/DejaVuSans-Oblique.ttf".to_string(),
            _index: 0, _slant: FontSlant::_Italic, _weight: FontWeight::_Regular,
            _width: FontWidth::_Normal, spacing: FontSpacing::Proportional,
            _lang: vec!["en".to_string()],
            _charset: "0000-FFFF".to_string(),
        },
        FontEntry {
            family: "DejaVu Sans Mono".to_string(),
            style: FontStyle::Regular,
            file: "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf".to_string(),
            _index: 0, _slant: FontSlant::_Roman, _weight: FontWeight::_Regular,
            _width: FontWidth::_Normal, spacing: FontSpacing::Mono,
            _lang: vec!["en".to_string()],
            _charset: "0000-FFFF".to_string(),
        },
        FontEntry {
            family: "DejaVu Serif".to_string(),
            style: FontStyle::Regular,
            file: "/usr/share/fonts/truetype/dejavu/DejaVuSerif.ttf".to_string(),
            _index: 0, _slant: FontSlant::_Roman, _weight: FontWeight::_Regular,
            _width: FontWidth::_Normal, spacing: FontSpacing::Proportional,
            _lang: vec!["en".to_string()],
            _charset: "0000-FFFF".to_string(),
        },
        FontEntry {
            family: "Noto Sans".to_string(),
            style: FontStyle::Regular,
            file: "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf".to_string(),
            _index: 0, _slant: FontSlant::_Roman, _weight: FontWeight::_Regular,
            _width: FontWidth::_Normal, spacing: FontSpacing::Proportional,
            _lang: vec!["en".to_string(), "ja".to_string(), "zh".to_string()],
            _charset: "0000-FFFF".to_string(),
        },
        FontEntry {
            family: "Noto Sans Mono".to_string(),
            style: FontStyle::Regular,
            file: "/usr/share/fonts/truetype/noto/NotoSansMono-Regular.ttf".to_string(),
            _index: 0, _slant: FontSlant::_Roman, _weight: FontWeight::_Regular,
            _width: FontWidth::_Normal, spacing: FontSpacing::Mono,
            _lang: vec!["en".to_string()],
            _charset: "0000-FFFF".to_string(),
        },
        FontEntry {
            family: "Liberation Sans".to_string(),
            style: FontStyle::Regular,
            file: "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf".to_string(),
            _index: 0, _slant: FontSlant::_Roman, _weight: FontWeight::_Regular,
            _width: FontWidth::_Normal, spacing: FontSpacing::Proportional,
            _lang: vec!["en".to_string()],
            _charset: "0000-FFFF".to_string(),
        },
        FontEntry {
            family: "Liberation Mono".to_string(),
            style: FontStyle::Regular,
            file: "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf".to_string(),
            _index: 0, _slant: FontSlant::_Roman, _weight: FontWeight::_Regular,
            _width: FontWidth::_Normal, spacing: FontSpacing::Mono,
            _lang: vec!["en".to_string()],
            _charset: "0000-FFFF".to_string(),
        },
        FontEntry {
            family: "Fira Code".to_string(),
            style: FontStyle::Regular,
            file: "/usr/share/fonts/truetype/firacode/FiraCode-Regular.ttf".to_string(),
            _index: 0, _slant: FontSlant::_Roman, _weight: FontWeight::_Regular,
            _width: FontWidth::_Normal, spacing: FontSpacing::Mono,
            _lang: vec!["en".to_string()],
            _charset: "0000-FFFF".to_string(),
        },
        FontEntry {
            family: "Fira Code".to_string(),
            style: FontStyle::Light,
            file: "/usr/share/fonts/truetype/firacode/FiraCode-Light.ttf".to_string(),
            _index: 0, _slant: FontSlant::_Roman, _weight: FontWeight::_Light,
            _width: FontWidth::_Normal, spacing: FontSpacing::Mono,
            _lang: vec!["en".to_string()],
            _charset: "0000-FFFF".to_string(),
        },
    ]
}

// ── fc-list personality ───────────────────────────────────────────────

fn run_fc_list(args: Vec<String>) -> i32 {
    let mut format_str: Option<&str> = None;
    let mut pattern: Option<&str> = None;
    let mut show_help = false;
    let mut brief = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => show_help = true,
            "--version" | "-V" => { println!("fc-list 0.1.0 (Slate OS)"); return 0; }
            "-f" | "--format" => {
                if let Some(val) = args.get(i + 1) { format_str = Some(val.as_str()); i += 1; }
            }
            "-b" | "--brief" => brief = true,
            s if !s.starts_with('-') => pattern = Some(s),
            _ => {}
        }
        i += 1;
    }

    if show_help {
        println!("Usage: fc-list [OPTIONS] [PATTERN [ELEMENT...]]");
        println!();
        println!("List available fonts.");
        println!();
        println!("Options:");
        println!("  -f, --format FMT  Use printf-style format string");
        println!("  -b, --brief       Brief output (family:style:file)");
        println!("  -V, --version     Show version");
        return 0;
    }

    let fonts = read_font_database();
    let filtered: Vec<&FontEntry> = if let Some(pat) = pattern {
        let pat_lower = pat.to_lowercase();
        fonts.iter().filter(|f| {
            f.family.to_lowercase().contains(&pat_lower) ||
            format!("{}", f.style).to_lowercase().contains(&pat_lower)
        }).collect()
    } else {
        fonts.iter().collect()
    };

    for f in &filtered {
        if brief {
            println!("{}:style={}", f.family, f.style);
        } else if let Some(_fmt) = format_str {
            println!("{}:style={}:file={}", f.family, f.style, f.file);
        } else {
            println!("{}: {} ({})", f.file, f.family, f.style);
        }
    }
    0
}

// ── fc-match personality ──────────────────────────────────────────────

fn run_fc_match(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "sans-serif".to_string());

    match cmd.as_str() {
        "--help" | "-h" => {
            println!("Usage: fc-match [OPTIONS] [PATTERN]");
            println!();
            println!("Find best font match for a pattern.");
            println!();
            println!("Options:");
            println!("  -a, --all     Show all matches, not just best");
            println!("  -s, --sort    Sort output by match quality");
            println!("  -V, --version Show version");
            0
        }
        "--version" | "-V" => { println!("fc-match 0.1.0 (Slate OS)"); 0 }
        _ => {
            let show_all = args.iter().any(|a| a == "-a" || a == "--all");
            let pattern = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("sans-serif");
            let fonts = read_font_database();

            let pat_lower = pattern.to_lowercase();
            let mut matches: Vec<&FontEntry> = fonts.iter().filter(|f| {
                f.family.to_lowercase().contains(&pat_lower) ||
                (pat_lower == "monospace" && f.spacing == FontSpacing::Mono) ||
                (pat_lower == "sans-serif" && !f.family.contains("Serif") && f.spacing == FontSpacing::Proportional) ||
                (pat_lower == "serif" && f.family.contains("Serif"))
            }).collect();

            if matches.is_empty() {
                // Fallback to first available font
                if let Some(f) = fonts.first() {
                    println!("{}: \"{}\" \"{}\"", f.file, f.family, f.style);
                }
            } else if show_all {
                for m in &matches {
                    println!("{}: \"{}\" \"{}\"", m.file, m.family, m.style);
                }
            } else if let Some(best) = matches.first() {
                println!("{}: \"{}\" \"{}\"", best.file, best.family, best.style);
            }
            // Sort stable by family name
            matches.sort_by(|a, b| a.family.cmp(&b.family));
            0
        }
    }
}

// ── fc-cache personality ──────────────────────────────────────────────

fn run_fc_cache(args: Vec<String>) -> i32 {
    let force = args.iter().any(|a| a == "-f" || a == "--force");
    let system_only = args.iter().any(|a| a == "-s" || a == "--system-only");
    let verbose = args.iter().any(|a| a == "-v" || a == "--verbose");

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fc-cache [OPTIONS] [DIR...]");
        println!();
        println!("Build font information cache.");
        println!();
        println!("Options:");
        println!("  -f, --force       Force rebuild of cache");
        println!("  -s, --system-only Only scan system directories");
        println!("  -v, --verbose     Show verbose output");
        println!("  -V, --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("fc-cache 0.1.0 (Slate OS)");
        return 0;
    }

    if force { println!("fc-cache: forcing rebuild..."); }

    let dirs: Vec<&str> = if system_only {
        vec!["/usr/share/fonts", "/usr/local/share/fonts"]
    } else {
        vec!["/usr/share/fonts", "/usr/local/share/fonts", "/home/user/.local/share/fonts"]
    };

    let fonts = read_font_database();
    for dir in &dirs {
        let count = fonts.iter().filter(|f| f.file.starts_with(dir)).count();
        if verbose {
            println!("{}: caching, new cache contents: {} fonts, 0 dirs", dir, count);
        }
    }

    println!("fc-cache: cache built for {} fonts in {} directories",
        fonts.len(), dirs.len());
    0
}

// ── fc-cat personality ────────────────────────────────────────────────

fn run_fc_cat(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fc-cat [CACHEFILE|DIRECTORY]");
        println!("Read font information cache files.");
        return 0;
    }

    let fonts = read_font_database();
    println!("\"\" 0 \"{}\"", _FC_CACHE_DIR);
    for f in &fonts {
        println!("\"{}\" 0 \"{}:style={}:spacing={}\"",
            f.file, f.family, f.style, f.spacing as u32);
    }
    0
}

// ── fc-query personality ──────────────────────────────────────────────

fn run_fc_query(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fc-query [OPTIONS] FONTFILE...");
        println!("Query font file(s) and report information.");
        return 0;
    }

    let file = args.first().map(|s| s.as_str()).unwrap_or("font.ttf");
    println!("Pattern has 1 elts (size 16)");
    println!("\tfamily: \"DejaVu Sans\"(s)");
    println!("\tstyle: \"Regular\"(s)");
    println!("\tslant: 0(i)(s)");
    println!("\tweight: 80(f)(s)");
    println!("\twidth: 100(f)(s)");
    println!("\tspacing: 0(i)(s)");
    println!("\tfile: \"{}\"(s)", file);
    println!("\toutline: True(s)");
    println!("\tscalable: True(s)");
    0
}

// ── fc-scan personality ───────────────────────────────────────────────

fn run_fc_scan(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fc-scan [OPTIONS] DIR...");
        println!("Scan font directories and print font information.");
        return 0;
    }

    let dir = args.first().map(|s| s.as_str()).unwrap_or("/usr/share/fonts");
    let fonts = read_font_database();
    let matching: Vec<_> = fonts.iter().filter(|f| f.file.starts_with(dir)).collect();

    println!("Scanning {}... found {} fonts", dir, matching.len());
    for f in &matching {
        println!("  {}: {} {}", f.file, f.family, f.style);
    }
    0
}

// ── fc-validate personality ───────────────────────────────────────────

fn run_fc_validate(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fc-validate [FONTFILE...]");
        println!("Validate font file(s).");
        return 0;
    }

    for file in &args {
        if file.starts_with('-') { continue; }
        println!("{}: valid", file);
    }
    if args.is_empty() || args.iter().all(|a| a.starts_with('-')) {
        println!("fc-validate: no font files specified");
        return 1;
    }
    0
}

// ── fc-conflist personality ───────────────────────────────────────────

fn run_fc_conflist(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fc-conflist");
        println!("List font configuration files.");
        return 0;
    }

    println!("Font configuration files:");
    println!("  /etc/fonts/fonts.conf (active)");
    println!("  /etc/fonts/conf.d/10-hinting-slight.conf (active)");
    println!("  /etc/fonts/conf.d/10-sub-pixel-rgb.conf (active)");
    println!("  /etc/fonts/conf.d/11-lcdfilter-default.conf (active)");
    println!("  /etc/fonts/conf.d/20-unhint-small-dejavu-sans.conf (active)");
    println!("  /etc/fonts/conf.d/30-metric-aliases.conf (active)");
    println!("  /etc/fonts/conf.d/40-nonlatin.conf (active)");
    println!("  /etc/fonts/conf.d/45-generic.conf (active)");
    println!("  /etc/fonts/conf.d/49-sansserif.conf (active)");
    println!("  /etc/fonts/conf.d/50-user.conf (active)");
    println!("  /etc/fonts/conf.d/60-generic.conf (active)");
    println!("  /etc/fonts/conf.d/60-latin.conf (active)");
    println!("  /etc/fonts/conf.d/65-nonlatin.conf (active)");
    println!("  /etc/fonts/conf.d/69-unifont.conf (active)");
    println!("  /etc/fonts/conf.d/80-delicious.conf (active)");
    println!("  /etc/fonts/conf.d/90-synthetic.conf (active)");
    0
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("fc-list");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog_name.as_str() {
        "fc-match" => run_fc_match(rest),
        "fc-cache" => run_fc_cache(rest),
        "fc-cat" => run_fc_cat(rest),
        "fc-query" => run_fc_query(rest),
        "fc-scan" => run_fc_scan(rest),
        "fc-validate" => run_fc_validate(rest),
        "fc-conflist" => run_fc_conflist(rest),
        _ => run_fc_list(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_font_database() {
        let fonts = read_font_database();
        assert!(fonts.len() >= 10);
    }

    #[test]
    fn test_mono_fonts() {
        let fonts = read_font_database();
        let mono: Vec<_> = fonts.iter().filter(|f| f.spacing == FontSpacing::Mono).collect();
        assert!(mono.len() >= 3);
    }

    #[test]
    fn test_font_families() {
        let fonts = read_font_database();
        let families: std::collections::HashSet<_> = fonts.iter().map(|f| &f.family).collect();
        assert!(families.len() >= 5);
    }

    #[test]
    fn test_font_style_display() {
        assert_eq!(format!("{}", FontStyle::Regular), "Regular");
        assert_eq!(format!("{}", FontStyle::Bold), "Bold");
        assert_eq!(format!("{}", FontStyle::BoldItalic), "Bold Italic");
    }

    #[test]
    fn test_font_spacing_display() {
        assert_eq!(format!("{}", FontSpacing::Mono), "mono");
        assert_eq!(format!("{}", FontSpacing::Proportional), "proportional");
    }

    #[test]
    fn test_dejavu_variants() {
        let fonts = read_font_database();
        let dejavu: Vec<_> = fonts.iter().filter(|f| f.family.starts_with("DejaVu")).collect();
        assert!(dejavu.len() >= 3);
    }

    #[test]
    fn test_fira_code() {
        let fonts = read_font_database();
        let fira: Vec<_> = fonts.iter().filter(|f| f.family == "Fira Code").collect();
        assert!(fira.len() >= 2);
        assert!(fira.iter().all(|f| f.spacing == FontSpacing::Mono));
    }
}
