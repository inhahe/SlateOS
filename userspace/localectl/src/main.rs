//! SlateOS locale and keyboard configuration.
//!
//! Multi-personality binary providing:
//! - **localectl** — control system locale and keyboard settings
//!
//! Manages /etc/locale.conf and /etc/vconsole.conf for system-wide
//! locale, keymap, and X11 keyboard settings.

#![deny(clippy::all)]

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;
use std::process;

const VERSION: &str = "0.1.0";

// ============================================================================
// Configuration
// ============================================================================

const LOCALE_CONF: &str = "/etc/locale.conf";
const VCONSOLE_CONF: &str = "/etc/vconsole.conf";
const X11_CONF: &str = "/etc/X11/xorg.conf.d/00-keyboard.conf";
const _LOCALE_GEN: &str = "/etc/locale.gen";
const SUPPORTED_LOCALES: &str = "/usr/share/i18n/SUPPORTED";

#[derive(Clone, Debug, Default)]
struct LocaleSettings {
    lang: String,
    language: String,
    lc_ctype: String,
    lc_numeric: String,
    lc_time: String,
    lc_collate: String,
    lc_monetary: String,
    lc_messages: String,
    lc_paper: String,
    lc_name: String,
    lc_address: String,
    lc_telephone: String,
    lc_measurement: String,
    lc_identification: String,
}

impl LocaleSettings {
    fn to_map(&self) -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();
        let entries = [
            ("LANG", &self.lang),
            ("LANGUAGE", &self.language),
            ("LC_CTYPE", &self.lc_ctype),
            ("LC_NUMERIC", &self.lc_numeric),
            ("LC_TIME", &self.lc_time),
            ("LC_COLLATE", &self.lc_collate),
            ("LC_MONETARY", &self.lc_monetary),
            ("LC_MESSAGES", &self.lc_messages),
            ("LC_PAPER", &self.lc_paper),
            ("LC_NAME", &self.lc_name),
            ("LC_ADDRESS", &self.lc_address),
            ("LC_TELEPHONE", &self.lc_telephone),
            ("LC_MEASUREMENT", &self.lc_measurement),
            ("LC_IDENTIFICATION", &self.lc_identification),
        ];
        for (key, value) in &entries {
            if !value.is_empty() {
                map.insert(key.to_string(), value.to_string());
            }
        }
        map
    }
}

#[derive(Clone, Debug, Default)]
struct KeymapSettings {
    keymap: String,
    keymap_toggle: String,
    _font: String,
    _font_map: String,
    _font_unimap: String,
}

#[derive(Clone, Debug, Default)]
struct X11KeyboardSettings {
    layout: String,
    model: String,
    variant: String,
    options: String,
}

// ============================================================================
// Config reading
// ============================================================================

fn read_key_value_file(path: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    if let Ok(content) = fs::read_to_string(path) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let value = value.trim().trim_matches('"');
                map.insert(key.trim().to_string(), value.to_string());
            }
        }
    }
    map
}

fn write_key_value_file(path: &str, map: &BTreeMap<String, String>) -> Result<(), String> {
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Cannot create dir: {e}"))?;
    }
    let mut content = String::new();
    for (key, value) in map {
        content.push_str(&format!("{key}=\"{value}\"\n"));
    }
    fs::write(path, content).map_err(|e| format!("Cannot write {path}: {e}"))
}

fn read_locale_settings() -> LocaleSettings {
    let map = read_key_value_file(LOCALE_CONF);
    LocaleSettings {
        lang: map.get("LANG").cloned().unwrap_or_default(),
        language: map.get("LANGUAGE").cloned().unwrap_or_default(),
        lc_ctype: map.get("LC_CTYPE").cloned().unwrap_or_default(),
        lc_numeric: map.get("LC_NUMERIC").cloned().unwrap_or_default(),
        lc_time: map.get("LC_TIME").cloned().unwrap_or_default(),
        lc_collate: map.get("LC_COLLATE").cloned().unwrap_or_default(),
        lc_monetary: map.get("LC_MONETARY").cloned().unwrap_or_default(),
        lc_messages: map.get("LC_MESSAGES").cloned().unwrap_or_default(),
        lc_paper: map.get("LC_PAPER").cloned().unwrap_or_default(),
        lc_name: map.get("LC_NAME").cloned().unwrap_or_default(),
        lc_address: map.get("LC_ADDRESS").cloned().unwrap_or_default(),
        lc_telephone: map.get("LC_TELEPHONE").cloned().unwrap_or_default(),
        lc_measurement: map.get("LC_MEASUREMENT").cloned().unwrap_or_default(),
        lc_identification: map.get("LC_IDENTIFICATION").cloned().unwrap_or_default(),
    }
}

