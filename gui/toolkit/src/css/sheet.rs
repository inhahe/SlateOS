//! Style sheets and declaration blocks: what a program writes.
//!
//! A **block** is a widget's own style -- `color: red; padding: 4px` -- with
//! blocks inside it for its states: `&:hover { background-color:
//! var(--surface1) }` (the `&` may be left out). A **style sheet** is rules,
//! each a list of selectors and a block:
//!
//! ```css
//! Button { padding: 4px 12px }
//! .danger { color: var(--red) }
//! Toolbar > Button:hover, #save { font-weight: bold }
//! ```
//!
//! A selector is a widget's kind (`Button`, `Label`, `*` for any), its
//! classes (`.danger`), its name (`#save`) and its states (`:hover`,
//! `:active`, `:focus`, `:disabled`, `:enabled`, `:checked`), joined by `>`
//! for "a child of". The descendant combinator (a space), the sibling
//! combinators and specificity are not CSS this reads: rules apply in the
//! order written (`design-decisions.md` §1478).
//!
//! Parsing is lenient, as CSS's is: a declaration that cannot be read, or a
//! rule whose selector cannot, is dropped with a [`Warning`] and the rest
//! kept.

use super::decl::{self, Declared};
use super::token::{Spanned, Token, tokenize};

/// Something in a style that was not used, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    /// The byte offset in the text it is about.
    pub at: usize,
    /// What, and why.
    pub message: String,
}

/// A widget's state a selector or a block can ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    /// The pointer is over it.
    Hover,
    /// It is being pressed.
    Active,
    /// It has the keyboard.
    Focus,
    /// It is disabled.
    Disabled,
    /// It is enabled.
    Enabled,
    /// It is checked: a ticked box, a chosen radio button.
    Checked,
}

impl State {
    fn named(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "hover" => Some(Self::Hover),
            "active" => Some(Self::Active),
            "focus" | "focus-visible" | "focus-within" => Some(Self::Focus),
            "disabled" => Some(Self::Disabled),
            "enabled" => Some(Self::Enabled),
            "checked" => Some(Self::Checked),
            _ => None,
        }
    }
}

/// What a widget is, as far as a selector asks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Subject<'a> {
    /// Its kind's name: `Button`, `Label`, ...
    pub kind: &'a str,
    /// Its classes.
    pub classes: &'a [String],
    /// Its name, if it has one.
    pub name: Option<&'a str>,
    /// Whether the pointer is over it.
    pub hover: bool,
    /// Whether it is being pressed.
    pub active: bool,
    /// Whether it has the keyboard.
    pub focus: bool,
    /// Whether it is enabled.
    pub enabled: bool,
    /// Whether it is checked.
    pub checked: bool,
}

impl Subject<'_> {
    /// Whether it is in `state`.
    #[must_use]
    pub const fn is(&self, state: State) -> bool {
        match state {
            State::Hover => self.hover,
            State::Active => self.active,
            State::Focus => self.focus,
            State::Disabled => !self.enabled,
            State::Enabled => self.enabled,
            State::Checked => self.checked,
        }
    }
}

/// One compound selector: a kind, classes, a name and states, all of which
/// must hold.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Compound {
    /// The kind, or `None` for any (`*`, or none written).
    pub kind: Option<String>,
    /// The classes it must have.
    pub classes: Vec<String>,
    /// The name it must have.
    pub name: Option<String>,
    /// The states it must be in.
    pub states: Vec<State>,
}

impl Compound {
    /// Whether `subject` is what this asks for.
    #[must_use]
    pub fn matches(&self, subject: &Subject<'_>) -> bool {
        self.kind.as_deref().is_none_or(|k| k == subject.kind)
            && self.classes.iter().all(|c| subject.classes.contains(c))
            && self.name.as_deref().is_none_or(|n| subject.name == Some(n))
            && self.states.iter().all(|s| subject.is(*s))
    }
}

