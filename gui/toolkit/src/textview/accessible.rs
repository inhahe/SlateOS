//! The text views as tools see them ([`Accessible`]).
//!
//! # A plain view
//!
//! A [`SimpleTextView`] -- a log, a program's output, a file's first lines
//! -- is a document: text to read, not to type into. Its node holds the
//! text, every line the view keeps. A tool scrolls it as the wheel does, by
//! whole lines: to the line the height it asks for falls on, as far as the
//! text goes.
//!
//! # A rich view
//!
//! A [`RichTextView`] is a document too, holding its text as copying all of
//! it gives it ([`RichTextView::plain_text`]), and under it each block as
//! what it is -- a heading, a paragraph, an item of a list, a block of code,
//! a picture by the words that stand for it, a rule -- with its box where it
//! is drawn, not shown while it is scrolled out of sight. Under its block,
//! each link is a link: named by its words, holding where it goes -- its
//! value, as a link's is to every accessibility interface. A run of spans
//! linking to one place, side by side, is one link, as it reads as one.
//!
//! A link pressed is the view's own click on it -- scrolled into sight
//! first, as its user would scroll to it -- and answers what the click does,
//! [`RichTextEvent::LinkClicked`], for its host to follow. A tool scrolls
//! the view as the wheel does, to the height asked for, as far as the text
//! goes.
//!
//! # What neither has
//!
//! A name: neither keeps one, so each is named for what it is -- "Text",
//! "Document" -- and a host that labels it renames the node. Nor the
//! keyboard: which of its host's parts has that is the host's to say, so
//! neither says it could take it.

use std::borrow::Cow;

use super::{RichBlock, RichSpan, RichTextEvent, RichTextView, SimpleTextView, WrappedLine};
use crate::event::{Event, MouseButton, MouseEvent, MouseEventKind};
use crate::frame::Rect;
use crate::widget::automation::{Accessible, Action, Node, Refusal, Role, Value};

/// A part of a plain text view, as tools name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextViewPart {
    /// The view: there is nothing else to it.
    View,
}

/// A part of a rich text view, as tools name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RichTextPart {
    /// The view.
    View,
    /// A block, by its place among the view's blocks.
    Block(usize),
    /// A link: the block it is in, and the first of the block's spans it is
    /// made of.
    Link {
        /// The block, by its place.
        block: usize,
        /// Its first span, by its place among the block's.
        span: usize,
    },
}

impl Accessible for SimpleTextView {
    type Part = TextViewPart;
    /// It says nothing back: scrolling is all a tool can do to it.
    type Event = core::convert::Infallible;

    fn automation(&self, _width: f32, _height: f32) -> Node<TextViewPart> {
        let text = (0..self.lines.len())
            .map(|line| self.line_text(line))
            .collect::<Vec<_>>()
            .join("\n");
        let mut node = Node::new(
            TextViewPart::View,
            Role::Document,
            "Text",
            Rect::new(0.0, 0.0, self.width, self.height),
        );
        node.value = Some(Value::Text(text));
        node
    }

    fn invoke(
        &mut self,
        part: &TextViewPart,
        action: Action,
        _width: f32,
        _height: f32,
    ) -> Result<Option<core::convert::Infallible>, Refusal> {
        let TextViewPart::View = part;
        match action {
            Action::ScrollTo { x, y } => {
                if !x.is_finite() || !y.is_finite() {
                    return Err(Refusal::NotANumber);
                }
                // Whole lines, as the wheel moves it: the line the height
                // falls on. A view whose lines have no height has one place.
                self.scroll_offset = if self.config.line_height > 0.0 {
                    (y.max(0.0) / self.config.line_height) as usize
                } else {
                    0
                };
                self.clamp_scroll();
                Ok(None)
            }
            _ => Err(Refusal::NotApplicable {
                role: Role::Document,
                action: action.name(),
            }),
        }
    }
}

/// A run of a block's spans, side by side, that link to one place.
struct LinkRun {
    /// Its first span, by its place among the block's.
    span: usize,
    /// Where it goes.
    url: String,
    /// What it says.
    text: String,
    /// Where it starts and ends in the block's text, in characters.
    from: usize,
    to: usize,
}

/// The spans of `block` that hold its text: none for a block of code, a
/// rule or a picture, which hold no links.
fn spans_of(block: &RichBlock) -> &[RichSpan] {
    match block {
        RichBlock::Paragraph { spans, .. }
        | RichBlock::Heading { spans, .. }
        | RichBlock::ListItem { spans, .. } => spans,
        RichBlock::CodeBlock { .. }
        | RichBlock::HorizontalRule
        | RichBlock::ImagePlaceholder { .. } => &[],
    }
}

