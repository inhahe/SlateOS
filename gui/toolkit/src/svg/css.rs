//! `<style>` sheets: the rules a document's style elements hold, applied
//! before anything is built.
//!
//! A rule's declarations are merged into the `style` attribute of each element
//! it selects, *ahead* of what that attribute says itself. The rest of the
//! renderer reads a property from `style` first -- the last declaration of it
//! winning -- and from a presentation attribute only when `style` says
//! nothing, so the cascade comes out as CSS has it: an element's own `style`
//! over the sheet, the sheet over presentation attributes (which weigh nothing
//! against any rule), among rules the more specific and then the later, a
//! rule's `!important` declarations over the element's own `style`, and the
//! element's own `!important` ones over everything. (What reads `style` takes
//! an important declaration over any that is not, wherever the two stand.)
//!
//! Illustrator writes its colours this way -- `.st0{fill:#3a6ea5}` and
//! `class="st0"` -- and so does most of what is exported for the web; a
//! renderer that read no sheet drew all of it in the default black.
//!
//! # What is read
//!
//! - **Selectors**: `*`, a type (`rect`), a class (`.st0`), an id (`#logo`),
//!   any compound of them (`path.st0.lit`), comma lists, and the descendant
//!   and child combinators (`g .a`, `g > .a`).
//! - **Declarations**, `!important` ones included, in any spelling CSS
//!   allows -- `! IMPORTANT` is one.
//! - **Comments** anywhere, in a sheet or an element's own `style`, as CSS
//!   reads them: a comment is no space, so `rect/**/.a` is `rect.a`, but it
//!   ends a word, so `g/**/rect` is not `grect` (and is no selector).
//!
//! A selector with anything else -- an attribute (`[x]`), a pseudo-class
//! (`:hover`), a sibling combinator -- selects nothing, and its rule is kept
//! for the rest of its comma list. One CSS rejects -- `a >`, `.1x`, an empty
//! place in a list -- voids its whole rule, as CSS has it. At-rules
//! (`@media`, `@import`, `@font-face`) are skipped whole: a sheet's
//! conditions are not evaluated.
//!
//! # What it may cost
//!
//! A document is anyone's, and so is its sheet. At most [`MAX_RULES`] rules
//! are read, a selector of more than [`MAX_COMPOUNDS`] parts selects nothing,
//! each element is tried only against the rules whose rightmost part names
//! its id, one of its classes or its type (or names none), and all matching
//! together takes at most [`MAX_STEPS`] steps -- past which the rest of the
//! document is left unstyled rather than the drawing left unfinished.

use std::collections::HashMap;

use super::{XmlElement, local_tag, without_important};

/// The most rules a document's sheets are read for.
const MAX_RULES: usize = 4096;

/// The most parts -- compounds -- one selector may have.
const MAX_COMPOUNDS: usize = 8;

/// The most steps all of a document's matching may take.
const MAX_STEPS: usize = 20_000_000;

/// What a comment leaves in a sheet until the sheet is read: a character
/// that is no part of CSS. Not nothing, since CSS reads `a/**/b` as two words
/// side by side and not as `ab`; and not a space, since a space between the
/// parts of a selector means something -- `rect/**/.a` is `rect.a`, a rect
/// of class `a`, where `rect .a` is anything of class `a` inside a rect.
/// Declarations read it as a space ([`declarations`]), selectors as CSS
/// does ([`joined`]).
const COMMENT: char = '\u{1}';

/// One element's part of a selector: `path.st0#logo`, or `*`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Compound {
    /// The element's type; `None` for `*`, or where none is written.
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
}

/// How one part of a selector stands to the part on its left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Combinator {
    /// Inside it, at any depth: `a b`.
    Descendant,
    /// Directly inside it: `a > b`.
    Child,
}

/// A selector: its compounds rightmost first -- the one the element itself
/// must match -- and how each stands to the next one out.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Selector {
    compounds: Vec<Compound>,
    /// `combinators[i]` is how `compounds[i]` stands to `compounds[i + 1]`.
    combinators: Vec<Combinator>,
}

/// One rule, for one selector of its comma list.
#[derive(Clone, Debug)]
struct Rule {
    selector: Selector,
    /// CSS's: ids, then classes, then types.
    specificity: (u32, u32, u32),
    /// Where it stands in the sheets: a later rule beats an earlier one as
    /// specific.
    order: usize,
    /// Its declarations as written, but for the `!important` ones.
    normal: String,
    /// Its `!important` declarations.
    important: String,
}