/// A selector: compound selectors joined by `>`, the subject last.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selector {
    /// The compounds, outermost first: `A > B > C` is `[A, B, C]`.
    pub parts: Vec<Compound>,
}

impl Selector {
    /// Whether it selects the last of `path` -- a widget, after its
    /// ancestors from the root.
    #[must_use]
    pub fn matches(&self, path: &[Subject<'_>]) -> bool {
        if self.parts.len() > path.len() {
            return false;
        }
        let tail = path.len().saturating_sub(self.parts.len());
        self.parts
            .iter()
            .zip(path.get(tail..).unwrap_or(&[]))
            .all(|(c, s)| c.matches(s))
    }
}

/// A block: declarations, and the declarations a state adds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Block {
    /// The declarations that always apply, in order.
    pub base: Vec<Declared>,
    /// Declarations that apply in states: each block applies when the
    /// widget is in all its states, after the base, in the order written.
    pub states: Vec<(Vec<State>, Vec<Declared>)>,
}

impl Block {
    /// The declarations that apply to `subject`, in order.
    pub fn applying<'b>(&'b self, subject: &Subject<'_>) -> impl Iterator<Item = &'b Declared> {
        let states: Vec<&'b Vec<Declared>> = self
            .states
            .iter()
            .filter(|(need, _)| need.iter().all(|s| subject.is(*s)))
            .map(|(_, d)| d)
            .collect();
        self.base.iter().chain(states.into_iter().flatten())
    }

    /// Whether it declares nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.base.is_empty() && self.states.is_empty()
    }
}

/// A rule: selectors and the block they choose widgets for.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    /// Any one of them chooses a widget.
    pub selectors: Vec<Selector>,
    /// What the chosen widgets are given.
    pub block: Block,
}

/// A style sheet: rules, applied in the order written.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleSheet {
    /// The rules, in order.
    pub rules: Vec<Rule>,
}

