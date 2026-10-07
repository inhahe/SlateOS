//! The file a user keeps their window rules in: `window-rules.yaml` in their
//! configuration directory, read by the desktop shell and written by
//! Settings.
//!
//! ```yaml
//! # Window rules: what happens to a program's windows as they open.
//! rules:
//!   Terminal remembers its place:
//!     app: terminal
//!     position: remember
//!     size: remember
//!   Notes on the second desktop:
//!     title-contains: notes
//!     desktop: 2
//!     taskbar: false
//! ```
//!
//! Each rule is keyed by its name, and the order they are written in is the
//! order of their priority: the rule nearest the top wins where two that match
//! a window disagree. A rule matches by exactly one of `app` (the program the
//! window belongs to), `title` (its whole title) or `title-contains` (part of
//! it, in any case), or `any: true`. The rest of its keys are what it does --
//! see [`read`] for each, and the numbers people count from one (desktops,
//! monitors) are written counting from one.
//!
//! # A rule this cannot read is not applied at all
//!
//! Not half-applied, and not widened. A misspelt key or a value out of range
//! makes the whole rule a [`Problem`], reported and left out: a rule that said
//! `can-close: flase` must not become a rule that lets a window be closed, and
//! a misspelt `app` must not become `any` -- which would apply the rule's
//! actions to every window the user opens. The rest of the file is read as
//! usual.
//!
//! # The rules a file has are the whole of a user's rules
//!
//! A file that says nothing about rules -- no `rules` key, which is what no
//! file at all reads as -- means the built-in defaults
//! ([`default_rules`](crate::default_rules)); one that has a `rules` key has
//! what it says there and nothing else, even when that is nothing. [`read`]
//! answers `None` for the first, so the two are never confused, and [`write()`]
//! always leaves the key, so a user who removes every rule keeps none rather
//! than getting the defaults back.
//!
//! The line is drawn at the key rather than at whether the file exists
//! because that is what a reader can see: `settingsfile`'s watcher, which is
//! how the shell notices Settings changing the rules, reads a missing file
//! and an unreadable one as an empty document, as `settingsfile::load` does.

use std::fmt;
use std::io;

use yamldoc::Document;

use crate::{
    InitialState, MAX_RULES, MatchCriteria, PositionSpec, RuleActions, SizeSpec, WindowRule,
};

/// The settings file's name: `window-rules.yaml`.
pub const CONFIG_NAME: &str = "window-rules";

/// The mapping the rules are written under.
const RULES: &str = "rules";

/// What a new file opens with, above the rules.
const HEADER: &str = "\
# Window rules: what happens to a program's windows as they open.
#
# Each rule is named, and matches a window by exactly one of
#   app: <program>          the program it belongs to
#   title: <title>          its whole title
#   title-contains: <text>  part of its title, in any case
#   any: true               every window
# The rule nearest the top wins where two disagree. A rule that cannot be
# read -- a misspelt key, a value out of range -- is not applied at all.
";

/// A rule that could not be read, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// The rule's name, as the file has it -- empty when what could not be
    /// read is the rules as a whole: `rules: none`, say.
    pub rule: String,
    /// What is wrong with it.
    pub what: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.rule.is_empty() {
            write!(f, "no window rule is applied: {}", self.what)
        } else {
            write!(
                f,
                "window rule {:?} is not applied: {}",
                self.rule, self.what
            )
        }
    }
}

/// What a file held: the rules that could be read, in priority order --
/// highest first -- and why the rest could not.
#[derive(Clone, Debug, Default)]
pub struct Loaded {
    /// The rules, highest priority first.
    pub rules: Vec<WindowRule>,
    /// The rules left out, each with its reason.
    pub problems: Vec<Problem>,
}