fn read_keymap_settings() -> KeymapSettings {
    let map = read_key_value_file(VCONSOLE_CONF);
    KeymapSettings {
        keymap: map.get("KEYMAP").cloned().unwrap_or_default(),
        keymap_toggle: map.get("KEYMAP_TOGGLE").cloned().unwrap_or_default(),
        _font: map.get("FONT").cloned().unwrap_or_default(),
        _font_map: map.get("FONT_MAP").cloned().unwrap_or_default(),
        _font_unimap: map.get("FONT_UNIMAP").cloned().unwrap_or_default(),
    }
}

fn read_x11_settings() -> X11KeyboardSettings {
    let mut settings = X11KeyboardSettings::default();
    if let Ok(content) = fs::read_to_string(X11_CONF) {
        for line in content.lines() {
            let line = line.trim();
            if let Some((key, value)) = line.split_once('"') {
                let value = value.trim_end_matches('"');
                let key_lower = key.to_lowercase();
                if key_lower.contains("xkblayout") {
                    settings.layout = value.to_string();
                } else if key_lower.contains("xkbmodel") {
                    settings.model = value.to_string();
                } else if key_lower.contains("xkbvariant") {
                    settings.variant = value.to_string();
                } else if key_lower.contains("xkboptions") {
                    settings.options = value.to_string();
                }
            }
        }
    }
    settings
}

// ============================================================================
// Available locales/keymaps
// ============================================================================

fn list_available_locales() -> Vec<String> {
    let mut locales = Vec::new();

    // Try /usr/share/i18n/SUPPORTED.
    if let Ok(content) = fs::read_to_string(SUPPORTED_LOCALES) {
        for line in content.lines() {
            let line = line.trim();
            if !line.is_empty() && !line.starts_with('#')
                && let Some(locale) = line.split_whitespace().next() {
                    locales.push(locale.to_string());
                }
        }
    }

    // Try locale -a style from /usr/lib/locale.
    if locales.is_empty()
        && let Ok(entries) = fs::read_dir("/usr/lib/locale") {
            for entry in entries.flatten() {
                locales.push(entry.file_name().to_string_lossy().to_string());
            }
        }

    // Fallback built-in list.
    if locales.is_empty() {
        locales = vec![
            "C".to_string(), "C.UTF-8".to_string(), "POSIX".to_string(),
            "en_US.UTF-8".to_string(), "en_GB.UTF-8".to_string(),
            "de_DE.UTF-8".to_string(), "fr_FR.UTF-8".to_string(),
            "es_ES.UTF-8".to_string(), "it_IT.UTF-8".to_string(),
            "ja_JP.UTF-8".to_string(), "ko_KR.UTF-8".to_string(),
            "zh_CN.UTF-8".to_string(), "zh_TW.UTF-8".to_string(),
            "pt_BR.UTF-8".to_string(), "ru_RU.UTF-8".to_string(),
        ];
    }

    locales.sort();
    locales.dedup();
    locales
}

fn list_available_keymaps() -> Vec<String> {
    let mut keymaps = Vec::new();
    let keymap_dirs = [
        "/usr/share/keymaps",
        "/usr/share/kbd/keymaps",
        "/usr/lib/kbd/keymaps",
    ];

    for dir in &keymap_dirs {
        collect_keymaps(Path::new(dir), &mut keymaps);
    }

    // Fallback.
    if keymaps.is_empty() {
        keymaps = vec![
            "us".to_string(), "uk".to_string(), "de".to_string(),
            "fr".to_string(), "es".to_string(), "it".to_string(),
            "jp".to_string(), "kr".to_string(), "ru".to_string(),
            "br".to_string(), "dvorak".to_string(), "colemak".to_string(),
        ];
    }

    keymaps.sort();
    keymaps.dedup();
    keymaps
}

fn collect_keymaps(dir: &Path, keymaps: &mut Vec<String>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_keymaps(&path, keymaps);
            } else {
                let name = entry.file_name().to_string_lossy().to_string();
                let name = name
                    .strip_suffix(".map.gz")
                    .or_else(|| name.strip_suffix(".map"))
                    .unwrap_or(&name)
                    .to_string();
                keymaps.push(name);
            }
        }
    }
}

