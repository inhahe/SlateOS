//! Drawing a widget tree's controls: each kind through the toolkit's
//! component module for it, in the palette the tree draws with.
//!
//! The tree drew its controls itself, in colours of its own -- a progress
//! bar in Windows' selection blue whatever the user's accent, text in black
//! whatever the theme -- and five of its kinds not at all. A control drawn
//! here is drawn by the same function that draws it everywhere else in the
//! toolkit, so the two cannot disagree.

use super::{SLIDER_THUMB, SLIDER_TRACK, Widget, WidgetKind, weight_to_hint};
use crate::color::Color;
use crate::frame::Rect;
use crate::palette::{Palette, readable_on};
use crate::render::{RenderCommand, RenderTree, TextOverflow};
use crate::style::{CornerRadii, FOCUS_RING_WIDTH};

impl Widget {
    /// The colour of this widget's text: its style's, or else the palette's
    /// text -- the disabled grey for a widget that cannot be used.
    pub(super) fn ink(&self, p: &Palette) -> Color {
        self.style
            .foreground
            .unwrap_or(if self.enabled { p.text } else { p.overlay0 })
    }

    /// What a selection in this widget is painted in, and the ink on it: its
    /// style's, or else the user's accent and what reads on it.
    fn selection(&self, p: &Palette) -> (Color, Color) {
        (
            self.style.selection_bg.unwrap_or(p.accent),
            self.style
                .selection_fg
                .unwrap_or_else(|| readable_on(p.accent)),
        )
    }

    /// The box a text field's text is measured and drawn in: its content box,
    /// in its own font.
    pub(super) fn text_metrics(&self) -> crate::textarea::Metrics {
        crate::textarea::Metrics::new(
            self.layout.width,
            self.layout.height,
            self.style.font_size,
            weight_to_hint(self.style.font_weight),
        )
    }

    /// Where this widget's slider is drawn and hit, in its parent's content
    /// space: a track across its content box, centred down it, with half a
    /// thumb's room at each end so the thumb stays inside.
    pub(super) fn slider_placement(&self) -> crate::slider::Placement {
        let (cx, cy) = self.content_origin();
        let (cw, ch) = (self.layout.width, self.layout.height);
        crate::slider::Placement::horizontal(
            Rect::new(
                cx + SLIDER_THUMB / 2.0,
                cy + (ch - SLIDER_TRACK) / 2.0,
                (cw - SLIDER_THUMB).max(0.0),
                SLIDER_TRACK,
            ),
            SLIDER_THUMB,
        )
    }

    /// What a text field shows of itself: lit under the pointer, ringed with
    /// the keyboard, greyed when it cannot be used.
    const fn field_state(&self) -> crate::field::State {
        crate::field::State {
            hovered: self.hovered,
            focused: self.focused,
            disabled: !self.enabled,
            invalid: false,
        }
    }