/// The rules in `doc`; `None` when it says nothing about rules, which means
/// the defaults.
///
/// A `rules` key with nothing under it -- or `~`, `null`, `{}` -- is a user
/// with no rules. One holding anything else that is not a rule apiece is a
/// [`Problem`], and no rules: the file meant to say something, and applying
/// the defaults instead would be guessing what.
///
/// Every key a rule may have, besides how it matches:
///
/// | Key | Value | What it does |
/// |---|---|---|
/// | `enabled` | `true`/`false` | `false` keeps the rule without applying it |
/// | `once` | `true`/`false` | applied to the first window it matches, then forgotten |
/// | `position` | `remember`, `centre`, `centre N`, `X Y`, `X% Y%` | where the window goes |
/// | `size` | `remember`, `W H`, `W% H%` | how big it is |
/// | `desktop` | 1, 2, ... | which virtual desktop |
/// | `state` | `normal`, `minimized`, `maximized`, `fullscreen` | how it opens |
/// | `on-top`, `on-bottom` | `true`/`false` | kept above, or below, other windows |
/// | `opacity` | 0 to 1 | how see-through |
/// | `taskbar`, `alt-tab` | `true`/`false` | whether it has a taskbar button, a place in Alt+Tab |
/// | `monitor` | 1, 2, ... | which monitor |
/// | `decorations` | `true`/`false` | whether it has a title bar |
/// | `min-size`, `max-size` | `W H` | limits on its size |
/// | `can-close`, `can-move`, `can-resize` | `true`/`false` | what the user may do to it |
/// | `snap` | a snap slot's number | the zone it snaps into |
/// | `tray` | `true`/`false` | minimised, it goes to the system tray, not the taskbar; with `state: minimized`, it starts there |
#[must_use]
pub fn read(doc: &Document) -> Option<Loaded> {
    if !doc.contains(&[RULES]) {
        return None;
    }
    let mut loaded = Loaded::default();
    let whole = |what: String| Problem {
        rule: String::new(),
        what,
    };
    if let Some(text) = doc.get_str(&[RULES]) {
        let text = text.trim();
        if !matches!(text, "" | "~" | "null" | "{}") {
            loaded.problems.push(whole(format!(
                "`rules` holds rules, each under its name, not {text:?}"
            )));
        }
        return Some(loaded);
    }
    if doc.get_seq(&[RULES]).is_some_and(|items| !items.is_empty()) {
        loaded.problems.push(whole(
            "the rules under `rules` are each written under a name, not listed with `-`".to_owned(),
        ));
        return Some(loaded);
    }
    let names = doc.keys(&[RULES]);
    let mut seen: Vec<&str> = Vec::with_capacity(names.len());
    for name in &names {
        if seen.contains(&name.as_str()) {
            loaded.problems.push(Problem {
                rule: name.clone(),
                what: "another rule above has the same name".to_owned(),
            });
            continue;
        }
        seen.push(name.as_str());
        if loaded.rules.len() >= MAX_RULES {
            loaded.problems.push(Problem {
                rule: name.clone(),
                what: format!("only {MAX_RULES} rules are kept"),
            });
            continue;
        }
        match read_rule(doc, name, loaded.rules.len()) {
            Ok(rule) => loaded.rules.push(rule),
            Err(what) => loaded.problems.push(Problem {
                rule: name.clone(),
                what,
            }),
        }
    }
    Some(loaded)
}

/// The rule named `name`, the `index`th readable one.
fn read_rule(doc: &Document, name: &str, index: usize) -> Result<WindowRule, String> {
    let keys = doc.keys(&[RULES, name]);
    let value = |key: &str| doc.get_str(&[RULES, name, key]);
    let criteria = criteria(&keys, &value)?;
    let mut rule = WindowRule::new(0, name, criteria);
    // Priority by position: the first rule highest. At most MAX_RULES, so the
    // count always fits.
    rule.priority = i32::try_from(index).map_or(i32::MIN, |at| at.saturating_neg());
    for key in &keys {
        let Some(text) = value(key) else {
            return Err(format!("`{key}` has no value"));
        };
        let text = text.trim();
        let a = &mut rule.actions;
        match key.as_str() {
            "app" | "title" | "title-contains" | "any" => {}
            "enabled" => rule.enabled = flag(key, text)?,
            "once" => rule.one_shot = flag(key, text)?,
            "position" => a.position = Some(position(text)?),
            "size" => a.size = Some(size(text)?),
            "desktop" => a.desktop = Some(counted(key, text)?),
            "state" => a.initial_state = Some(state(text)?),
            "on-top" => a.always_on_top = Some(flag(key, text)?),
            "on-bottom" => a.always_on_bottom = Some(flag(key, text)?),
            "opacity" => a.opacity = Some(opacity(text)?),
            "taskbar" => a.skip_taskbar = Some(!flag(key, text)?),
            "alt-tab" => a.skip_alt_tab = Some(!flag(key, text)?),
            "monitor" => a.target_monitor = Some(counted(key, text)?),
            "decorations" => a.no_decorations = Some(!flag(key, text)?),
            "min-size" => a.min_size = Some(pair(key, text)?),
            "max-size" => a.max_size = Some(pair(key, text)?),
            "can-close" => a.prevent_close = Some(!flag(key, text)?),
            "can-move" => a.prevent_move = Some(!flag(key, text)?),
            "can-resize" => a.prevent_resize = Some(!flag(key, text)?),
            "snap" => {
                a.snap_zone = Some(
                    text.parse()
                        .map_err(|_| format!("`snap` is not a slot number: {text:?}"))?,
                );
            }
            "tray" => a.to_tray = Some(flag(key, text)?),
            other => return Err(format!("`{other}` is not something a rule can say")),
        }
    }
    Ok(rule)
}