/// What a selector reads of an element.
struct Facts {
    tag: String,
    id: Option<String>,
    classes: Vec<String>,
}

impl Facts {
    fn of(elem: &XmlElement) -> Self {
        Self {
            tag: local_tag(&elem.tag).to_owned(),
            id: elem
                .attr("id")
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_owned),
            classes: elem
                .attr("class")
                .map(|c| c.split_ascii_whitespace().map(str::to_owned).collect())
                .unwrap_or_default(),
        }
    }
}

/// The rules of a document's sheets, indexed by what their rightmost part
/// names.
#[derive(Debug, Default)]
pub(super) struct Sheet {
    rules: Vec<Rule>,
    by_id: HashMap<String, Vec<usize>>,
    by_class: HashMap<String, Vec<usize>>,
    by_tag: HashMap<String, Vec<usize>>,
    /// Rules whose rightmost part names no id, class or type.
    universal: Vec<usize>,
}

impl Sheet {
    /// The rules of every `<style>` sheet under `root` -- one whose `type` is
    /// not CSS's aside -- in document order.
    pub(super) fn of(root: &XmlElement) -> Self {
        let mut text = String::new();
        gather(root, &mut text);
        Self::parse(&text)
    }

    /// The rules `text`, a style sheet, holds.
    fn parse(text: &str) -> Self {
        let text = without_comments(text);
        let mut sheet = Self::default();
        let mut rest = text.as_str();
        let mut order = 0usize;
        loop {
            rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == COMMENT);
            if rest.is_empty() || sheet.rules.len() >= MAX_RULES {
                break;
            }
            if rest.starts_with('@') {
                rest = past_at_rule(rest);
                continue;
            }
            let Some(open) = rest.find('{') else {
                break;
            };
            let prelude = rest.get(..open).unwrap_or("");
            let after = rest.get(open.saturating_add(1)..).unwrap_or("");
            let close = after.find('}').unwrap_or(after.len());
            let block = after.get(..close).unwrap_or("");
            rest = after.get(close.saturating_add(1)..).unwrap_or("");
            // A rule any of whose selectors is no selector is void, whole,
            // as CSS has it; one this cannot read only selects nothing.
            let Ok(selectors) = prelude
                .split(',')
                .map(Selector::parse)
                .collect::<Result<Vec<_>, Invalid>>()
            else {
                continue;
            };
            let (normal, important) = declarations(block);
            for selector in selectors.into_iter().flatten() {
                if sheet.rules.len() >= MAX_RULES {
                    break;
                }
                let specificity = selector.specificity();
                sheet.add(Rule {
                    selector,
                    specificity,
                    order,
                    normal: normal.clone(),
                    important: important.clone(),
                });
                order = order.saturating_add(1);
            }
        }
        sheet
    }

    /// Keep `rule`, indexed by what its rightmost part names: its id, else
    /// its first class, else its type.
    fn add(&mut self, rule: Rule) {
        let at = self.rules.len();
        let key = rule.selector.compounds.first();
        if let Some(id) = key.and_then(|c| c.id.clone()) {
            self.by_id.entry(id).or_default().push(at);
        } else if let Some(class) = key.and_then(|c| c.classes.first().cloned()) {
            self.by_class.entry(class).or_default().push(at);
        } else if let Some(tag) = key.and_then(|c| c.tag.clone()) {
            self.by_tag.entry(tag).or_default().push(at);
        } else {
            self.universal.push(at);
        }
        self.rules.push(rule);
    }

    /// Merge the declarations of the rules each element under `root`
    /// matches into its `style`, in the order the cascade weighs them -- and
    /// take the comments out of every element's own `style`, which is CSS
    /// too.
    pub(super) fn apply(&self, root: &mut XmlElement) {
        if self.rules.is_empty() {
            uncomment_styles(root);
            return;
        }
        let mut steps = MAX_STEPS;
        self.apply_to(root, &mut Vec::new(), &mut steps);
    }

    fn apply_to(&self, elem: &mut XmlElement, ancestors: &mut Vec<Facts>, steps: &mut usize) {
        uncomment_style(elem);
        let facts = Facts::of(elem);
        let mut matched: Vec<&Rule> = self
            .candidates(&facts)
            .filter_map(|at| self.rules.get(at))
            .filter(|rule| matches(&rule.selector, &facts, ancestors, steps))
            .collect();
        if !matched.is_empty() {
            matched.sort_by_key(|rule| (rule.specificity, rule.order));
            // Weakest first, since what reads a property takes the last of
            // it (but an important one over any that is not): the rules,
            // the element's own declarations, the rules' important ones, and
            // its own important ones -- which CSS weighs above any rule's.
            let (own, own_important) = elem.attr("style").map(declarations).unwrap_or_default();
            let mut style = String::new();
            for rule in &matched {
                style.push_str(&rule.normal);
            }
            style.push_str(&own);
            for rule in &matched {
                style.push_str(&rule.important);
            }
            style.push_str(&own_important);
            set_style(elem, style);
        }
        ancestors.push(facts);
        for child in &mut elem.children {
            self.apply_to(child, ancestors, steps);
        }
        ancestors.pop();
    }

    /// The rules worth trying on an element: those its id, a class of its, or
    /// its type indexes, and those that name none of these.
    fn candidates<'s>(&'s self, facts: &'s Facts) -> impl Iterator<Item = usize> + 's {
        let named = |map: &'s HashMap<String, Vec<usize>>, key: &str| {
            map.get(key).map_or(&[][..], Vec::as_slice).iter().copied()
        };
        facts
            .id
            .iter()
            .flat_map(move |id| named(&self.by_id, id))
            .chain(
                facts
                    .classes
                    .iter()
                    .flat_map(move |class| named(&self.by_class, class)),
            )
            .chain(named(&self.by_tag, &facts.tag))
            .chain(self.universal.iter().copied())
    }
}

