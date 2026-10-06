//! Computing a widget's style: its declarations, read where it is -- its
//! parent's font and colour, the theme's colours as variables, the window --
//! over its program's [`Style`].
//!
//! # In order, and inherited only from styles
//!
//! Declarations apply in the order given -- a style sheet's rules in the
//! order written, then the widget's own -- and a later one wins: there is no
//! specificity and no `!important` (`design-decisions.md` §1478).
//!
//! A widget takes its parent's text colour, font, line height, alignment,
//! text shadow and pointer when its own declarations do not set them -- CSS's
//! inherited properties -- but only values a style set: a widget whose
//! parent was styled in code alone keeps its own, so a tree with no CSS draws
//! exactly as it did.
//!
//! # Variables
//!
//! `var(--name)` is the custom property `--name` in force -- declared on the
//! widget or inherited from its parents -- or else one of the theme's: the
//! palette's colours by their role (`--base`, `--surface0`, `--text`,
//! `--accent` ...), and `--font-size`, the base text size. A `var()` with
//! nothing to stand for takes its fallback, if it has one; else the
//! declaration is dropped with a warning.
//!
//! # Percentages
//!
//! A percentage of a box's size, margin or padding is of its container's
//! width (or height, for a height), which is known only where the container
//! lays it out: such lengths are kept in [`BoxLengths`] and settled by
//! [`BoxLengths::apply`] then.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::decl::{
    self, BorderStyle, Corner, Declared, Family, LineHeight, Property, ShadowValue, Side, Value,
};
use super::token::{Spanned, Token};
use super::transition::TransitionSpec;
use super::value::{ColorValue, Length, Units};
use crate::color::Color;
use crate::palette::Palette;
use crate::style::{BoxShadow, Cursor, FontWeight, Style, TextAlign};

/// How deep `var()`s may stand for others: past this a chain is a cycle.
const MAX_VAR_DEPTH: usize = 16;

/// What the tree knows that a style's units and variables are read against.
pub struct Env<'a> {
    /// The base font size: `rem`, `medium` and the other size keywords.
    pub root_font_size: f32,
    /// The window, for `vw` and `vh`.
    pub viewport: (f32, f32),
    /// Pixels to a millimetre.
    pub px_per_mm: f32,
    /// The theme, whose colours are variables.
    pub palette: &'a Palette,
    /// How wide a `0` is: what `ch` is.
    pub zero_width: &'a ZeroWidth<'a>,
    /// The tree's clock, in milliseconds, when the style is computed: when
    /// a transition its change starts begins ([`super::transition`]).
    pub now_ms: f64,
}

/// How wide a `0` is at a size and weight, in a style's `font-family` list
/// (`None` where no style gave one) -- [`Env::zero_width`].
pub type ZeroWidth<'a> = dyn Fn(f32, FontWeight, Option<&[Family]>) -> f32 + 'a;

/// What a widget's style leaves its children: the inherited properties a
/// style set, and the custom properties in force.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inherited {
    /// The text's colour.
    pub color: Option<Color>,
    /// The font size, in pixels.
    pub font_size: Option<f32>,
    /// The weight, 1 to 1000.
    pub font_weight: Option<u16>,
    /// The families, in order.
    pub font_family: Option<Vec<Family>>,
    /// The line height: a multiple of the font size, or pixels.
    pub line_height: Option<InheritedLine>,
    /// The text's alignment.
    pub text_align: Option<TextAlign>,
    /// The text's shadow: `Some(None)` for `none`.
    pub text_shadow: Option<Option<BoxShadow>>,
    /// The pointer's shape.
    pub cursor: Option<Cursor>,
    /// The custom properties in force, by name, as written.
    pub vars: Vars,
}

/// Custom properties by name, as written: shared from parent to child until
/// one declares its own.
pub type Vars = Arc<BTreeMap<String, Vec<Spanned>>>;

/// A line height as children inherit it: a multiple stays one, so each child
/// takes it of its own font size; a length is the parent's pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InheritedLine {
    /// Times the font size.
    Multiple(f32),
    /// Pixels.
    Px(f32),
}