/// How a rule matches: exactly one of its four ways.
fn criteria(
    keys: &[String],
    value: &dyn Fn(&str) -> Option<String>,
) -> Result<MatchCriteria, String> {
    let ways: Vec<&str> = ["app", "title", "title-contains", "any"]
        .into_iter()
        .filter(|way| keys.iter().any(|key| key == way))
        .collect();
    let [way] = ways.as_slice() else {
        return Err(if ways.is_empty() {
            "it says nothing to match: `app`, `title`, `title-contains` or `any`".to_owned()
        } else {
            format!("it matches more than one way: {}", ways.join(", "))
        });
    };
    let text = value(way).unwrap_or_default();
    let text = text.trim();
    if text.is_empty() {
        return Err(format!("`{way}` is empty"));
    }
    Ok(match *way {
        "app" => MatchCriteria::AppId(text.to_owned()),
        "title" => MatchCriteria::TitleExact(text.to_owned()),
        "title-contains" => MatchCriteria::TitleContains(text.to_owned()),
        _ => {
            if !flag("any", text)? {
                return Err("`any: false` matches nothing".to_owned());
            }
            MatchCriteria::Any
        }
    })
}

fn flag(key: &str, text: &str) -> Result<bool, String> {
    match text {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(format!("`{key}` is `true` or `false`, not {other:?}")),
    }
}

/// A number counted from one, as the model's from nought.
fn counted(key: &str, text: &str) -> Result<u32, String> {
    text.parse::<u32>()
        .ok()
        .and_then(|n| n.checked_sub(1))
        .ok_or_else(|| format!("`{key}` counts from 1, not {text:?}"))
}

/// `W H`, two whole numbers.
fn pair(key: &str, text: &str) -> Result<(u32, u32), String> {
    let mut parts = text.split_whitespace();
    match (
        parts.next().and_then(|w| w.parse().ok()),
        parts.next().and_then(|h| h.parse().ok()),
        parts.next(),
    ) {
        (Some(w), Some(h), None) => Ok((w, h)),
        _ => Err(format!("`{key}` is a width and a height, not {text:?}")),
    }
}

/// `N%` as a fraction, from 0 to 1.
fn percent(text: &str) -> Option<f32> {
    let number: f32 = text.strip_suffix('%')?.parse().ok()?;
    (0.0..=100.0).contains(&number).then_some(number / 100.0)
}

fn position(text: &str) -> Result<PositionSpec, String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let refused = || {
        format!("`position` is `remember`, `centre`, `centre N`, `X Y` or `X% Y%`, not {text:?}")
    };
    Ok(match words.as_slice() {
        ["remember"] => PositionSpec::RememberLast,
        ["centre" | "center"] => PositionSpec::CenterOnMonitor(0),
        ["centre" | "center", monitor] => {
            PositionSpec::CenterOnMonitor(counted("position", monitor).map_err(|_| refused())?)
        }
        [x, y] if x.ends_with('%') => PositionSpec::Percentage {
            x_pct: percent(x).ok_or_else(refused)?,
            y_pct: percent(y).ok_or_else(refused)?,
        },
        [x, y] => PositionSpec::Absolute {
            x: x.parse().map_err(|_| refused())?,
            y: y.parse().map_err(|_| refused())?,
        },
        _ => return Err(refused()),
    })
}