/// The links among `spans`, in order: a run of spans side by side linking
/// to one place is one link.
fn link_runs(spans: &[RichSpan]) -> Vec<LinkRun> {
    let mut runs: Vec<LinkRun> = Vec::new();
    let mut at = 0usize;
    for (index, span) in spans.iter().enumerate() {
        let end = at.saturating_add(span.text.chars().count());
        if let Some(url) = &span.style.link
            && !span.text.is_empty()
        {
            match runs.last_mut() {
                Some(run) if run.to == at && run.url == *url => {
                    run.text.push_str(&span.text);
                    run.to = end;
                }
                _ => runs.push(LinkRun {
                    span: index,
                    url: url.clone(),
                    text: span.text.clone(),
                    from: at,
                    to: end,
                }),
            }
        }
        at = end;
    }
    runs
}

/// The smallest box holding all of `boxes`; `None` for none.
fn union(boxes: &[Rect]) -> Option<Rect> {
    boxes.iter().copied().reduce(|a, b| {
        let (left, top) = (a.x.min(b.x), a.y.min(b.y));
        Rect::new(
            left,
            top,
            a.right().max(b.right()) - left,
            a.bottom().max(b.bottom()) - top,
        )
    })
}

impl RichTextView {
    /// The view laid out: as it was last laid out, or -- when something
    /// since has made that stale -- as it will be when next drawn.
    fn laid_out(&self) -> Cow<'_, [WrappedLine]> {
        if self.layout_dirty {
            Cow::Owned(self.layout().0)
        } else {
            Cow::Borrowed(&self.wrapped_lines)
        }
    }

    /// The boxes characters `from..to` of block `block` are drawn in, in
    /// the view's text before it is scrolled -- one or more to a row they
    /// run along.
    fn boxes_of(&self, lines: &[WrappedLine], block: usize, from: usize, to: usize) -> Vec<Rect> {
        let gutter = self.gutter_width_for(lines.len());
        let mut boxes = Vec::new();
        let mut seen = 0usize;
        for line in lines.iter().filter(|line| line.block_idx == block) {
            let length: usize = line.spans.iter().map(|s| s.text.chars().count()).sum();
            let end = seen.saturating_add(length);
            if end > from && seen < to {
                let lo = from.max(seen).saturating_sub(seen);
                let hi = to.min(end).saturating_sub(seen);
                for (x, w) in self.selection_boxes_of_cols(line, lo, hi) {
                    boxes.push(Rect::new(
                        gutter + line.indent + x,
                        line.y,
                        w,
                        line.line_height,
                    ));
                }
            }
            seen = end;
        }
        boxes
    }

    /// Whether `bounds`, on screen, shows in the view.
    fn shows(&self, bounds: Rect) -> bool {
        bounds.bottom() > 0.0 && bounds.y < self.height
    }

    /// Block `index`'s node, its links under it, laid out as `lines`.
    fn block_node(&self, lines: &[WrappedLine], index: usize) -> Option<Node<RichTextPart>> {
        let block = self.blocks.get(index)?;
        let gutter = self.gutter_width_for(lines.len());
        let mut rows = lines.iter().filter(|line| line.block_idx == index);
        let first = rows.next()?;
        let last = rows.next_back().unwrap_or(first);
        let bounds = Rect::new(
            gutter,
            first.y - self.scroll_offset_px,
            (self.width - gutter).max(0.0),
            last.y + last.line_height - first.y,
        );
        let words = |spans: &[RichSpan]| spans.iter().map(|s| s.text.as_str()).collect::<String>();
        let (role, name) = match block {
            RichBlock::Heading { spans, .. } => (Role::Heading, words(spans)),
            RichBlock::Paragraph { spans, .. } => (Role::Paragraph, words(spans)),
            RichBlock::ListItem { spans, .. } => (Role::ListItem, words(spans)),
            RichBlock::CodeBlock { code, .. } => (Role::Paragraph, code.clone()),
            RichBlock::HorizontalRule => (Role::Separator, String::new()),
            RichBlock::ImagePlaceholder { alt_text, .. } => (Role::Image, alt_text.clone()),
        };
        let mut node = Node::new(RichTextPart::Block(index), role, name, bounds);
        node.shown = self.shows(bounds);
        for run in link_runs(spans_of(block)) {
            let boxes: Vec<Rect> = self
                .boxes_of(lines, index, run.from, run.to)
                .into_iter()
                .map(|b| b.translated(0.0, -self.scroll_offset_px))
                .collect();
            let Some(bounds) = union(&boxes) else {
                continue;
            };
            let mut link = Node::new(
                RichTextPart::Link {
                    block: index,
                    span: run.span,
                },
                Role::Link,
                run.text,
                bounds,
            );
            link.value = Some(Value::Text(run.url));
            link.shown = boxes.iter().any(|b| self.shows(*b));
            node.children.push(link);
        }
        Some(node)
    }

    /// Press the link starting at span `span` of block `block`, as its user
    /// would: scrolled into sight, then clicked at the middle of its first
    /// box.
    fn press_link(&mut self, block: usize, span: usize) -> Result<Option<RichTextEvent>, Refusal> {
        self.ensure_layout();
        let run = self
            .blocks
            .get(block)
            .map(|b| link_runs(spans_of(b)))
            .and_then(|runs| runs.into_iter().find(|run| run.span == span))
            .ok_or(Refusal::NoSuchWidget)?;
        let first = self
            .boxes_of(&self.wrapped_lines, block, run.from, run.to)
            .first()
            .copied()
            .ok_or(Refusal::Hidden)?;

        // Into sight: the least scroll that shows its first box whole.
        let was = self.scroll_offset_px;
        if first.y < self.scroll_offset_px {
            self.scroll_offset_px = first.y;
        } else if first.bottom() > self.scroll_offset_px + self.height {
            self.scroll_offset_px = first.bottom() - self.height;
        }
        self.clamp_scroll();
        let (x, y) = first.translated(0.0, -self.scroll_offset_px).centre();
        // Where a click could not land on it -- its middle outside a view
        // too short to show it -- it is refused, and nothing has moved.
        let inside = (0.0..self.width).contains(&x) && (0.0..self.height).contains(&y);
        if !inside || self.link_at(x, y).as_deref() != Some(run.url.as_str()) {
            self.scroll_offset_px = was;
            return Err(Refusal::Hidden);
        }
        let (_, said) = self.handle_event(&click(MouseEventKind::Press(MouseButton::Left), x, y));
        // The let go ends the click; it says nothing of its own.
        let (_, _released) =
            self.handle_event(&click(MouseEventKind::Release(MouseButton::Left), x, y));
        Ok(said)
    }
}