/// A box's lengths as a style gave them, where any is a percentage of its
/// container: kept for the container to settle when it lays the box out.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoxLengths {
    /// `width`: `Some(None)` is `auto`.
    pub width: Option<Option<Length>>,
    /// `height`: `Some(None)` is `auto`.
    pub height: Option<Option<Length>>,
    /// `min-width`.
    pub min_width: Option<Length>,
    /// `max-width`: `Some(None)` is `none`.
    pub max_width: Option<Option<Length>>,
    /// `min-height`.
    pub min_height: Option<Length>,
    /// `max-height`: `Some(None)` is `none`.
    pub max_height: Option<Option<Length>>,
    /// Each side's margin, top first.
    pub margin: [Option<Length>; 4],
    /// Each side's padding, top first.
    pub padding: [Option<Length>; 4],
    /// What the units were worth where the style was computed.
    pub units: Option<Units>,
}

impl BoxLengths {
    /// Whether any length waits on its container.
    #[must_use]
    pub fn waits(&self) -> bool {
        let has = |l: &Option<Length>| l.is_some_and(|l| l.has_percent());
        let has_opt =
            |l: &Option<Option<Length>>| l.is_some_and(|l| l.is_some_and(|l| l.has_percent()));
        has_opt(&self.width)
            || has_opt(&self.height)
            || has(&self.min_width)
            || has_opt(&self.max_width)
            || has(&self.min_height)
            || has_opt(&self.max_height)
            || self.margin.iter().any(has)
            || self.padding.iter().any(has)
    }

    /// Settle them into `style` with the container's content `width` and
    /// `height`. Where one is not known (infinite) -- a container measuring
    /// what its content needs -- a size that is a percentage of it is
    /// `auto`, and a margin or padding that is one is nought, as CSS takes a
    /// percentage of an indefinite size.
    pub fn apply(&self, style: &mut Style, width: f32, height: f32) {
        let Some(units) = self.units else {
            return;
        };
        // A size: `None` where it is a percentage of what is not known, and
        // never below nought -- a `calc()` may come to less.
        let size = |l: Length, of: f32| -> Option<f32> {
            if of.is_finite() {
                Some(l.resolve(&units, of).max(0.0))
            } else if l.has_percent() {
                None
            } else {
                Some(l.resolve(&units, 0.0).max(0.0))
            }
        };
        if let Some(v) = self.width {
            style.width = v.and_then(|l| size(l, width));
        }
        if let Some(v) = self.height {
            style.height = v.and_then(|l| size(l, height));
        }
        if let Some(l) = self.min_width {
            style.min_width = size(l, width);
        }
        if let Some(v) = self.max_width {
            style.max_width = v.and_then(|l| size(l, width));
        }
        if let Some(l) = self.min_height {
            style.min_height = size(l, height);
        }
        if let Some(v) = self.max_height {
            style.max_height = v.and_then(|l| size(l, height));
        }
        // Margins and padding, top and bottom included, are of the width:
        // CSS's rule, so a box's sides do not depend on how tall it is.
        let across = if width.is_finite() { width } else { 0.0 };
        // A margin may be negative; padding may not.
        let sides = |edges: &mut crate::style::Edges, lengths: &[Option<Length>; 4], least: f32| {
            for (side, l) in Side::ALL.iter().zip(lengths) {
                if let Some(l) = l {
                    let v = l.resolve(&units, across).max(least);
                    match side {
                        Side::Top => edges.top = v,
                        Side::Right => edges.right = v,
                        Side::Bottom => edges.bottom = v,
                        Side::Left => edges.left = v,
                    }
                }
            }
        };
        sides(&mut style.margin, &self.margin, f32::NEG_INFINITY);
        sides(&mut style.padding, &self.padding, 0.0);
    }
}