fn size(text: &str) -> Result<SizeSpec, String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let refused = || format!("`size` is `remember`, `W H` or `W% H%`, not {text:?}");
    Ok(match words.as_slice() {
        ["remember"] => SizeSpec::RememberLast,
        [w, h] if w.ends_with('%') => SizeSpec::Percentage {
            w_pct: percent(w).ok_or_else(refused)?,
            h_pct: percent(h).ok_or_else(refused)?,
        },
        [_, _] => {
            let (width, height) = pair("size", text).map_err(|_| refused())?;
            SizeSpec::Exact { width, height }
        }
        _ => return Err(refused()),
    })
}

fn state(text: &str) -> Result<InitialState, String> {
    Ok(match text {
        "normal" => InitialState::Normal,
        "minimized" | "minimised" => InitialState::Minimized,
        "maximized" | "maximised" => InitialState::Maximized,
        "fullscreen" => InitialState::Fullscreen,
        other => {
            return Err(format!(
                "`state` is `normal`, `minimized`, `maximized` or `fullscreen`, not {other:?}"
            ));
        }
    })
}

fn opacity(text: &str) -> Result<f32, String> {
    text.parse::<f32>()
        .ok()
        .filter(|o| (0.0..=1.0).contains(o))
        .ok_or_else(|| format!("`opacity` is a number from 0 to 1, not {text:?}"))
}

/// Write `rules` -- highest priority first -- into `doc` as its rules,
/// replacing what it had there and leaving everything else as it was.
///
/// The `rules` key is written even for no rules at all, so that what reads
/// back is "none" and not the defaults ([`read`]).
pub fn write(rules: &[&WindowRule], doc: &mut Document) {
    if doc.contains(&[RULES]) {
        // Something that is not a rule apiece -- `rules: none`, a `-` list --
        // goes first, whole: what is written is exactly the rules given, and
        // a named rule written beside a list would not be YAML at all.
        let scalar = doc.get_str(&[RULES]).is_some();
        let listed = doc.get_seq(&[RULES]).is_some_and(|items| !items.is_empty());
        if scalar || listed {
            doc.set_seq(&[RULES], &[]);
        }
        // Empty the block and write into it, so the rules stay where the user
        // had them among their comments. Removing the block itself would move
        // it: a key yamldoc adds goes after the file's last line that is not
        // a comment, which in a file of comments and rules is the top.
        for name in doc.keys(&[RULES]) {
            doc.remove(&[RULES, &name]);
        }
        put_rules(rules, doc);
    } else {
        // No block: the rules go at the end, below whatever the file says --
        // spliced as text for the reason above, since through yamldoc they
        // would land above a header of comments.
        let mut text = doc.to_text();
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        if rules.is_empty() {
            text.push_str(RULES);
            text.push_str(":\n");
        } else {
            let mut fresh = Document::new();
            put_rules(rules, &mut fresh);
            text.push_str(&fresh.to_text());
        }
        *doc = Document::parse(&text);
    }
}

/// Write `rules` under the rules block of `doc`.
fn put_rules(rules: &[&WindowRule], doc: &mut Document) {
    for rule in rules {
        let mut put = |key: &str, value: Value| set(doc, &rule.name, key, value);
        match &rule.criteria {
            MatchCriteria::AppId(app) => put("app", Value::Text(app.clone())),
            MatchCriteria::TitleExact(title) => put("title", Value::Text(title.clone())),
            MatchCriteria::TitleContains(part) => put("title-contains", Value::Text(part.clone())),
            MatchCriteria::Any => put("any", Value::Flag(true)),
        }
        if !rule.enabled {
            put("enabled", Value::Flag(false));
        }
        if rule.one_shot {
            put("once", Value::Flag(true));
        }
        write_actions(&rule.actions, &mut put);
    }
}

/// One value as the file writes it: a flag as `true`/`false` and a number
/// as a number -- not as text that happens to spell one, which the YAML
/// writer would quote.
enum Value {
    Text(String),
    Flag(bool),
    Whole(i64),
    Real(f64),
}

/// A number the model counts from nought, as people count it: from one.
fn from_one(n: u32) -> Value {
    Value::Whole(i64::from(n).saturating_add(1))
}

/// Set `key` of the rule `name` to `value`.
fn set(doc: &mut Document, name: &str, key: &str, value: Value) {
    let path = [RULES, name, key];
    match value {
        Value::Text(text) => doc.set_str(&path, &text),
        Value::Flag(on) => doc.set_bool(&path, on),
        Value::Whole(n) => doc.set_i64(&path, n),
        Value::Real(x) => doc.set_f64(&path, x),
    }
}

