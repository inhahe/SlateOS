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
//! - [`transition`]: a style moving from what it was to what it has become,
//!   over the time its `transition` gives -- at the user's motion speed, and
//!   not at all with animations off.
//!
//! The widget tree is where it is used: [`Widget::css`] gives a widget its
//! own block, [`Widget::class`] and [`Widget::named`] what selectors ask
//! for, and [`WidgetTree::set_style_sheet`] a tree its sheet. Every layout
//! computes each widget's style in its present state -- and a styled tree
//! lays itself out again when the pointer or a key changes a state -- over
//! the program's own style, which is kept apart ([`Widget::look`] is the
//! style it is drawn in). Percentages are settled as each container lays
//! its children out; a box's and a label's text shadows are drawn. A
//! `font-family` list draws in the first generic family of it (a family by
//! name waits on `FontFamily::Named`), pushed round
//! the widget's drawing and measured in while it is laid out, drawn and
//! handles its events ([`crate::text::in_family`]). A tree moving a
//! transition says so ([`WidgetTree::animating`]): its program sends it
//! ticks until it is done. `position` takes a widget out of its parent's
//! flow -- `relative` moved from its place, `absolute` placed in its
//! parent's padding box, `fixed` in the window and over everything -- and
//! `z-index` orders siblings, drawn and hit (`design-decisions.md` §1478).
//!
//! [`Widget::css`]: crate::widget::Widget::css
//! [`Widget::class`]: crate::widget::Widget::class
//! [`Widget::named`]: crate::widget::Widget::named
//! [`Widget::look`]: crate::widget::Widget::look
//! [`WidgetTree::set_style_sheet`]: crate::widget::WidgetTree::set_style_sheet
//! [`WidgetTree::animating`]: crate::widget::WidgetTree::animating

pub mod compute;
pub mod decl;
pub mod sheet;
pub mod token;
pub mod transition;
pub mod value;