/// A widget's style, computed.
#[derive(Clone, Debug, PartialEq)]
pub struct Computed {
    /// The style it is drawn and laid out in.
    pub style: Style,
    /// The families it asks its text be drawn in, in order: `None` is its
    /// program's.
    pub font_family: Option<Vec<Family>>,
    /// Its lengths that wait on its container.
    pub lengths: BoxLengths,
    /// What it leaves its children.
    pub inherited: Inherited,
    /// What of it moves when it changes, and how: its `transition`.
    pub transition: TransitionSpec,
    /// What in its declarations was not used, and why.
    pub warnings: Vec<String>,
}

/// The variable `--name` from the theme: a palette colour, or the base
/// font size.
fn theme_var(name: &str, env: &Env<'_>) -> Option<Vec<Spanned>> {
    let p = env.palette;
    let color = match name {
        "--crust" => p.crust,
        "--mantle" => p.mantle,
        "--base" => p.base,
        "--surface0" => p.surface0,
        "--surface1" => p.surface1,
        "--surface2" => p.surface2,
        "--overlay0" => p.overlay0,
        "--subtext0" => p.subtext0,
        "--subtext1" => p.subtext1,
        "--text" => p.text,
        "--link" => p.link,
        "--border" => p.border,
        "--accent" => p.accent,
        "--on-accent" => p.ink(p.accent),
        "--blue" => p.blue,
        "--green" => p.green,
        "--red" => p.red,
        "--yellow" => p.yellow,
        "--peach" => p.peach,
        "--lavender" => p.lavender,
        "--mauve" => p.mauve,
        "--sapphire" => p.sapphire,
        "--teal" => p.teal,
        "--sky" => p.sky,
        "--pink" => p.pink,
        "--rosewater" => p.rosewater,
        "--flamingo" => p.flamingo,
        "--maroon" => p.maroon,
        "--font-size" => {
            return Some(vec![Spanned {
                token: Token::Dimension(f64::from(env.root_font_size), "px".into()),
                at: 0,
            }]);
        }
        _ => return None,
    };
    Some(vec![Spanned {
        token: Token::Hash(format!(
            "{:02x}{:02x}{:02x}{:02x}",
            color.r, color.g, color.b, color.a
        )),
        at: 0,
    }])
}

/// `tokens` with every `var()` replaced by what it stands for.
fn substitute(
    tokens: &[Spanned],
    vars: &BTreeMap<String, Vec<Spanned>>,
    env: &Env<'_>,
    depth: usize,
) -> Result<Vec<Spanned>, String> {
    if depth > MAX_VAR_DEPTH {
        return Err("variables stand for each other in a circle".to_string());
    }
    let mut out = Vec::with_capacity(tokens.len());
    let mut i = 0usize;
    while let Some(s) = tokens.get(i) {
        i = i.saturating_add(1);
        let Token::Function(f) = &s.token else {
            out.push(s.clone());
            continue;
        };
        if !f.eq_ignore_ascii_case("var") {
            out.push(s.clone());
            continue;
        }
        // The arguments: up to the matching `)`.
        let start = i;
        let mut level = 0usize;
        let mut end = None;
        while let Some(t) = tokens.get(i) {
            i = i.saturating_add(1);
            match t.token {
                Token::Function(_) | Token::OpenParen => level = level.saturating_add(1),
                Token::CloseParen if level == 0 => {
                    end = Some(i.saturating_sub(1));
                    break;
                }
                Token::CloseParen => level = level.saturating_sub(1),
                _ => {}
            }
        }
        let end = end.ok_or("a `var(` is never closed")?;
        let args = tokens.get(start..end).unwrap_or(&[]);
        let mut parts = args.splitn(2, |t| t.token == Token::Comma);
        let name = parts
            .next()
            .and_then(|n| {
                n.iter()
                    .find(|t| t.token != Token::Whitespace)
                    .and_then(|t| match &t.token {
                        Token::Ident(name) if name.starts_with("--") => Some(name.clone()),
                        _ => None,
                    })
            })
            .ok_or("`var()` names no custom property")?;
        let fallback = parts.next();
        let value = vars
            .get(&name)
            .cloned()
            .or_else(|| theme_var(&name, env))
            .or_else(|| fallback.map(<[Spanned]>::to_vec))
            .ok_or_else(|| format!("`{name}` stands for nothing here"))?;
        out.extend(substitute(&value, vars, env, depth.saturating_add(1))?);
    }
    Ok(out)
}