fn write_actions(a: &RuleActions, put: &mut dyn FnMut(&str, Value)) {
    if let Some(position) = a.position {
        put(
            "position",
            Value::Text(match position {
                PositionSpec::RememberLast => "remember".to_owned(),
                PositionSpec::CenterOnMonitor(0) => "centre".to_owned(),
                PositionSpec::CenterOnMonitor(m) => {
                    format!("centre {}", u64::from(m).saturating_add(1))
                }
                PositionSpec::Absolute { x, y } => format!("{x} {y}"),
                PositionSpec::Percentage { x_pct, y_pct } => {
                    format!("{}% {}%", x_pct * 100.0, y_pct * 100.0)
                }
            }),
        );
    }
    if let Some(size) = a.size {
        put(
            "size",
            Value::Text(match size {
                SizeSpec::RememberLast => "remember".to_owned(),
                SizeSpec::Exact { width, height } => format!("{width} {height}"),
                SizeSpec::Percentage { w_pct, h_pct } => {
                    format!("{}% {}%", w_pct * 100.0, h_pct * 100.0)
                }
            }),
        );
    }
    if let Some(desktop) = a.desktop {
        put("desktop", from_one(desktop));
    }
    if let Some(state) = a.initial_state {
        let word = match state {
            InitialState::Normal => "normal",
            InitialState::Minimized => "minimized",
            InitialState::Maximized => "maximized",
            InitialState::Fullscreen => "fullscreen",
        };
        put("state", Value::Text(word.to_owned()));
    }
    if let Some(on) = a.always_on_top {
        put("on-top", Value::Flag(on));
    }
    if let Some(on) = a.always_on_bottom {
        put("on-bottom", Value::Flag(on));
    }
    if let Some(opacity) = a.opacity {
        put("opacity", Value::Real(f64::from(opacity)));
    }
    if let Some(skip) = a.skip_taskbar {
        put("taskbar", Value::Flag(!skip));
    }
    if let Some(skip) = a.skip_alt_tab {
        put("alt-tab", Value::Flag(!skip));
    }
    if let Some(monitor) = a.target_monitor {
        put("monitor", from_one(monitor));
    }
    if let Some(none) = a.no_decorations {
        put("decorations", Value::Flag(!none));
    }
    if let Some((w, h)) = a.min_size {
        put("min-size", Value::Text(format!("{w} {h}")));
    }
    if let Some((w, h)) = a.max_size {
        put("max-size", Value::Text(format!("{w} {h}")));
    }
    if let Some(prevent) = a.prevent_close {
        put("can-close", Value::Flag(!prevent));
    }
    if let Some(prevent) = a.prevent_move {
        put("can-move", Value::Flag(!prevent));
    }
    if let Some(prevent) = a.prevent_resize {
        put("can-resize", Value::Flag(!prevent));
    }
    if let Some(slot) = a.snap_zone {
        put("snap", Value::Whole(i64::from(slot)));
    }
    if let Some(tray) = a.to_tray {
        put("tray", Value::Flag(tray));
    }
}

/// The user's rules, as [`read`] finds them in their file: `None` when it
/// says nothing about rules -- or there is no file -- so the defaults are
/// theirs.
///
/// For a program that reads the rules once. One that holds them while
/// Settings may change them watches the file instead, with a
/// `settingsfile::Watcher` on [`CONFIG_NAME`], and hands each document it
/// reports to [`read`]: the desktop shell does.
#[must_use]
pub fn load() -> Option<Loaded> {
    read(&settingsfile::load(CONFIG_NAME))
}

/// Write `rules` -- highest priority first -- as the user's rules, keeping
/// whatever else their file says (its comments above all).
///
/// # Errors
///
/// As `settingsfile::store`: no configuration directory, or the write failed.
pub fn store(rules: &[&WindowRule]) -> io::Result<()> {
    let mut doc = settingsfile::load(CONFIG_NAME);
    // A new file -- or an empty one -- opens with what it is for, above the
    // rules.
    if doc.is_empty() {
        doc = Document::parse(HEADER);
    }
    write(rules, &mut doc);
    settingsfile::store(CONFIG_NAME, &doc)
}

#[cfg(test)]
#[path = "file_tests.rs"]
mod tests;