/// Every CSS sheet's text under `elem`, in document order.
fn gather(elem: &XmlElement, out: &mut String) {
    if local_tag(&elem.tag) == "style"
        && elem
            .attr("type")
            .is_none_or(|t| t.trim().is_empty() || t.trim() == "text/css")
    {
        out.push_str(&elem.text);
        out.push('\n');
    }
    for child in &elem.children {
        gather(child, out);
    }
}

/// `elem`'s `style` attribute set to `style`.
fn set_style(elem: &mut XmlElement, style: String) {
    match elem.attrs.iter_mut().find(|(name, _)| name == "style") {
        Some((_, value)) => *value = style,
        None => elem.attrs.push(("style".to_owned(), style)),
    }
}

/// `text` with each of its `/* ... */` comments left as one [`COMMENT`]; one
/// left open runs to the end.
fn without_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("/*") {
        out.push_str(rest.get(..open).unwrap_or(""));
        let after = rest.get(open.saturating_add(2)..).unwrap_or("");
        rest = after
            .find("*/")
            .and_then(|close| after.get(close.saturating_add(2)..))
            .unwrap_or("");
        out.push(COMMENT);
    }
    out.push_str(rest);
    out
}

/// `elem`'s own `style` without its comments, each read as a space -- as
/// the declarations of a sheet are.
fn uncomment_style(elem: &mut XmlElement) {
    if let Some(own) = elem.attr("style").filter(|style| style.contains("/*")) {
        let plain = without_comments(own).replace(COMMENT, " ");
        set_style(elem, plain);
    }
}

/// [`uncomment_style`] for `elem` and everything under it.
fn uncomment_styles(elem: &mut XmlElement) {
    uncomment_style(elem);
    for child in &mut elem.children {
        uncomment_styles(child);
    }
}

/// A selector as written, its comments taken out as CSS takes them: one
/// between two words leaves two words side by side, which no selector is
/// (`g/**/rect`, `.a/**/b`), and is `None`; one anywhere else joins what is
/// either side of it (`rect/**/.a` is `rect.a`).
fn joined(written: &str) -> Option<String> {
    let in_word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_');
    let mut out = String::with_capacity(written.len());
    let mut chars = written.chars().peekable();
    while let Some(c) = chars.next() {
        if c != COMMENT {
            out.push(c);
            continue;
        }
        while chars.next_if_eq(&COMMENT).is_some() {}
        if in_word(out.chars().next_back()) && in_word(chars.peek().copied()) {
            return None;
        }
    }
    Some(out)
}