/// A font weight's toolkit rung: what the renderer draws.
fn weight_rung(weight: u16) -> FontWeight {
    match weight {
        0..=150 => FontWeight::Thin,
        151..=350 => FontWeight::Light,
        351..=450 => FontWeight::Regular,
        451..=550 => FontWeight::Medium,
        551..=650 => FontWeight::SemiBold,
        651..=750 => FontWeight::Bold,
        _ => FontWeight::ExtraBold,
    }
}

/// A rung's weight.
const fn rung_weight(w: FontWeight) -> u16 {
    match w {
        FontWeight::Thin => 100,
        FontWeight::Light => 300,
        FontWeight::Regular => 400,
        FontWeight::Medium => 500,
        FontWeight::SemiBold => 600,
        FontWeight::Bold => 700,
        FontWeight::ExtraBold => 800,
    }
}

/// CSS Fonts 4's `bolder` and `lighter`, from the parent's weight.
const fn relative_weight(parent: u16, bolder: bool) -> u16 {
    if bolder {
        if parent < 350 {
            400
        } else if parent < 550 {
            700
        } else {
            900
        }
    } else if parent < 550 {
        100
    } else if parent < 750 {
        400
    } else {
        700
    }
}

/// `style` with the declarations `declared`, in order, computed where the
/// widget is: under a parent styled `parent_style` that left it `parent`
/// and moves as `parent_transition` says -- what `transition: inherit`
/// takes -- in `env`.
#[must_use]
pub fn compute(
    base: &Style,
    declared: &[&Declared],
    parent_style: &Style,
    parent: &Inherited,
    parent_transition: &TransitionSpec,
    env: &Env<'_>,
) -> Computed {
    let Gathered {
        set,
        vars,
        warnings,
    } = gather(declared, parent, env);
    let mut c = Computing {
        set: &set,
        parent_style,
        parent,
        env,
        style: base.clone(),
        inherited: Inherited {
            vars,
            ..Inherited::default()
        },
    };
    // The font first -- its size, weight and family: every `em` but the
    // size's own is of the size, and a `ch` is a `0` in all three.
    c.font_size();
    c.font_weight();
    let font_family = c.font_family();
    let units = Units {
        em: c.style.font_size,
        rem: env.root_font_size,
        ch: (env.zero_width)(
            c.style.font_size,
            c.style.font_weight,
            font_family.as_deref(),
        ),
        viewport: env.viewport,
        px_per_mm: env.px_per_mm,
    };
    // The text's colour next: it is what `currentcolor` is.
    c.color();
    c.line_height(&units);
    c.text_align();
    c.cursor();
    c.text_shadow(&units);
    c.uninherited(&units);
    c.border_styles();
    let lengths = c.box_lengths(units);
    let transition = c.transition(parent_transition);
    Computed {
        style: c.style,
        font_family,
        lengths,
        inherited: c.inherited,
        transition,
        warnings,
    }
}

/// The properties `declared` sets -- the last one for each winning -- with
/// the custom properties in force and what could not be read.
fn gather(declared: &[&Declared], parent: &Inherited, env: &Env<'_>) -> Gathered {
    let mut warnings = Vec::new();
    // The custom properties first: a declaration may name one declared after
    // it, as in CSS.
    let mut vars = parent.vars.clone();
    for d in declared {
        if let Declared::Custom { name, tokens } = d {
            Arc::make_mut(&mut vars).insert(name.clone(), tokens.clone());
        }
    }
    let mut set: BTreeMap<Property, Value> = BTreeMap::new();
    for d in declared {
        match d {
            Declared::Value(p, v) => {
                set.insert(*p, v.clone());
            }
            Declared::Pending { name, tokens } => {
                match substitute(tokens, &vars, env, 0).and_then(|t| decl::read(name, &t)) {
                    Ok(all) => set.extend(all),
                    Err(why) => warnings.push(format!("`{name}`: {why}")),
                }
            }
            Declared::Custom { .. } => {}
        }
    }
    Gathered {
        set,
        vars,
        warnings,
    }
}