fn list_x11_layouts() -> Vec<String> {
    // Parse /usr/share/X11/xkb/rules/base.lst.
    let rules_file = "/usr/share/X11/xkb/rules/base.lst";
    let mut layouts = Vec::new();
    let mut in_layouts = false;

    if let Ok(content) = fs::read_to_string(rules_file) {
        for line in content.lines() {
            if line.starts_with("! layout") {
                in_layouts = true;
                continue;
            }
            if line.starts_with('!') {
                in_layouts = false;
                continue;
            }
            if in_layouts && !line.is_empty()
                && let Some(name) = line.split_whitespace().next() {
                    layouts.push(name.to_string());
                }
        }
    }

    if layouts.is_empty() {
        layouts = vec![
            "us".to_string(), "gb".to_string(), "de".to_string(),
            "fr".to_string(), "es".to_string(), "it".to_string(),
            "jp".to_string(), "kr".to_string(), "ru".to_string(),
        ];
    }

    layouts
}

// ============================================================================
// Commands
// ============================================================================

fn cmd_status() -> i32 {
    let locale = read_locale_settings();
    let keymap = read_keymap_settings();
    let x11 = read_x11_settings();

    println!("   System Locale: LANG={}", if locale.lang.is_empty() { "n/a" } else { &locale.lang });
    let locale_map = locale.to_map();
    for (key, value) in &locale_map {
        if key != "LANG" {
            println!("                  {key}={value}");
        }
    }

    println!("       VC Keymap: {}", if keymap.keymap.is_empty() { "n/a" } else { &keymap.keymap });
    if !keymap.keymap_toggle.is_empty() {
        println!("  VC Toggle Keymap: {}", keymap.keymap_toggle);
    }

    println!("      X11 Layout: {}", if x11.layout.is_empty() { "n/a" } else { &x11.layout });
    if !x11.model.is_empty() {
        println!("       X11 Model: {}", x11.model);
    }
    if !x11.variant.is_empty() {
        println!("     X11 Variant: {}", x11.variant);
    }
    if !x11.options.is_empty() {
        println!("     X11 Options: {}", x11.options);
    }

    0
}

fn cmd_set_locale(args: &[String]) -> i32 {
    let mut map = read_key_value_file(LOCALE_CONF);

    for arg in args {
        if let Some((key, value)) = arg.split_once('=') {
            map.insert(key.to_string(), value.to_string());
        }
    }

    match write_key_value_file(LOCALE_CONF, &map) {
        Ok(()) => {
            println!("localectl: locale settings updated");
            0
        }
        Err(e) => {
            eprintln!("localectl: {e}");
            1
        }
    }
}

fn cmd_set_keymap(keymap: &str, toggle: Option<&str>) -> i32 {
    let mut map = read_key_value_file(VCONSOLE_CONF);
    map.insert("KEYMAP".to_string(), keymap.to_string());
    if let Some(t) = toggle {
        map.insert("KEYMAP_TOGGLE".to_string(), t.to_string());
    }

    match write_key_value_file(VCONSOLE_CONF, &map) {
        Ok(()) => {
            println!("localectl: VC keymap set to '{keymap}'");
            0
        }
        Err(e) => {
            eprintln!("localectl: {e}");
            1
        }
    }
}

