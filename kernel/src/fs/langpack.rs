//! Language packs — system language and translation management.
//!
//! Manages installed language packs with translation strings,
//! input methods per language, and locale-specific formatting rules.
//! Separate from locale (which handles number/date formats) — this
//! handles actual translated UI strings.
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Language
//!   → langpack::set_system_language() / install()
//!
//! UI toolkit
//!   → langpack::translate(key) → localized string
//!
//! Integration:
//!   → locale (formatting rules)
//!   → ime (input methods per language)
//!   → keylayout (keyboard layout association)
//!   → pkgmgr (language pack installation)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Completeness of a language pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackStatus {
    /// Complete, all strings translated.
    Complete,
    /// Partial — some strings fall back to English.
    Partial,
    /// Not installed.
    NotInstalled,
    /// Installing.
    Installing,
}

impl PackStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Complete => "Complete",
            Self::Partial => "Partial",
            Self::NotInstalled => "Not Installed",
            Self::Installing => "Installing",
        }
    }
}

/// A language pack entry.
#[derive(Debug, Clone)]
pub struct LanguagePack {
    /// Language code (e.g., "en-US", "de-DE").
    pub code: String,
    /// Native name (e.g., "English (US)", "Deutsch").
    pub native_name: String,
    /// English name.
    pub english_name: String,
    /// Status.
    pub status: PackStatus,
    /// Number of translated strings.
    pub string_count: u32,
    /// Completeness percentage (0-100).
    pub completeness_pct: u8,
    /// Pack size in bytes.
    pub size_bytes: u64,
    /// Associated keyboard layout.
    pub keyboard_layout: String,
    /// Text direction: left-to-right or right-to-left.
    pub rtl: bool,
}

/// A translated string entry.
#[derive(Debug, Clone)]
pub struct TranslationEntry {
    /// String key (e.g., "menu.file.open").
    pub key: String,
    /// Translated value.
    pub value: String,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_PACKS: usize = 50;

struct State {
    packs: Vec<LanguagePack>,
    system_language: String,
    fallback_language: String,
    /// Translation table for current language (key → value).
    translations: Vec<TranslationEntry>,
    total_lookups: u64,
    total_misses: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }

    let packs = alloc::vec![
        LanguagePack {
            code: String::from("en-US"), native_name: String::from("English (US)"),
            english_name: String::from("English (US)"), status: PackStatus::Complete,
            string_count: 5000, completeness_pct: 100, size_bytes: 2 * 1024 * 1024,
            keyboard_layout: String::from("us"), rtl: false,
        },
        LanguagePack {
            code: String::from("en-GB"), native_name: String::from("English (UK)"),
            english_name: String::from("English (UK)"), status: PackStatus::Complete,
            string_count: 5000, completeness_pct: 100, size_bytes: 2 * 1024 * 1024,
            keyboard_layout: String::from("gb"), rtl: false,
        },
    ];

    *guard = Some(State {
        packs,
        system_language: String::from("en-US"),
        fallback_language: String::from("en-US"),
        translations: Vec::new(),
        total_lookups: 0,
        total_misses: 0,
        ops: 0,
    });
}

/// Install a language pack.
pub fn install(
    code: &str, native_name: &str, english_name: &str,
    rtl: bool, keyboard_layout: &str,
) -> KernelResult<()> {
    with_state(|state| {
        if state.packs.iter().any(|p| p.code == code && p.status != PackStatus::NotInstalled) {
            return Err(KernelError::AlreadyExists);
        }
        if state.packs.len() >= MAX_PACKS {
            return Err(KernelError::ResourceExhausted);
        }

        // Update existing or add new.
        if let Some(p) = state.packs.iter_mut().find(|p| p.code == code) {
            p.status = PackStatus::Complete;
            p.completeness_pct = 100;
        } else {
            state.packs.push(LanguagePack {
                code: String::from(code),
                native_name: String::from(native_name),
                english_name: String::from(english_name),
                status: PackStatus::Complete,
                string_count: 0,
                completeness_pct: 0,
                size_bytes: 1024 * 1024,
                keyboard_layout: String::from(keyboard_layout),
                rtl,
            });
        }
        Ok(())
    })
}

/// Remove a language pack.
pub fn uninstall(code: &str) -> KernelResult<()> {
    with_state(|state| {
        if code == state.system_language {
            return Err(KernelError::InvalidArgument); // Can't uninstall active language.
        }
        let pos = state.packs.iter().position(|p| p.code == code)
            .ok_or(KernelError::NotFound)?;
        state.packs.remove(pos);
        // Remove translations for this language.
        state.translations.retain(|t| !t.key.starts_with(code));
        Ok(())
    })
}