/// What follows the at-rule `text` starts with: past its `;`, or past its
/// block and every block inside it.
fn past_at_rule(text: &str) -> &str {
    let semi = text.find(';');
    let open = text.find('{');
    match (semi, open) {
        (Some(s), Some(o)) if s < o => text.get(s.saturating_add(1)..).unwrap_or(""),
        (Some(s), None) => text.get(s.saturating_add(1)..).unwrap_or(""),
        (_, Some(o)) => {
            let mut depth = 0usize;
            for (at, ch) in text.char_indices().skip_while(|&(at, _)| at < o) {
                match ch {
                    '{' => depth = depth.saturating_add(1),
                    '}' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            return text.get(at.saturating_add(1)..).unwrap_or("");
                        }
                    }
                    _ => {}
                }
            }
            ""
        }
        (None, None) => "",
    }
}

/// A block's declarations, each ended with `;`, a comment in them read as a
/// space: those that are not `!important`, and those that are.
fn declarations(block: &str) -> (String, String) {
    let (mut normal, mut important) = (String::new(), String::new());
    let block = block.replace(COMMENT, " ");
    for declaration in block.split(';') {
        let declaration = declaration.trim();
        let Some((property, value)) = declaration.split_once(':') else {
            continue;
        };
        let (property, value) = (property.trim(), value.trim());
        if property.is_empty() || value.is_empty() {
            continue;
        }
        let out = if without_important(value).1 {
            &mut important
        } else {
            &mut normal
        };
        out.push_str(property);
        out.push(':');
        out.push_str(value);
        out.push(';');
    }
    (normal, important)
}