/// The pointer's `kind` at `(x, y)` in the view.
fn click(kind: MouseEventKind, x: f32, y: f32) -> Event {
    Event::Mouse(MouseEvent { x, y, kind })
}

impl Accessible for RichTextView {
    type Part = RichTextPart;
    type Event = RichTextEvent;

    fn automation(&self, _width: f32, _height: f32) -> Node<RichTextPart> {
        let lines = self.laid_out();
        let mut root = Node::new(
            RichTextPart::View,
            Role::Document,
            "Document",
            Rect::new(0.0, 0.0, self.width, self.height),
        );
        root.value = Some(Value::Text(self.plain_text()));
        root.children = (0..self.blocks.len())
            .filter_map(|index| self.block_node(&lines, index))
            .collect();
        root
    }

    fn invoke(
        &mut self,
        part: &RichTextPart,
        action: Action,
        _width: f32,
        _height: f32,
    ) -> Result<Option<RichTextEvent>, Refusal> {
        let not_for = |role: Role| Refusal::NotApplicable {
            role,
            action: action.name(),
        };
        match (*part, &action) {
            (RichTextPart::View, Action::ScrollTo { x, y }) => {
                if !x.is_finite() || !y.is_finite() {
                    return Err(Refusal::NotANumber);
                }
                self.scroll_offset_px = *y;
                self.clamp_scroll();
                Ok(None)
            }
            (RichTextPart::View, _) => Err(not_for(Role::Document)),
            (RichTextPart::Link { block, span }, Action::Press) => self.press_link(block, span),
            (RichTextPart::Link { block, span }, _) => {
                let is_link = self
                    .blocks
                    .get(block)
                    .is_some_and(|b| link_runs(spans_of(b)).iter().any(|run| run.span == span));
                Err(if is_link {
                    not_for(Role::Link)
                } else {
                    Refusal::NoSuchWidget
                })
            }
            (RichTextPart::Block(index), _) => {
                let lines = self.laid_out();
                let role = self
                    .block_node(&lines, index)
                    .ok_or(Refusal::NoSuchWidget)?
                    .role;
                Err(not_for(role))
            }
        }
    }
}

#[cfg(test)]
#[path = "accessible_tests.rs"]
mod tests;