/// Set the system display language.
pub fn set_system_language(code: &str) -> KernelResult<()> {
    with_state(|state| {
        if !state.packs.iter().any(|p| p.code == code && p.status == PackStatus::Complete) {
            return Err(KernelError::NotFound);
        }
        state.system_language = String::from(code);
        Ok(())
    })
}

/// Get current system language.
pub fn system_language() -> String {
    STATE.lock().as_ref().map_or(String::from("en-US"), |s| s.system_language.clone())
}

/// Add a translation string.
pub fn add_translation(key: &str, value: &str) -> KernelResult<()> {
    with_state(|state| {
        if let Some(t) = state.translations.iter_mut().find(|t| t.key == key) {
            t.value = String::from(value);
        } else {
            state.translations.push(TranslationEntry {
                key: String::from(key),
                value: String::from(value),
            });
        }
        Ok(())
    })
}

/// Translate a key to the current language's string.
pub fn translate(key: &str) -> String {
    let mut guard = STATE.lock();
    match guard.as_mut() {
        Some(state) => {
            state.total_lookups += 1;
            match state.translations.iter().find(|t| t.key == key) {
                Some(t) => t.value.clone(),
                None => {
                    state.total_misses += 1;
                    String::from(key) // Return key as fallback.
                }
            }
        }
        None => String::from(key),
    }
}

/// List installed packs.
pub fn list_packs() -> Vec<LanguagePack> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.packs.clone())
}

/// Get pack info.
pub fn get_pack(code: &str) -> KernelResult<LanguagePack> {
    with_state(|state| {
        state.packs.iter().find(|p| p.code == code).cloned().ok_or(KernelError::NotFound)
    })
}

/// Check if current language is RTL.
pub fn is_rtl() -> bool {
    let guard = STATE.lock();
    guard.as_ref().is_some_and(|s| {
        s.packs.iter().find(|p| p.code == s.system_language).is_some_and(|p| p.rtl)
    })
}

/// Statistics: (pack_count, installed_count, system_lang, lookups, misses, ops).
pub fn stats() -> (usize, usize, String, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let installed = s.packs.iter().filter(|p| p.status == PackStatus::Complete || p.status == PackStatus::Partial).count();
            (s.packs.len(), installed, s.system_language.clone(), s.total_lookups, s.total_misses, s.ops)
        }
        None => (0, 0, String::from("N/A"), 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("langpack::self_test() — running tests...");
    init_defaults();

    // 1: Default packs.
    let packs = list_packs();
    assert!(packs.len() >= 2);
    crate::serial_println!("  [1/11] default packs: OK");

    // 2: System language.
    let lang = system_language();
    assert_eq!(lang, "en-US");
    crate::serial_println!("  [2/11] system language: OK");

    // 3: Install a language pack.
    install("de-DE", "Deutsch", "German", false, "de").expect("install de");
    let pack = get_pack("de-DE").expect("get de");
    assert_eq!(pack.status, PackStatus::Complete);
    crate::serial_println!("  [3/11] install pack: OK");

    // 4: Set system language.
    set_system_language("de-DE").expect("set lang");
    assert_eq!(system_language(), "de-DE");
    crate::serial_println!("  [4/11] set system language: OK");

    // 5: Add translations.
    add_translation("menu.file", "Datei").expect("add trans 1");
    add_translation("menu.edit", "Bearbeiten").expect("add trans 2");
    crate::serial_println!("  [5/11] add translations: OK");

    // 6: Translate.
    let result = translate("menu.file");
    assert_eq!(result, "Datei");
    crate::serial_println!("  [6/11] translate: OK");

    // 7: Translate miss (returns key).
    let result = translate("menu.unknown");
    assert_eq!(result, "menu.unknown");
    crate::serial_println!("  [7/11] translate miss: OK");

    // 8: RTL check.
    assert!(!is_rtl());
    install("ar-SA", "العربية", "Arabic", true, "ar").expect("install ar");
    set_system_language("ar-SA").expect("set ar");
    assert!(is_rtl());
    crate::serial_println!("  [8/11] RTL detection: OK");

    // 9: Can't uninstall active language.
    let result = uninstall("ar-SA");
    assert!(result.is_err());
    crate::serial_println!("  [9/11] uninstall active blocked: OK");

    // 10: Uninstall inactive.
    set_system_language("en-US").expect("switch back");
    uninstall("ar-SA").expect("uninstall ar");
    crate::serial_println!("  [10/11] uninstall: OK");

    // 11: Stats.
    let (total, installed, _lang, lookups, misses, ops) = stats();
    assert!(total >= 3);
    assert!(installed >= 2);
    assert!(lookups >= 2);
    assert!(misses >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("langpack::self_test() — all 11 tests passed");
}
