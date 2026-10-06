//! Styles written as CSS: the subset of CSS that makes sense for a widget,
//! read into the toolkit's [`Style`](crate::style::Style).
//!
//! `design.txt` asks for "css styles applied to various things like with Qt,
//! but no annoying css overrides unlike Qt", and `roadmap-detailed.md` §3.5
//! (*Styling -- CSS Subset with Inheritance, No Cascade*) says which subset:
//! the properties a widget has -- colours, fonts, margins, padding, borders
//! and their radii, sizes, opacity, shadows -- with CSS's values for them,
//! its units, `var()` for the theme's colours and `calc()`; and none of the
//! cascade's machinery -- no specificity, no `!important`.
//!
//! Built in parts, each usable as it lands:
//!
//! - [`token`]: the tokens a style is written in (CSS Syntax Level 3's
//!   tokenizer, for this subset).
//! - [`value`]: the values a property is given -- lengths in CSS's units and
//!   their sums (`calc()`), numbers, colours in every notation CSS has.
//! - [`decl`]: the properties, what each may be given, and the shorthands
//!   read into the properties they set.
//! - [`compute`]: a widget's declarations, in order, computed where it is --
//!   its parent's font and colour inherited, the theme's colours as
//!   variables, its units measured -- over its program's style.
//! - [`sheet`]: what a program writes -- a widget's own block, with blocks
//!   for its states (`&:hover { ... }`), and style sheets of rules chosen by
//!   selectors (kind, `.class`, `#name`, states, `>`).
//!
//! The widget tree's side, transitions and positioning follow
//! (`design-decisions.md` §1478 says how each is to work).

pub mod compute;
pub mod decl;
pub mod sheet;
pub mod token;
pub mod value;