impl StyleSheet {
    /// The declarations it gives the last of `path`, in order.
    pub fn applying<'s>(&'s self, path: &[Subject<'_>]) -> Vec<&'s Declared> {
        let Some(subject) = path.last() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for rule in &self.rules {
            // Each of a rule's selectors may hold in a different state; the
            // rule's block applies once if any does, its state blocks by the
            // widget's own state.
            if rule.selectors.iter().any(|s| s.matches(path)) {
                out.extend(rule.block.applying(subject));
            }
        }
        out
    }
}

/// `text` read as a widget's own block: declarations, and state blocks.
#[must_use]
pub fn parse_block(text: &str) -> (Block, Vec<Warning>) {
    let tokens = tokenize(text);
    let mut warnings = Vec::new();
    let block = block_of(&tokens, &mut warnings, true);
    (block, warnings)
}

/// `text` read as a style sheet.
#[must_use]
pub fn parse_sheet(text: &str) -> (StyleSheet, Vec<Warning>) {
    let tokens = tokenize(text);
    let mut warnings = Vec::new();
    let mut rules = Vec::new();
    let mut i = 0usize;
    while i < tokens.len() {
        // The prelude: up to `{`, or a `;` for an at-rule with no block.
        let start = i;
        while let Some(s) = tokens.get(i) {
            if matches!(s.token, Token::OpenBrace | Token::Semicolon) {
                break;
            }
            i = i.saturating_add(1);
        }
        let prelude = tokens.get(start..i).unwrap_or(&[]);
        let at = prelude.first().map_or(0, |s| s.at);
        match tokens.get(i).map(|s| &s.token) {
            Some(Token::Semicolon) => {
                i = i.saturating_add(1);
                if !is_blank(prelude) {
                    warnings.push(Warning {
                        at,
                        message: "a statement outside any rule is not read".to_string(),
                    });
                }
                continue;
            }
            None => {
                if !is_blank(prelude) {
                    warnings.push(Warning {
                        at,
                        message: "a rule has no block".to_string(),
                    });
                }
                break;
            }
            _ => {}
        }
        // The block: up to the matching `}`.
        let body_start = i.saturating_add(1);
        let body_end = matching_brace(&tokens, i);
        let body = tokens.get(body_start..body_end).unwrap_or(&[]);
        i = body_end.saturating_add(1);
        if let Some(Token::AtKeyword(name)) = prelude
            .iter()
            .find(|s| s.token != Token::Whitespace)
            .map(|s| &s.token)
        {
            warnings.push(Warning {
                at,
                message: format!("`@{name}` is not read: a style sheet here has rules only"),
            });
            continue;
        }
        match selectors(prelude) {
            Ok(selectors) => rules.push(Rule {
                selectors,
                block: block_of(body, &mut warnings, true),
            }),
            Err(why) => warnings.push(Warning { at, message: why }),
        }
    }
    (StyleSheet { rules }, warnings)
}

fn is_blank(tokens: &[Spanned]) -> bool {
    tokens.iter().all(|s| s.token == Token::Whitespace)
}

/// The index of the `}` that closes the `{` at `open`, or the end.
fn matching_brace(tokens: &[Spanned], open: usize) -> usize {
    let mut depth = 0usize;
    let mut i = open;
    while let Some(s) = tokens.get(i) {
        match s.token {
            Token::OpenBrace => depth = depth.saturating_add(1),
            Token::CloseBrace => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
        i = i.saturating_add(1);
    }
    tokens.len()
}

/// A block's tokens read: declarations, and -- where `nested` -- blocks for
/// states (`&:hover { ... }`).
fn block_of(tokens: &[Spanned], warnings: &mut Vec<Warning>, nested: bool) -> Block {
    let mut block = Block::default();
    let mut i = 0usize;
    while i < tokens.len() {
        let start = i;
        // To the end of this declaration, or to a nested block's `{`.
        while let Some(s) = tokens.get(i) {
            if matches!(s.token, Token::Semicolon | Token::OpenBrace) {
                break;
            }
            i = i.saturating_add(1);
        }
        let part = tokens.get(start..i).unwrap_or(&[]);
        let at = part
            .iter()
            .find(|s| s.token != Token::Whitespace)
            .map_or(0, |s| s.at);
        if let Some(Token::OpenBrace) = tokens.get(i).map(|s| &s.token) {
            let end = matching_brace(tokens, i);
            let inner = tokens.get(i.saturating_add(1)..end).unwrap_or(&[]);
            i = end.saturating_add(1);
            if !nested {
                warnings.push(Warning {
                    at,
                    message: "a block inside a state's block is not read".to_string(),
                });
                continue;
            }
            match states_of(part) {
                Ok(states) => {
                    let inner = block_of(inner, warnings, false);
                    block.states.push((states, inner.base));
                }
                Err(why) => warnings.push(Warning { at, message: why }),
            }
            continue;
        }
        i = i.saturating_add(1);
        if is_blank(part) {
            continue;
        }
        match declaration(part) {
            Ok(d) => block.base.extend(d),
            Err(why) => warnings.push(Warning { at, message: why }),
        }
    }
    block
}

/// `name: value`, its `!important` refused.
fn declaration(part: &[Spanned]) -> Result<Vec<Declared>, String> {
    let mut it = part.iter().filter(|s| s.token != Token::Whitespace);
    let name = match it.next().map(|s| &s.token) {
        Some(Token::Ident(name)) => name.clone(),
        other => return Err(format!("{other:?} is not a property's name")),
    };
    let colon = part
        .iter()
        .position(|s| s.token == Token::Colon)
        .ok_or_else(|| format!("`{name}` has no `:`"))?;
    let value = part.get(colon.saturating_add(1)..).unwrap_or(&[]);
    if let Some(bang) = value.iter().position(|s| s.token == Token::Delim('!')) {
        let rest = value.get(bang..).unwrap_or(&[]);
        if rest
            .iter()
            .any(|s| matches!(&s.token, Token::Ident(w) if w.eq_ignore_ascii_case("important")))
        {
            return Err(format!(
                "`{name}`: `!important` is not read -- a later declaration is the one that wins"
            ));
        }
    }
    decl::declare(&name, value)
}

/// `&:hover:focus` -- or `:hover` -- read as the states it asks for.
fn states_of(part: &[Spanned]) -> Result<Vec<State>, String> {
    let mut tokens = part
        .iter()
        .filter(|s| s.token != Token::Whitespace)
        .peekable();
    if matches!(tokens.peek().map(|s| &s.token), Some(Token::Delim('&'))) {
        tokens.next();
    }
    let mut states = Vec::new();
    while let Some(s) = tokens.next() {
        if s.token != Token::Colon {
            return Err("a block inside a block is for a state: `&:hover { ... }`".to_string());
        }
        match tokens.next().map(|s| &s.token) {
            Some(Token::Ident(name)) => {
                states.push(
                    State::named(name)
                        .ok_or_else(|| format!("`:{name}` is not a state this reads"))?,
                );
            }
            other => return Err(format!("{other:?} where a state was wanted")),
        }
    }
    if states.is_empty() {
        return Err("a state's block names no state".to_string());
    }
    Ok(states)
}

/// A rule's prelude read as its selectors.
fn selectors(prelude: &[Spanned]) -> Result<Vec<Selector>, String> {
    prelude
        .split(|s| s.token == Token::Comma)
        .map(selector)
        .collect()
}

/// One selector: compounds joined by `>`.
fn selector(tokens: &[Spanned]) -> Result<Selector, String> {
    let mut parts = Vec::new();
    let mut current = Compound::default();
    let mut started = false;
    let mut i = 0usize;
    let mut space = false;
    while let Some(s) = tokens.get(i) {
        i = i.saturating_add(1);
        match &s.token {
            Token::Whitespace => {
                space = true;
                continue;
            }
            Token::Delim('>') => {
                if !started {
                    return Err("`>` with nothing before it".to_string());
                }
                parts.push(std::mem::take(&mut current));
                started = false;
                space = false;
                continue;
            }
            // Before the space between them is taken for a descendant.
            Token::Delim('+' | '~') => {
                return Err("the sibling combinators (`+`, `~`) are not read".to_string());
            }
            _ => {}
        }
        if space && started {
            return Err(
                "a space between selectors (any descendant) is not read: use `>` for a child"
                    .to_string(),
            );
        }
        space = false;
        match &s.token {
            Token::Ident(kind) if !started => {
                current.kind = Some(kind.clone());
            }
            Token::Delim('*') if !started => {}
            Token::Delim('.') => match tokens.get(i).map(|s| &s.token) {
                Some(Token::Ident(class)) => {
                    i = i.saturating_add(1);
                    current.classes.push(class.clone());
                }
                _ => return Err("`.` with no class after it".to_string()),
            },
            Token::Hash(name) => {
                if current.name.is_some() {
                    return Err("a selector names two names".to_string());
                }
                current.name = Some(name.clone());
            }
            Token::Colon => match tokens.get(i).map(|s| &s.token) {
                Some(Token::Ident(name)) => {
                    i = i.saturating_add(1);
                    current.states.push(
                        State::named(name)
                            .ok_or_else(|| format!("`:{name}` is not a state this reads"))?,
                    );
                }
                _ => return Err("`:` with no state after it".to_string()),
            },
            Token::OpenBracket => return Err("attribute selectors are not read".to_string()),
            other => return Err(format!("{other:?} in a selector")),
        }
        started = true;
    }
    if !started {
        return Err(if parts.is_empty() {
            "a rule with no selector".to_string()
        } else {
            "`>` with nothing after it".to_string()
        });
    }
    parts.push(current);
    Ok(Selector { parts })
}

#[cfg(test)]
#[path = "sheet_tests.rs"]
mod tests;