fn cmd_set_x11_keymap(layout: &str, model: Option<&str>, variant: Option<&str>, options: Option<&str>) -> i32 {
    let conf = format!(
        "Section \"InputClass\"\n\
         \tIdentifier \"system-keyboard\"\n\
         \tMatchIsKeyboard \"on\"\n\
         \tOption \"XkbLayout\" \"{layout}\"\n\
         {}\
         {}\
         {}\
         EndSection\n",
        model.map(|m| format!("\tOption \"XkbModel\" \"{m}\"\n")).unwrap_or_default(),
        variant.map(|v| format!("\tOption \"XkbVariant\" \"{v}\"\n")).unwrap_or_default(),
        options.map(|o| format!("\tOption \"XkbOptions\" \"{o}\"\n")).unwrap_or_default(),
    );

    if let Some(parent) = Path::new(X11_CONF).parent() {
        let _ = fs::create_dir_all(parent);
    }

    match fs::write(X11_CONF, conf) {
        Ok(()) => {
            println!("localectl: X11 keymap set to '{layout}'");
            0
        }
        Err(e) => {
            eprintln!("localectl: {e}");
            1
        }
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("localectl");
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
    let _ = prog_name;

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    if rest.is_empty() {
        process::exit(cmd_status());
    }

    let exit_code = match rest[0].as_str() {
        "status" => cmd_status(),
        "set-locale" => cmd_set_locale(&rest[1..]),
        "set-keymap" => {
            if rest.len() < 2 {
                eprintln!("localectl: set-keymap requires a keymap name");
                1
            } else {
                let toggle = rest.get(2).map(|s| s.as_str());
                cmd_set_keymap(&rest[1], toggle)
            }
        }
        "set-x11-keymap" => {
            if rest.len() < 2 {
                eprintln!("localectl: set-x11-keymap requires a layout");
                1
            } else {
                cmd_set_x11_keymap(
                    &rest[1],
                    rest.get(2).map(|s| s.as_str()),
                    rest.get(3).map(|s| s.as_str()),
                    rest.get(4).map(|s| s.as_str()),
                )
            }
        }
        "list-locales" => {
            for locale in &list_available_locales() {
                println!("{locale}");
            }
            0
        }
        "list-keymaps" => {
            for keymap in &list_available_keymaps() {
                println!("{keymap}");
            }
            0
        }
        "list-x11-keymap-layouts" => {
            for layout in &list_x11_layouts() {
                println!("{layout}");
            }
            0
        }
        "--help" | "-h" | "help" => {
            println!("Usage: localectl [command]");
            println!();
            println!("Commands:");
            println!("  status                           Show current settings");
            println!("  set-locale LOCALE=VALUE ...      Set system locale");
            println!("  set-keymap MAP [TOGGLE]          Set VC keymap");
            println!("  set-x11-keymap LAYOUT [MODEL] [VARIANT] [OPTIONS]");
            println!("  list-locales                     List available locales");
            println!("  list-keymaps                     List available keymaps");
            println!("  list-x11-keymap-layouts          List X11 layouts");
            println!("  --help                           Display this help");
            println!("  --version                        Display version");
            0
        }
        "--version" => {
            println!("localectl (SlateOS) {VERSION}");
            0
        }
        other => {
            eprintln!("localectl: unknown command '{other}'");
            1
        }
    };

    process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_locale_settings() {
        let settings = LocaleSettings::default();
        assert!(settings.lang.is_empty());
        assert!(settings.lc_ctype.is_empty());
    }

    #[test]
    fn test_locale_to_map_empty() {
        let settings = LocaleSettings::default();
        let map = settings.to_map();
        assert!(map.is_empty());
    }

    #[test]
    fn test_locale_to_map_with_values() {
        let settings = LocaleSettings {
            lang: "en_US.UTF-8".to_string(),
            lc_time: "de_DE.UTF-8".to_string(),
            ..LocaleSettings::default()
        };
        let map = settings.to_map();
        assert_eq!(map.get("LANG").unwrap(), "en_US.UTF-8");
        assert_eq!(map.get("LC_TIME").unwrap(), "de_DE.UTF-8");
        assert!(!map.contains_key("LC_CTYPE")); // Empty not included.
    }

    #[test]
    fn test_default_keymap_settings() {
        let settings = KeymapSettings::default();
        assert!(settings.keymap.is_empty());
    }

    #[test]
    fn test_default_x11_settings() {
        let settings = X11KeyboardSettings::default();
        assert!(settings.layout.is_empty());
    }

    #[test]
    fn test_read_key_value_file_nonexistent() {
        let map = read_key_value_file("/nonexistent_file_xyz");
        assert!(map.is_empty());
    }

    #[test]
    fn test_list_available_locales() {
        let locales = list_available_locales();
        assert!(!locales.is_empty());
    }

    #[test]
    fn test_list_available_keymaps() {
        let keymaps = list_available_keymaps();
        assert!(!keymaps.is_empty());
    }

    #[test]
    fn test_list_x11_layouts() {
        let layouts = list_x11_layouts();
        assert!(!layouts.is_empty());
    }

    #[test]
    fn test_read_locale_fallback() {
        let locale = read_locale_settings();
        // Should not panic, returns defaults.
        let _ = locale.lang;
    }

    #[test]
    fn test_read_keymap_fallback() {
        let keymap = read_keymap_settings();
        let _ = keymap.keymap;
    }
}