    /// Draw what this widget is -- not the background and border its style
    /// gives every widget, nor its children -- with its border box at
    /// `(x, y, w, h)` in its parent's content space, on `ground`, the
    /// colour behind it.
    pub(super) fn draw_kind(
        &self,
        p: &Palette,
        ground: Color,
        tree: &mut RenderTree,
        (x, y, w, h): (f32, f32, f32, f32),
    ) {
        let (cx, cy) = self.content_origin();
        let (cw, ch) = (self.layout.width, self.layout.height);
        let disabled = !self.enabled;
        match &self.kind {
            // What they hold is drawn as their children.
            WidgetKind::Container | WidgetKind::ScrollView { .. } => {}
            WidgetKind::Label { text } => tree.push(RenderCommand::Text {
                x: cx,
                y: cy,
                text: text.clone(),
                color: self.ink(p),
                font_size: self.style.font_size,
                font_weight: weight_to_hint(self.style.font_weight),
                max_width: Some(cw),
                overflow: TextOverflow::Ellipsis,
            }),
            WidgetKind::Button { text, pressed } => crate::button::draw(
                tree,
                p,
                (x, y, w, h),
                text,
                crate::button::Kind::Plain,
                crate::button::State {
                    hovered: self.hovered,
                    pressed: *pressed,
                    disabled,
                    focused: self.focused,
                },
                ground,
                FOCUS_RING_WIDTH,
            ),
            WidgetKind::TextInput {
                value,
                placeholder,
                cursor,
                selection_anchor,
            } => {
                crate::field::draw(
                    tree,
                    p,
                    Rect::new(x, y, w, h),
                    self.field_state(),
                    FOCUS_RING_WIDTH,
                );
                self.draw_line_of_text(p, tree, value, placeholder, *cursor, *selection_anchor);
            }
            WidgetKind::TextArea {
                area, placeholder, ..
            } => {
                crate::field::draw(
                    tree,
                    p,
                    Rect::new(x, y, w, h),
                    self.field_state(),
                    FOCUS_RING_WIDTH,
                );
                let (selection_bg, selection_fg) = self.selection(p);
                crate::textarea::draw(
                    tree,
                    &crate::textarea::MultiLine {
                        area,
                        x: cx,
                        y: cy,
                        metrics: self.text_metrics(),
                        color: self.ink(p),
                        selection_bg,
                        selection_fg,
                        focused: self.focused,
                        caret_width: self.style.caret_width,
                        placeholder: Some((placeholder, p.subtext0)),
                    },
                );
            }
            WidgetKind::Checkbox { checked, label } => crate::checkbox::draw(
                tree,
                p,
                (cx, cy, ch),
                label,
                *checked,
                crate::checkbox::State {
                    hovered: self.hovered,
                    focused: self.focused,
                    disabled,
                },
                FOCUS_RING_WIDTH,
            ),
            WidgetKind::RadioButton { selected, label } => crate::radio::draw(
                tree,
                p,
                (cx, cy, ch),
                label,
                *selected,
                crate::radio::State {
                    hovered: self.hovered,
                    focused: self.focused,
                    disabled,
                },
                FOCUS_RING_WIDTH,
            ),
            WidgetKind::Slider { slider } => {
                let placement = self.slider_placement();
                let look = crate::slider::Look::accent(p, p.surface1);
                if disabled {
                    // The slider's own disabled drawing, for a widget the
                    // program has disabled: the slider cannot know that.
                    let dimmed = crate::slider::Look {
                        alpha: dimmed_alpha(look.alpha),
                        ..look
                    };
                    crate::slider::draw_bar(tree, p, &placement, slider.fraction(), dimmed);
                } else {
                    slider.draw(tree, p, &placement, look, self.focused, FOCUS_RING_WIDTH);
                }
            }
            WidgetKind::ProgressBar { value, max } => {
                let radius = CornerRadii::all(ch / 2.0);
                tree.push(RenderCommand::FillRect {
                    x: cx,
                    y: cy,
                    width: cw,
                    height: ch,
                    color: p.surface0,
                    corner_radii: radius,
                });
                // A share that is not a number, or of nothing, is none.
                let share = if *max > 0.0 && value.is_finite() {
                    (value / max).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                if share > 0.0 {
                    tree.push(RenderCommand::FillRect {
                        x: cx,
                        y: cy,
                        width: cw * share,
                        height: ch,
                        color: if disabled { p.overlay0 } else { p.accent },
                        corner_radii: radius,
                    });
                }
            }
            WidgetKind::Separator { vertical } => {
                let color = self.style.foreground.unwrap_or(p.border);
                let (x1, y1, x2, y2) = if *vertical {
                    (cx + cw / 2.0, cy, cx + cw / 2.0, cy + ch)
                } else {
                    (cx, cy + ch / 2.0, cx + cw, cy + ch / 2.0)
                };
                tree.push(RenderCommand::Line {
                    x1,
                    y1,
                    x2,
                    y2,
                    color,
                    width: 1.0,
                });
            }
            WidgetKind::Image { image_id, .. } => tree.push(RenderCommand::Image {
                x: cx,
                y: cy,
                width: cw,
                height: ch,
                image_id: *image_id,
            }),
        }
    }

    /// A one-line field's text: its placeholder while it is empty -- never
    /// scrolled, never selected, cut with an ellipsis, since a hint cut
    /// silently reads as the whole hint -- else the text, its selection and
    /// its caret, drawn as the toolkit's other one-line fields draw them.
    fn draw_line_of_text(
        &self,
        p: &Palette,
        tree: &mut RenderTree,
        value: &str,
        placeholder: &str,
        cursor: crate::text::TextCursor,
        selection_anchor: Option<usize>,
    ) {
        let (cx, cy) = self.content_origin();
        let line_h = self.style.font_size * self.style.line_height;
        if value.is_empty() {
            tree.push(RenderCommand::Text {
                x: cx,
                y: cy,
                text: placeholder.to_owned(),
                color: p.subtext0,
                font_size: self.style.font_size,
                font_weight: weight_to_hint(self.style.font_weight),
                max_width: Some(self.layout.width),
                overflow: TextOverflow::Ellipsis,
            });
            if self.focused {
                crate::textedit::push_caret(
                    tree,
                    cx,
                    cy,
                    line_h,
                    self.ink(p),
                    self.style.caret_width,
                );
            }
            return;
        }
        let (selection_bg, selection_fg) = self.selection(p);
        crate::textedit::draw(
            tree,
            &crate::textedit::SingleLine {
                text: value,
                cursor,
                selection_anchor,
                focused: self.focused,
                x: cx,
                y: cy,
                width: self.layout.width,
                line_height: line_h,
                font_size: self.style.font_size,
                weight: weight_to_hint(self.style.font_weight),
                color: self.ink(p),
                selection_bg,
                selection_fg,
                caret_width: self.style.caret_width,
            },
        );
    }

    /// The bars of a scroll view whose content overflows it, over the
    /// content: down its right edge for content taller than it, across its
    /// foot for content wider -- each stopping short of the corner where
    /// both are.
    pub(super) fn draw_scrollbars(&self, p: &Palette, tree: &mut RenderTree) {
        let WidgetKind::ScrollView {
            scroll_x,
            scroll_y,
            content_width,
            content_height,
        } = self.kind
        else {
            return;
        };
        let (cx, cy) = self.content_origin();
        let (cw, ch) = (self.layout.width, self.layout.height);
        let bar = crate::scrollbar::WIDTH;
        let state = crate::scrollbar::BarState {
            hovered: self.hovered,
            dragging: false,
        };
        let down = content_height > ch && ch > 0.0;
        let across = content_width > cw && cw > 0.0;
        let corner = |both: bool| if both { bar } else { 0.0 };
        if down {
            let track = Rect::new(cx + cw - bar, cy, bar, (ch - corner(across)).max(0.0));
            let thumb = crate::scrollbar::thumb_of(
                track,
                ch / content_height,
                scroll_y / (content_height - ch),
                crate::scrollbar::MIN_THUMB,
            );
            crate::scrollbar::draw(tree, p, track, thumb, state);
        }
        if across {
            let track = Rect::new(cx, cy + ch - bar, (cw - corner(down)).max(0.0), bar);
            let thumb = crate::scrollbar::thumb_across(
                track,
                cw / content_width,
                scroll_x / (content_width - cw),
                crate::scrollbar::MIN_THUMB,
            );
            crate::scrollbar::draw_across(tree, p, track, thumb, state);
        }
    }
}

/// `alpha` at the toolkit's disabled opacity.
fn dimmed_alpha(alpha: u8) -> u8 {
    // In 0..=255: a byte times a fraction under one, rounded.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a byte times the disabled opacity, which is under one"
    )]
    let dimmed = (f32::from(alpha) * crate::disabled::DISABLED_OPACITY).round() as u8;
    dimmed
}