/// What [`gather`] finds.
struct Gathered {
    /// Every property set, the last one winning.
    set: BTreeMap<Property, Value>,
    /// The custom properties in force.
    vars: Vars,
    /// What could not be read.
    warnings: Vec<String>,
}

/// A style being computed: what was set, where, and what it has come to.
struct Computing<'s, 'e> {
    set: &'s BTreeMap<Property, Value>,
    parent_style: &'s Style,
    parent: &'s Inherited,
    env: &'s Env<'e>,
    style: Style,
    inherited: Inherited,
}

impl<'s> Computing<'s, '_> {
    /// What `p` was set to: borrowed from the declarations, not from the
    /// style being computed, so it can be read while that is written.
    fn get(&self, p: Property) -> Option<&'s Value> {
        self.set.get(&p)
    }

    fn font_size(&mut self) {
        let parent_size = self.parent_style.font_size;
        let units = Units {
            em: parent_size,
            rem: self.env.root_font_size,
            ch: (self.env.zero_width)(
                parent_size,
                self.parent_style.font_weight,
                self.parent.font_family.as_deref(),
            ),
            viewport: self.env.viewport,
            px_per_mm: self.env.px_per_mm,
        };
        match self.get(Property::FontSize) {
            Some(Value::Length(l)) => {
                self.style.font_size = l.resolve(&units, parent_size).max(0.0);
                self.inherited.font_size = Some(self.style.font_size);
            }
            Some(Value::Inherit) => {
                self.style.font_size = parent_size;
                self.inherited.font_size = Some(parent_size);
            }
            Some(Value::Initial) => self.style.font_size = Style::default().font_size,
            _ => {
                if let Some(size) = self.parent.font_size {
                    self.style.font_size = size;
                    self.inherited.font_size = Some(size);
                }
            }
        }
    }

    fn font_weight(&mut self) {
        let from = self
            .parent
            .font_weight
            .unwrap_or_else(|| rung_weight(self.parent_style.font_weight));
        let weight = match self.get(Property::FontWeight) {
            Some(Value::FontWeight(w)) => Some(*w),
            Some(Value::Bolder) => Some(relative_weight(from, true)),
            Some(Value::Lighter) => Some(relative_weight(from, false)),
            Some(Value::Inherit) => Some(from),
            Some(Value::Initial) => {
                self.style.font_weight = Style::default().font_weight;
                None
            }
            _ => self.parent.font_weight,
        };
        if let Some(w) = weight {
            self.style.font_weight = weight_rung(w);
            self.inherited.font_weight = Some(w);
        }
    }

    fn color(&mut self) {
        match self.get(Property::Color) {
            Some(Value::Color(ColorValue::Color(c))) => {
                let c = *c;
                self.style.foreground = Some(c);
                self.inherited.color = Some(c);
            }
            // `color: currentcolor` is the colour it would inherit.
            Some(Value::Color(ColorValue::CurrentColor) | Value::Inherit) => {
                self.style.foreground = self.parent.color.or(self.parent_style.foreground);
                self.inherited.color = self.style.foreground;
            }
            Some(Value::Initial) => self.style.foreground = None,
            _ => {
                if let Some(c) = self.parent.color {
                    self.style.foreground = Some(c);
                    self.inherited.color = Some(c);
                }
            }
        }
    }

    /// The text's colour: what `currentcolor` stands for.
    fn current(&self) -> Color {
        self.style.foreground.unwrap_or(self.env.palette.text)
    }

    fn color_of(&self, v: &ColorValue) -> Color {
        match v {
            ColorValue::Color(c) => *c,
            ColorValue::CurrentColor => self.current(),
        }
    }

    /// Its `transition-*` lists: what each says, `inherit` the parent's, and
    /// what none says -- or `initial` -- the initial one's.
    fn transition(&self, parent: &TransitionSpec) -> TransitionSpec {
        let initial = TransitionSpec::default();
        TransitionSpec {
            properties: match self.get(Property::TransitionProperty) {
                Some(Value::Transitions(t)) => t.clone(),
                Some(Value::Inherit) => parent.properties.clone(),
                _ => initial.properties,
            },
            durations: match self.get(Property::TransitionDuration) {
                Some(Value::Times(t)) => t.clone(),
                Some(Value::Inherit) => parent.durations.clone(),
                _ => initial.durations,
            },
            timings: match self.get(Property::TransitionTimingFunction) {
                Some(Value::Timings(t)) => t.clone(),
                Some(Value::Inherit) => parent.timings.clone(),
                _ => initial.timings,
            },
            delays: match self.get(Property::TransitionDelay) {
                Some(Value::Times(t)) => t.clone(),
                Some(Value::Inherit) => parent.delays.clone(),
                _ => initial.delays,
            },
        }
    }

    fn font_family(&mut self) -> Option<Vec<Family>> {
        let family = match self.get(Property::FontFamily) {
            Some(Value::FontFamily(f)) => Some(f.clone()),
            Some(Value::Initial) => None,
            _ => self.parent.font_family.clone(),
        };
        self.inherited.font_family.clone_from(&family);
        family
    }

    fn line_height(&mut self, units: &Units) {
        let size = self.style.font_size.max(f32::MIN_POSITIVE);
        let line = match self.get(Property::LineHeight) {
            Some(Value::LineHeight(LineHeight::Normal)) => {
                Some(InheritedLine::Multiple(Style::default().line_height))
            }
            Some(Value::LineHeight(LineHeight::Multiple(m))) => Some(InheritedLine::Multiple(*m)),
            Some(Value::LineHeight(LineHeight::Length(l))) => {
                Some(InheritedLine::Px(l.resolve(units, size)))
            }
            Some(Value::Inherit) => self
                .parent
                .line_height
                .or(Some(InheritedLine::Multiple(self.parent_style.line_height))),
            Some(Value::Initial) => {
                self.style.line_height = Style::default().line_height;
                None
            }
            _ => self.parent.line_height,
        };
        if let Some(line) = line {
            self.style.line_height = match line {
                InheritedLine::Multiple(m) => m,
                InheritedLine::Px(px) => px / size,
            };
            self.inherited.line_height = Some(line);
        }
    }

    fn text_align(&mut self) {
        match self.get(Property::TextAlign) {
            Some(Value::TextAlign(a)) => {
                let a = *a;
                self.style.text_align = a;
                self.inherited.text_align = Some(a);
            }
            Some(Value::Inherit) => {
                let a = self
                    .parent
                    .text_align
                    .unwrap_or(self.parent_style.text_align);
                self.style.text_align = a;
                self.inherited.text_align = Some(a);
            }
            Some(Value::Initial) => self.style.text_align = TextAlign::default(),
            _ => {
                if let Some(a) = self.parent.text_align {
                    self.style.text_align = a;
                    self.inherited.text_align = Some(a);
                }
            }
        }
    }

    fn cursor(&mut self) {
        match self.get(Property::Cursor) {
            Some(Value::Cursor(c)) => {
                let c = *c;
                self.style.cursor = c;
                self.inherited.cursor = Some(c);
            }
            Some(Value::Inherit) => {
                let c = self.parent.cursor.unwrap_or(self.parent_style.cursor);
                self.style.cursor = c;
                self.inherited.cursor = Some(c);
            }
            Some(Value::Initial) => self.style.cursor = Cursor::default(),
            _ => {
                if let Some(c) = self.parent.cursor {
                    self.style.cursor = c;
                    self.inherited.cursor = Some(c);
                }
            }
        }
    }

    fn shadow_of(&self, s: &ShadowValue, units: &Units) -> BoxShadow {
        BoxShadow {
            offset_x: s.x.resolve(units, 0.0),
            offset_y: s.y.resolve(units, 0.0),
            blur: s.blur.resolve(units, 0.0).max(0.0),
            spread: s.spread.resolve(units, 0.0),
            color: self.color_of(&s.color),
        }
    }

    fn text_shadow(&mut self, units: &Units) {
        let shadow = match self.get(Property::TextShadow) {
            Some(Value::Shadow(s)) => Some(Some(self.shadow_of(s, units))),
            Some(Value::None | Value::Initial) => Some(None),
            Some(Value::Inherit) => Some(
                self.parent
                    .text_shadow
                    .unwrap_or(self.parent_style.text_shadow),
            ),
            _ => self.parent.text_shadow,
        };
        if let Some(shadow) = shadow {
            self.style.text_shadow = shadow;
            self.inherited.text_shadow = Some(shadow);
        }
    }

    /// The properties not inherited: background, opacity, the box's shadow,
    /// the borders' colours and widths, the corners.
    fn uninherited(&mut self, units: &Units) {
        let initial = Style::default();
        let current = self.current();
        for (property, value) in self.set {
            match (*property, value) {
                (Property::BackgroundColor, Value::Color(c)) => {
                    self.style.background = self.color_of(c);
                }
                (Property::BackgroundColor, Value::Inherit) => {
                    self.style.background = self.parent_style.background;
                }
                (Property::BackgroundColor, Value::Initial) => {
                    self.style.background = initial.background;
                }
                (Property::Opacity, Value::Number(n)) => self.style.opacity = *n,
                (Property::Opacity, Value::Inherit) => {
                    self.style.opacity = self.parent_style.opacity;
                }
                (Property::Opacity, Value::Initial) => self.style.opacity = initial.opacity,
                (Property::BoxShadow, Value::Shadow(s)) => {
                    self.style.shadow = Some(self.shadow_of(s, units));
                }
                (Property::BoxShadow, Value::None | Value::Initial) => self.style.shadow = None,
                (Property::BoxShadow, Value::Inherit) => {
                    self.style.shadow = self.parent_style.shadow;
                }
                (Property::BorderColor(side), Value::Color(c)) => {
                    let c = self.color_of(c);
                    border_mut(&mut self.style, side).color = c;
                }
                (Property::BorderColor(side), Value::Inherit) => {
                    border_mut(&mut self.style, side).color =
                        border_of(self.parent_style, side).color;
                }
                (Property::BorderColor(side), Value::Initial) => {
                    border_mut(&mut self.style, side).color = current;
                }
                (Property::BorderWidth(side), Value::Length(l)) => {
                    border_mut(&mut self.style, side).width = l.resolve(units, 0.0).max(0.0);
                }
                (Property::BorderWidth(side), Value::Inherit) => {
                    border_mut(&mut self.style, side).width =
                        border_of(self.parent_style, side).width;
                }
                (Property::BorderWidth(side), Value::Initial) => {
                    // CSS's initial `medium`; with the initial style (none)
                    // it draws nothing.
                    border_mut(&mut self.style, side).width = 3.0;
                }
                (Property::BorderRadius(corner), Value::Length(l)) => {
                    // A percentage is of the box itself, which the radius
                    // does not move: taken of nought here.
                    *radius_mut(&mut self.style, corner) = l.resolve(units, 0.0).max(0.0);
                }
                (Property::BorderRadius(corner), Value::Inherit) => {
                    *radius_mut(&mut self.style, corner) = radius_of(self.parent_style, corner);
                }
                (Property::BorderRadius(corner), Value::Initial) => {
                    *radius_mut(&mut self.style, corner) = 0.0;
                }
                _ => {}
            }
        }
    }

    /// A side whose style is none has no border, whatever its width: CSS's
    /// rule, so a style sheet's `border: 2px red` (no style) draws nothing.
    fn border_styles(&mut self) {
        for side in Side::ALL {
            if let Some(Value::BorderStyle(BorderStyle::None) | Value::Initial) =
                self.get(Property::BorderStyle(side))
            {
                border_mut(&mut self.style, side).width = 0.0;
            }
        }
    }

    /// The box's lengths: settled now where nothing waits on the container,
    /// kept for it where something does.
    fn box_lengths(&mut self, units: Units) -> BoxLengths {
        let initial = Style::default();
        let parent = self.parent_style;
        let mut lengths = BoxLengths {
            units: Some(units),
            ..BoxLengths::default()
        };
        for (property, value) in self.set {
            // `inherit` is the parent's, in pixels; `initial` the toolkit's.
            let own = |inherit: Option<f32>, initial: Option<f32>| match value {
                Value::Length(l) => Some(*l),
                Value::Inherit => inherit.map(Length::px),
                Value::Initial => initial.map(Length::px),
                _ => None,
            };
            let limit = |inherit: Option<f32>, initial: Option<f32>| match value {
                Value::Auto | Value::None => Some(None),
                _ => Some(own(inherit, initial)),
            };
            match *property {
                Property::Width => lengths.width = limit(parent.width, initial.width),
                Property::Height => lengths.height = limit(parent.height, initial.height),
                Property::MaxWidth => {
                    lengths.max_width = limit(parent.max_width, initial.max_width);
                }
                Property::MaxHeight => {
                    lengths.max_height = limit(parent.max_height, initial.max_height);
                }
                Property::MinWidth => {
                    lengths.min_width =
                        own(parent.min_width, initial.min_width).or(Some(Length::default()));
                }
                Property::MinHeight => {
                    lengths.min_height =
                        own(parent.min_height, initial.min_height).or(Some(Length::default()));
                }
                Property::Margin(side) => {
                    let edge = side_of(&parent.margin, side);
                    if let Some(slot) = lengths.margin.get_mut(side_index(side)) {
                        *slot = Some(own(Some(edge), Some(0.0)).unwrap_or_default());
                    }
                }
                Property::Padding(side) => {
                    let edge = side_of(&parent.padding, side);
                    if let Some(slot) = lengths.padding.get_mut(side_index(side)) {
                        *slot = Some(own(Some(edge), Some(0.0)).unwrap_or_default());
                    }
                }
                _ => {}
            }
        }
        // Settled against a container not yet known: what has no percentage
        // is final, and what has one is settled again where the widget is
        // laid out.
        lengths.apply(&mut self.style, f32::INFINITY, f32::INFINITY);
        lengths
    }
}