/// Whether `name` is a plain CSS identifier: letters, digits, `-` and `_`,
/// not starting with a digit or with `-` and a digit, and not `-` alone. An
/// escaped one is not read.
fn is_identifier(name: &str) -> bool {
    let digit_first = |s: &str| s.starts_with(|c: char| c.is_ascii_digit());
    !name.is_empty()
        && name != "-"
        && !digit_first(name)
        && !name.strip_prefix('-').is_some_and(digit_first)
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

/// A selector CSS rejects. A rule any selector in whose list is one is void
/// whole -- where one that is merely not read here, `:hover` say, selects
/// nothing and leaves the rest of its list standing.
#[derive(Debug, PartialEq, Eq)]
struct Invalid;

impl Compound {
    /// A compound selector as written; `Ok(None)` for one that selects
    /// nothing (two different ids), `Err` for one that is no selector.
    fn parse(written: &str) -> Result<Option<Self>, Invalid> {
        let mut compound = Self::default();
        let mut possible = true;
        let mut rest = written;
        if let Some(after) = rest.strip_prefix('*') {
            rest = after;
        } else {
            let end = rest.find(['.', '#']).unwrap_or(rest.len());
            let tag = rest.get(..end).ok_or(Invalid)?;
            if !tag.is_empty() {
                if !is_identifier(tag) {
                    return Err(Invalid);
                }
                compound.tag = Some(tag.to_owned());
            }
            rest = rest.get(end..).ok_or(Invalid)?;
        }
        while let Some(mark) = rest.chars().next() {
            let after = rest.get(1..).ok_or(Invalid)?;
            let end = after.find(['.', '#']).unwrap_or(after.len());
            let name = after.get(..end).ok_or(Invalid)?;
            if !is_identifier(name) {
                return Err(Invalid);
            }
            match mark {
                '.' => compound.classes.push(name.to_owned()),
                // An element has one id: a compound asking for two different
                // ones selects nothing.
                '#' => match &compound.id {
                    Some(id) if id != name => possible = false,
                    _ => compound.id = Some(name.to_owned()),
                },
                // `*rect` and the like: a type after something that is not.
                _ => return Err(Invalid),
            }
            rest = after.get(end..).ok_or(Invalid)?;
        }
        Ok(possible.then_some(compound))
    }

    /// Whether the element `facts` describes matches this part.
    fn matches(&self, facts: &Facts) -> bool {
        self.tag.as_ref().is_none_or(|tag| *tag == facts.tag)
            && self
                .id
                .as_ref()
                .is_none_or(|id| facts.id.as_ref() == Some(id))
            && self
                .classes
                .iter()
                .all(|class| facts.classes.contains(class))
    }
}

impl Selector {
    /// A selector as written: `Ok(None)` for one that selects nothing, or
    /// that CSS reads and this does not -- an attribute, a pseudo-class, a
    /// sibling combinator, a namespace, an escape, more than
    /// [`MAX_COMPOUNDS`] parts -- and `Err` for one CSS rejects: empty, a
    /// combinator with nothing on one side, a part that is no compound.
    fn parse(written: &str) -> Result<Option<Self>, Invalid> {
        let written = joined(written).ok_or(Invalid)?;
        let written = written.trim();
        if written.is_empty() {
            return Err(Invalid);
        }
        if written.contains(['[', ':', '+', '~', '\\', '|']) {
            return Ok(None);
        }
        let mut compounds = Vec::new();
        let mut combinators = Vec::new();
        let mut pending: Option<Combinator> = None;
        let mut possible = true;
        let spaced = written.replace('>', " > ");
        // Every part is read, even past one that selects nothing or past the
        // most parts read: a part further on that is no selector still voids
        // the rule.
        for token in spaced.split_ascii_whitespace() {
            if token == ">" {
                if compounds.is_empty() || pending.is_some() {
                    return Err(Invalid);
                }
                pending = Some(Combinator::Child);
                continue;
            }
            let compound = Compound::parse(token)?.unwrap_or_else(|| {
                possible = false;
                Compound::default()
            });
            let outward = pending.take();
            if compounds.len() >= MAX_COMPOUNDS {
                possible = false;
                continue;
            }
            if !compounds.is_empty() {
                combinators.push(outward.unwrap_or(Combinator::Descendant));
            }
            compounds.push(compound);
        }
        if pending.is_some() {
            return Err(Invalid);
        }
        compounds.reverse();
        combinators.reverse();
        Ok(possible.then_some(Self {
            compounds,
            combinators,
        }))
    }

    /// CSS's specificity: how many ids, classes and types it names.
    fn specificity(&self) -> (u32, u32, u32) {
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        self.compounds
            .iter()
            .fold((0, 0, 0), |(ids, classes, types), c| {
                (
                    ids.saturating_add(count(usize::from(c.id.is_some()))),
                    classes.saturating_add(count(c.classes.len())),
                    types.saturating_add(count(usize::from(c.tag.is_some()))),
                )
            })
    }
}

/// Whether `selector` selects the element `facts` describes, inside
/// `ancestors` (outermost first) -- spending `steps`, and selecting nothing
/// once they are spent.
///
/// Worked from the selector's outermost part inwards: for each part, on
/// which ancestors it can stand with every part further out standing where
/// its combinators put it. Each part's row is filled once, from the row
/// outside it, so a selector costs its parts times the depth -- where trying
/// each way through a chain of descendant combinators in turn would cost the
/// depth to the power of its parts.
fn matches(selector: &Selector, facts: &Facts, ancestors: &[Facts], steps: &mut usize) -> bool {
    let Some(own) = selector.compounds.first() else {
        return false;
    };
    if *steps == 0 || !own.matches(facts) {
        return false;
    }
    let Some(outermost) = selector.compounds.len().checked_sub(1) else {
        return false;
    };
    if outermost == 0 {
        return true;
    }
    // `row[n]`: the part being placed can stand on `ancestors[n]`, with the
    // parts further out placed. Starts as the outermost part's.
    let mut row: Vec<bool> = Vec::with_capacity(ancestors.len());
    for i in (1..=outermost).rev() {
        let Some(part) = selector.compounds.get(i) else {
            return false;
        };
        // How this part stands to the next one out, and where that one can
        // stand: on the ancestor just outside this one, or on any further out.
        let outward = selector.combinators.get(i).copied();
        let outer = std::mem::take(&mut row);
        let mut seen_outer = false;
        for (n, ancestor) in ancestors.iter().enumerate() {
            let Some(left) = steps.checked_sub(1) else {
                return false;
            };
            *steps = left;
            let placed_outside = match outward {
                None => true,
                Some(Combinator::Child) => n
                    .checked_sub(1)
                    .and_then(|just_out| outer.get(just_out))
                    .copied()
                    .unwrap_or(false),
                Some(Combinator::Descendant) => seen_outer,
            };
            row.push(placed_outside && part.matches(ancestor));
            // Whether the next part out can stand anywhere outside the next
            // ancestor in.
            seen_outer = seen_outer || outer.get(n).copied().unwrap_or(false);
        }
    }
    // The element is part 0: part 1 stands on its parent, or on any ancestor.
    match selector.combinators.first() {
        Some(Combinator::Child) => row.last().copied().unwrap_or(false),
        Some(Combinator::Descendant) => row.iter().any(|&fits| fits),
        None => true,
    }
}

#[cfg(test)]
#[path = "css_tests.rs"]
mod tests;