const fn side_of(edges: &crate::style::Edges, side: Side) -> f32 {
    match side {
        Side::Top => edges.top,
        Side::Right => edges.right,
        Side::Bottom => edges.bottom,
        Side::Left => edges.left,
    }
}

const fn side_index(side: Side) -> usize {
    match side {
        Side::Top => 0,
        Side::Right => 1,
        Side::Bottom => 2,
        Side::Left => 3,
    }
}

fn border_mut(style: &mut Style, side: Side) -> &mut crate::style::Border {
    match side {
        Side::Top => &mut style.border.top,
        Side::Right => &mut style.border.right,
        Side::Bottom => &mut style.border.bottom,
        Side::Left => &mut style.border.left,
    }
}

const fn border_of(style: &Style, side: Side) -> crate::style::Border {
    match side {
        Side::Top => style.border.top,
        Side::Right => style.border.right,
        Side::Bottom => style.border.bottom,
        Side::Left => style.border.left,
    }
}

fn radius_mut(style: &mut Style, corner: Corner) -> &mut f32 {
    match corner {
        Corner::TopLeft => &mut style.border_radius.top_left,
        Corner::TopRight => &mut style.border_radius.top_right,
        Corner::BottomRight => &mut style.border_radius.bottom_right,
        Corner::BottomLeft => &mut style.border_radius.bottom_left,
    }
}

const fn radius_of(style: &Style, corner: Corner) -> f32 {
    match corner {
        Corner::TopLeft => style.border_radius.top_left,
        Corner::TopRight => style.border_radius.top_right,
        Corner::BottomRight => style.border_radius.bottom_right,
        Corner::BottomLeft => style.border_radius.bottom_left,
    }
}

#[cfg(test)]
#[path = "compute_tests.rs"]
mod tests;
