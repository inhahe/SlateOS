//! The toolkit's layout engine: CSS Flexbox's algorithm ([`flex_layout`]),
//! as native code over sizes rather than through style sheets.
//!
//! A container ([`FlexLayout`]) sets the direction, whether its items wrap
//! into lines, how they are placed along a line (`justify`) and across it
//! (`align_items`), how its lines share its thickness (`align_content`) and
//! the gaps. Each item ([`FlexItem`]) sets how it grows and shrinks, its
//! starting size, its own cross alignment, the least and most it may be,
//! its margins and its baseline. The answer is a box per item.
//!
//! What it does follows the specification's order (Flexbox §9, the steps
//! are on [`flex_layout`]): lines, flexible lengths with min/max freezing,
//! line thickness, `align-content`, `justify-content`, `align-self`, and the
//! reversed directions as mirror images. What it leaves out, as a desktop UI
//! has not needed it: `order`, auto margins, percentages, aspect ratios and
//! absolutely positioned children.

use crate::style::Edges;

pub mod grid;

/// A 2D size.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const ZERO: Self = Self {
        width: 0.0,
        height: 0.0,
    };

    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// A 2D position.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// Axis direction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Axis {
    #[default]
    Horizontal,
    Vertical,
}

/// Flex layout direction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlexDirection {
    #[default]
    Row,
    Column,
    RowReverse,
    ColumnReverse,
}

impl FlexDirection {
    pub fn main_axis(self) -> Axis {
        match self {
            Self::Row | Self::RowReverse => Axis::Horizontal,
            Self::Column | Self::ColumnReverse => Axis::Vertical,
        }
    }

    pub fn is_reversed(self) -> bool {
        matches!(self, Self::RowReverse | Self::ColumnReverse)
    }
}

/// Flex wrap behavior.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlexWrap {
    #[default]
    NoWrap,
    Wrap,
    WrapReverse,
}

/// Alignment along the main axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlexJustify {
    #[default]
    Start,
    End,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

/// Alignment along the cross axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlexAlign {
    #[default]
    Start,
    End,
    Center,
    Stretch,
    Baseline,
}

/// How the lines of a wrapping container share its cross axis -- CSS's
/// `align-content`. A container that does not wrap has one line, as tall
/// (or wide) as the container, and this does nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlignContent {
    /// Lines packed at the cross axis's start.
    Start,
    /// Packed at its end.
    End,
    /// Packed in its middle.
    Center,
    /// Each line made taller (or wider) by an equal share of the room left.
    #[default]
    Stretch,
    /// The room left between the lines, none outside them.
    SpaceBetween,
    /// Half a share outside the first and last lines, a share between.
    SpaceAround,
    /// Equal shares outside and between.
    SpaceEvenly,
}

/// Layout properties for a flex container.
#[derive(Clone, Debug)]
pub struct FlexLayout {
    pub direction: FlexDirection,
    pub wrap: FlexWrap,
    pub justify: FlexJustify,
    pub align_items: FlexAlign,
    /// How a wrapping container's lines share its cross axis.
    pub align_content: AlignContent,
    /// The space between neighbouring items on a line.
    pub gap: f32,
    /// The space between lines when the container wraps; `None` is `gap`,
    /// as CSS's `gap` sets both.
    pub line_gap: Option<f32>,
}

impl Default for FlexLayout {
    fn default() -> Self {
        Self {
            direction: FlexDirection::Row,
            wrap: FlexWrap::NoWrap,
            justify: FlexJustify::Start,
            align_items: FlexAlign::Stretch,
            align_content: AlignContent::Stretch,
            gap: 0.0,
            line_gap: None,
        }
    }
}

/// Layout properties for a flex child item.
#[derive(Clone, Debug)]
pub struct FlexItem {
    /// How much this item should grow relative to siblings.
    pub grow: f32,
    /// How much this item should shrink relative to siblings.
    pub shrink: f32,
    /// Initial size before grow/shrink (None = auto/content size).
    pub basis: Option<f32>,
    /// Override alignment for this item (overrides container's align_items).
    pub align_self: Option<FlexAlign>,
    /// The least the item may be, across and along: growing, shrinking and
    /// stretching all stop here. Zero is no limit.
    pub min: Size,
    /// The most it may be; `f32::INFINITY` is no limit.
    pub max: Size,
    /// Room kept clear around the item, outside its box: its neighbours,
    /// the gaps and the container's padding are measured from here.
    pub margin: Edges,
    /// How far below the item's top its first line of text sits, for
    /// [`FlexAlign::Baseline`]. `None` is its bottom edge, as CSS takes an
    /// item with no text to have.
    pub baseline: Option<f32>,
}

impl Default for FlexItem {
    fn default() -> Self {
        Self {
            grow: 0.0,
            shrink: 1.0,
            basis: None,
            align_self: None,
            min: Size::ZERO,
            max: Size::new(f32::INFINITY, f32::INFINITY),
            margin: Edges::ZERO,
            baseline: None,
        }
    }
}

/// Computed layout box for a widget after layout pass.
#[derive(Clone, Debug, Default)]
pub struct LayoutBox {
    /// Position relative to parent's content area.
    pub x: f32,
    pub y: f32,
    /// Content size (excluding padding and border).
    pub width: f32,
    pub height: f32,
    /// Padding.
    pub padding: Edges,
    /// Border widths.
    pub border_widths: Edges,
    /// Margin.
    pub margin: Edges,
}

impl LayoutBox {
    /// Total outer width including padding, border, and margin.
    pub fn outer_width(&self) -> f32 {
        self.margin.left
            + self.border_widths.left
            + self.padding.left
            + self.width
            + self.padding.right
            + self.border_widths.right
            + self.margin.right
    }

    /// Total outer height including padding, border, and margin.
    pub fn outer_height(&self) -> f32 {
        self.margin.top
            + self.border_widths.top
            + self.padding.top
            + self.height
            + self.padding.bottom
            + self.border_widths.bottom
            + self.margin.bottom
    }

    /// Content area origin (inside padding and border).
    pub fn content_x(&self) -> f32 {
        self.x + self.margin.left + self.border_widths.left + self.padding.left
    }

    pub fn content_y(&self) -> f32 {
        self.y + self.margin.top + self.border_widths.top + self.padding.top
    }

    /// Border box (position + padding + content, no margin).
    pub fn border_box_width(&self) -> f32 {
        self.border_widths.left
            + self.padding.left
            + self.width
            + self.padding.right
            + self.border_widths.right
    }

    pub fn border_box_height(&self) -> f32 {
        self.border_widths.top
            + self.padding.top
            + self.height
            + self.padding.bottom
            + self.border_widths.bottom
    }
}

/// Size constraint passed during layout.
#[derive(Clone, Copy, Debug)]
pub struct SizeConstraint {
    pub min_width: f32,
    pub max_width: f32,
    pub min_height: f32,
    pub max_height: f32,
}

impl SizeConstraint {
    pub fn tight(size: Size) -> Self {
        Self {
            min_width: size.width,
            max_width: size.width,
            min_height: size.height,
            max_height: size.height,
        }
    }

    pub fn loose(max: Size) -> Self {
        Self {
            min_width: 0.0,
            max_width: max.width,
            min_height: 0.0,
            max_height: max.height,
        }
    }

    pub fn unbounded() -> Self {
        Self {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: 0.0,
            max_height: f32::INFINITY,
        }
    }

    /// `size` held within the constraint -- the minimum winning where a
    /// minimum is larger than its maximum, as CSS has it. (`f32::clamp`
    /// panicked there: a widget whose style gave it a `min_width` above its
    /// `max_width` took the whole window down.)
    pub fn constrain(&self, size: Size) -> Size {
        Size {
            width: hold(size.width, self.min_width, self.max_width),
            height: hold(size.height, self.min_height, self.max_height),
        }
    }
}

/// Lay `children` out in a flex container `container_size` big, inside its
/// `padding`: CSS Flexbox's algorithm, for what a desktop UI uses.
///
/// Each child is its content size and its [`FlexItem`] properties; the
/// answer is a box per child in the same order, `x` and `y` the corner of its
/// margin box in the container's space (the box's `margin` is the item's, so
/// [`LayoutBox::content_x`] is where its border box starts), `width` and
/// `height` its size.
///
/// What it does, in CSS's order (Flexbox §9):
///
/// 1. Each item's base size is its `basis`, else its content size; its
///    hypothetical size is that held between its `min` and `max`.
/// 2. A container that wraps breaks its items into lines where the next one
///    would not fit; one that does not is a single line.
/// 3. Each line's free space goes to its items by `grow` -- or, when the
///    line is too long, comes from them by `shrink` weighted by their base
///    sizes -- and an item that would pass its `min` or `max` stops there
///    while the rest share what is left (§9.7's freezing).
/// 4. A line is as thick as its thickest item (a single line, the whole
///    container); `align_content` shares the container's spare thickness
///    among the lines of a wrapping one.
/// 5. `justify` places the items along each line, `align_self` (else
///    `align_items`) across it. `Stretch` makes an item as thick as its line
///    within its `max`; `Baseline` lines the items' `baseline`s up, in a row.
/// 6. A reversed direction is the same layout mirrored along the main axis,
///    `WrapReverse` across it -- which is what CSS's main-start and
///    cross-start sides being swapped comes to.
///
/// An unbounded side (`f32::INFINITY`, a container sized to its content)
/// has no free space: nothing grows, shrinks or wraps along it, and a
/// reversed layout mirrors against what the items take.
///
/// Where an item's margins are set, they are kept on the side they are set
/// on: `margin.left` stays on the left in a reversed row.
pub fn flex_layout(
    container_size: Size,
    flex: &FlexLayout,
    children: &[(Size, FlexItem)],
    padding: &Edges,
) -> Vec<LayoutBox> {
    if children.is_empty() {
        return Vec::new();
    }
    let horizontal = flex.direction.main_axis() == Axis::Horizontal;
    let (avail_main, avail_cross) = along(
        horizontal,
        Size::new(
            (container_size.width - padding.horizontal()).max(0.0),
            (container_size.height - padding.vertical()).max(0.0),
        ),
    );
    let main_gap = flex.gap.max(0.0);
    let cross_gap = flex.line_gap.unwrap_or(flex.gap).max(0.0);
    let wraps = flex.wrap != FlexWrap::NoWrap;
    let reversed_main = flex.direction.is_reversed();
    let reversed_cross = flex.wrap == FlexWrap::WrapReverse;

    // 1. Each item's numbers on the two axes. The margin on each side is the
    // one on the side the layout starts from, so that mirroring a reversed
    // layout at the end puts each margin back where it was set.
    let mut items: Vec<FlexLine> = children
        .iter()
        .map(|(size, item)| {
            let (content_main, content_cross) = along(horizontal, *size);
            let (min_main, min_cross) = along(horizontal, item.min);
            let (max_main, max_cross) = along(horizontal, item.max);
            let m = &item.margin;
            let (main_margins, cross_margins) = if horizontal {
                ((m.left, m.right), (m.top, m.bottom))
            } else {
                ((m.top, m.bottom), (m.left, m.right))
            };
            let swap = |(a, b): (f32, f32), flip: bool| if flip { (b, a) } else { (a, b) };
            let base = item.basis.unwrap_or(content_main).max(0.0);
            FlexLine {
                base,
                hypothetical: hold(base, min_main, max_main),
                main: 0.0,
                min_main,
                max_main,
                cross: hold(content_cross.max(0.0), min_cross, max_cross),
                min_cross,
                max_cross,
                margin_main: swap(main_margins, reversed_main),
                margin_cross: swap(cross_margins, reversed_cross),
                grow: item.grow.max(0.0),
                shrink: item.shrink.max(0.0),
                align: item.align_self.unwrap_or(flex.align_items),
                baseline: item.baseline,
                main_pos: 0.0,
                cross_pos: 0.0,
            }
        })
        .collect();

    // 2. Lines.
    let lines = break_lines(&items, wraps, avail_main, main_gap);

    // 3. Flexible lengths, line by line.
    for line in &lines {
        if let Some(slice) = items.get_mut(line.clone()) {
            resolve_flexible_lengths(slice, avail_main, main_gap);
        }
    }

    // 4. Each line's thickness, then what the container's spare thickness
    // does to the lines.
    let mut thickness: Vec<f32> = lines
        .iter()
        .map(|line| {
            items
                .get(line.clone())
                .map_or(0.0, |s| line_thickness(s, horizontal))
        })
        .collect();
    if !wraps && avail_cross.is_finite() {
        // A single line is the container's thickness, whatever is in it.
        thickness.fill(avail_cross);
    }
    let line_starts = place_lines(
        &mut thickness,
        if wraps { avail_cross } else { f32::INFINITY },
        cross_gap,
        flex.align_content,
    );

    // 5. Along each line, then across it.
    let mut extent_main = 0.0_f32;
    for line in &lines {
        if let Some(slice) = items.get_mut(line.clone()) {
            extent_main = extent_main.max(justify_line(slice, avail_main, main_gap, flex.justify));
        }
    }
    for ((line, &start), &thick) in lines.iter().zip(&line_starts).zip(&thickness) {
        if let Some(slice) = items.get_mut(line.clone()) {
            align_line(slice, start, thick, horizontal);
        }
    }

    // 6. Mirrored where reversed: against the container, or against what
    // the items take where the container has no end on that side.
    let extent_main = if avail_main.is_finite() {
        avail_main
    } else {
        extent_main
    };
    let extent_cross = if wraps && avail_cross.is_finite() {
        avail_cross
    } else {
        line_starts
            .iter()
            .zip(&thickness)
            .map(|(s, t)| s + t)
            .fold(0.0_f32, f32::max)
    };
    items
        .iter()
        .zip(children)
        .map(|(it, (_, item))| {
            let mut main = it.main_pos;
            if reversed_main {
                main = extent_main - main - it.main;
            }
            let mut cross = it.cross_pos;
            if reversed_cross {
                cross = extent_cross - cross - it.cross;
            }
            let (x, y, width, height) = if horizontal {
                (main, cross, it.main, it.cross)
            } else {
                (cross, main, it.cross, it.main)
            };
            LayoutBox {
                x: x + padding.left - item.margin.left,
                y: y + padding.top - item.margin.top,
                width,
                height,
                padding: Edges::ZERO,
                border_widths: Edges::ZERO,
                margin: item.margin,
            }
        })
        .collect()
}

/// `size` as `(main, cross)` for a layout whose main axis is horizontal or
/// not.
fn along(horizontal: bool, size: Size) -> (f32, f32) {
    if horizontal {
        (size.width, size.height)
    } else {
        (size.height, size.width)
    }
}

/// `value` held between `min` and `max` -- the minimum winning where the two
/// cross, as CSS has it, and without `f32::clamp`'s panic when they do.
fn hold(value: f32, min: f32, max: f32) -> f32 {
    value.min(max).max(min)
}

/// One item while its line is laid out: sizes along the main axis and
/// across, its margins on the side the layout starts from and the other,
/// and where it ends up.
struct FlexLine {
    base: f32,
    hypothetical: f32,
    main: f32,
    min_main: f32,
    max_main: f32,
    cross: f32,
    min_cross: f32,
    max_cross: f32,
    margin_main: (f32, f32),
    margin_cross: (f32, f32),
    grow: f32,
    shrink: f32,
    align: FlexAlign,
    baseline: Option<f32>,
    main_pos: f32,
    cross_pos: f32,
}

impl FlexLine {
    fn margins_main(&self) -> f32 {
        self.margin_main.0 + self.margin_main.1
    }

    fn margins_cross(&self) -> f32 {
        self.margin_cross.0 + self.margin_cross.1
    }
}

/// Where the lines break: one line unless `wraps`, else a new line wherever
/// the next item's margin box would pass `avail_main`. A line always takes
/// at least one item, however long.
fn break_lines(
    items: &[FlexLine],
    wraps: bool,
    avail_main: f32,
    main_gap: f32,
) -> Vec<core::ops::Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut used = 0.0_f32;
    for (i, it) in items.iter().enumerate() {
        let outer = it.hypothetical + it.margins_main();
        if i == start {
            used = outer;
            continue;
        }
        let longer = used + main_gap + outer;
        if wraps && longer > avail_main {
            lines.push(start..i);
            start = i;
            used = outer;
        } else {
            used = longer;
        }
    }
    lines.push(start..items.len());
    lines
}

/// Share a line's free space among its items by `grow`, or take its
/// overflow from them by `shrink` weighted by base size -- CSS Flexbox
/// §9.7, with the freezing that stops an item at its `min` or `max` and
/// shares what it could not take among the rest. No free space where the
/// container has no end.
fn resolve_flexible_lengths(items: &mut [FlexLine], avail_main: f32, main_gap: f32) {
    if !avail_main.is_finite() {
        for it in items.iter_mut() {
            it.main = it.hypothetical;
        }
        return;
    }
    #[allow(clippy::cast_precision_loss, reason = "a line's item count")]
    let gaps = main_gap * items.len().saturating_sub(1) as f32;
    let outer_hypothetical: f32 = items
        .iter()
        .map(|it| it.hypothetical + it.margins_main())
        .sum();
    let growing = outer_hypothetical + gaps < avail_main;
    // An item that cannot flex this way is frozen at its hypothetical size
    // from the start: no factor, or a base its limits already moved it off
    // in the direction of the flex.
    let mut frozen: Vec<bool> = items
        .iter()
        .map(|it| {
            let factor = if growing { it.grow } else { it.shrink };
            factor <= 0.0
                || (growing && it.base > it.hypothetical)
                || (!growing && it.base < it.hypothetical)
        })
        .collect();
    for it in items.iter_mut() {
        it.main = it.hypothetical;
    }
    let space = |items: &[FlexLine], frozen: &[bool]| -> f32 {
        let taken: f32 = items
            .iter()
            .zip(frozen)
            .map(|(it, &f)| if f { it.main } else { it.base } + it.margins_main())
            .sum();
        avail_main - taken - gaps
    };
    let initial = space(items, &frozen);
    while frozen.iter().any(|f| !f) {
        let mut free = space(items, &frozen);
        let factors: f32 = items
            .iter()
            .zip(&frozen)
            .filter(|(_, f)| !**f)
            .map(|(it, _)| if growing { it.grow } else { it.shrink })
            .sum();
        // Factors summing to less than one take only that fraction of the
        // space (§9.7 step 4b): two items of grow 0.25 leave half of it.
        if factors < 1.0 {
            let fraction = initial * factors;
            if fraction.abs() < free.abs() {
                free = fraction;
            }
        }
        if growing {
            for (it, _) in items.iter_mut().zip(&frozen).filter(|(_, f)| !**f) {
                it.main = it.base + free * it.grow / factors;
            }
        } else {
            let scaled: f32 = items
                .iter()
                .zip(&frozen)
                .filter(|(_, f)| !**f)
                .map(|(it, _)| it.shrink * it.base)
                .sum();
            for (it, _) in items.iter_mut().zip(&frozen).filter(|(_, f)| !**f) {
                it.main = if scaled > 0.0 {
                    it.base - free.abs() * it.shrink * it.base / scaled
                } else {
                    it.base
                };
            }
        }
        // Hold each to its limits, and freeze whichever way the holding
        // went on the whole -- every item the minimums moved, or every one
        // the maximums did, or all of them when nothing moved.
        let mut violation = 0.0_f32;
        let mut moved: Vec<f32> = vec![0.0; items.len()];
        for ((it, &f), m) in items.iter_mut().zip(&frozen).zip(&mut moved) {
            if f {
                continue;
            }
            let held = hold(it.main.max(0.0), it.min_main, it.max_main);
            *m = held - it.main;
            violation += *m;
            it.main = held;
        }
        for (f, m) in frozen.iter_mut().zip(&moved) {
            if *f {
                continue;
            }
            if violation > 0.0 {
                *f = *m > 0.0;
            } else if violation < 0.0 {
                *f = *m < 0.0;
            } else {
                *f = true;
            }
        }
    }
}

/// How thick a line of several lines is: its thickest item's margin box,
/// and, for items lined up by their baselines in a row, enough to line them
/// up.
fn line_thickness(items: &[FlexLine], horizontal: bool) -> f32 {
    let mut thickest = 0.0_f32;
    let (mut above, mut below) = (0.0_f32, 0.0_f32);
    for it in items {
        if horizontal && it.align == FlexAlign::Baseline {
            let rise = it.margin_cross.0 + it.baseline.unwrap_or(it.cross);
            above = above.max(rise);
            below = below.max(it.cross + it.margins_cross() - rise);
        } else {
            thickest = thickest.max(it.cross + it.margins_cross());
        }
    }
    thickest.max(above + below)
}

/// Where each line starts across the container, and how thick each ends
/// up: `align_content` sharing the spare thickness of `avail_cross` (none
/// where it is unbounded).
fn place_lines(
    thickness: &mut [f32],
    avail_cross: f32,
    cross_gap: f32,
    align: AlignContent,
) -> Vec<f32> {
    #[allow(clippy::cast_precision_loss, reason = "a container's line count")]
    let count = thickness.len() as f32;
    let gaps = cross_gap * (count - 1.0).max(0.0);
    let free = if avail_cross.is_finite() {
        avail_cross - thickness.iter().sum::<f32>() - gaps
    } else {
        0.0
    };
    let (mut at, between) = match align {
        AlignContent::Start => (0.0, 0.0),
        AlignContent::End => (free, 0.0),
        AlignContent::Center => (free / 2.0, 0.0),
        AlignContent::Stretch => {
            if free > 0.0 {
                for t in thickness.iter_mut() {
                    *t += free / count;
                }
            }
            (0.0, 0.0)
        }
        // Spare room shared out, and an overflow handled as CSS's fallbacks
        // do: from the start for SpaceBetween, from the middle otherwise.
        AlignContent::SpaceBetween if free > 0.0 && count > 1.0 => (0.0, free / (count - 1.0)),
        AlignContent::SpaceBetween => (0.0, 0.0),
        AlignContent::SpaceAround if free > 0.0 => (free / count / 2.0, free / count),
        AlignContent::SpaceEvenly if free > 0.0 => (free / (count + 1.0), free / (count + 1.0)),
        AlignContent::SpaceAround | AlignContent::SpaceEvenly => (free / 2.0, 0.0),
    };
    let mut starts = Vec::with_capacity(thickness.len());
    for t in thickness.iter() {
        starts.push(at);
        at += t + cross_gap + between;
    }
    starts
}

/// Place one line's items along it by `justify`, answering how far the
/// line reaches. An overflowing line falls back as CSS's do: from the start
/// for `SpaceBetween`, from the middle for `SpaceAround` and `SpaceEvenly`.
fn justify_line(
    items: &mut [FlexLine],
    avail_main: f32,
    main_gap: f32,
    justify: FlexJustify,
) -> f32 {
    #[allow(clippy::cast_precision_loss, reason = "a line's item count")]
    let count = items.len() as f32;
    let used: f32 = items
        .iter()
        .map(|it| it.main + it.margins_main())
        .sum::<f32>()
        + main_gap * (count - 1.0).max(0.0);
    let free = if avail_main.is_finite() {
        avail_main - used
    } else {
        0.0
    };
    let (mut at, between) = match justify {
        FlexJustify::Start => (0.0, 0.0),
        FlexJustify::End => (free, 0.0),
        FlexJustify::Center => (free / 2.0, 0.0),
        FlexJustify::SpaceBetween if free > 0.0 && count > 1.0 => (0.0, free / (count - 1.0)),
        FlexJustify::SpaceBetween => (0.0, 0.0),
        FlexJustify::SpaceAround if free > 0.0 => (free / count / 2.0, free / count),
        FlexJustify::SpaceEvenly if free > 0.0 => (free / (count + 1.0), free / (count + 1.0)),
        FlexJustify::SpaceAround | FlexJustify::SpaceEvenly => (free / 2.0, 0.0),
    };
    for (i, it) in items.iter_mut().enumerate() {
        if i > 0 {
            at += main_gap + between;
        }
        it.main_pos = at + it.margin_main.0;
        at += it.main + it.margins_main();
    }
    used
}

/// Place one line's items across it: a line `thick` thick starting at
/// `start`.
fn align_line(items: &mut [FlexLine], start: f32, thick: f32, horizontal: bool) {
    let rise = |it: &FlexLine| it.margin_cross.0 + it.baseline.unwrap_or(it.cross);
    let baseline = items
        .iter()
        .filter(|it| horizontal && it.align == FlexAlign::Baseline)
        .map(rise)
        .fold(0.0_f32, f32::max);
    for it in items.iter_mut() {
        let room = thick - it.margins_cross();
        let offset = match it.align {
            FlexAlign::Start => 0.0,
            FlexAlign::End => room - it.cross,
            FlexAlign::Center => (room - it.cross) / 2.0,
            FlexAlign::Stretch => {
                it.cross = hold(room.max(0.0), it.min_cross, it.max_cross);
                0.0
            }
            // A column has no baselines to line up; CSS starts them.
            FlexAlign::Baseline if horizontal => baseline - rise(it),
            FlexAlign::Baseline => 0.0,
        };
        it.cross_pos = start + it.margin_cross.0 + offset;
    }
}

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;

    #[test]
    fn test_basic_row_layout() {
        let container = Size::new(300.0, 100.0);
        let flex = FlexLayout::default(); // Row, no wrap, start
        let children = vec![
            (Size::new(50.0, 30.0), FlexItem::default()),
            (Size::new(80.0, 40.0), FlexItem::default()),
            (Size::new(60.0, 20.0), FlexItem::default()),
        ];

        let results = flex_layout(container, &flex, &children, &Edges::ZERO);

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].x, 0.0);
        assert_eq!(results[0].width, 50.0);
        assert_eq!(results[1].x, 50.0);
        assert_eq!(results[1].width, 80.0);
        assert_eq!(results[2].x, 130.0);
        assert_eq!(results[2].width, 60.0);
    }

    #[test]
    fn one_item_takes_no_gaps_and_no_space_between() {
        // The single-item case is where every "one fewer gap than items"
        // subtraction has to stop, and it reaches all four of them at once:
        // the total gap, the SpaceBetween divisor, and the per-item step in
        // each direction.
        let container = Size::new(300.0, 100.0);
        for direction in [FlexDirection::Row, FlexDirection::RowReverse] {
            let flex = FlexLayout {
                direction,
                gap: 20.0,
                justify: FlexJustify::SpaceBetween,
                ..FlexLayout::default()
            };
            let children = vec![(Size::new(50.0, 30.0), FlexItem::default())];
            let results = flex_layout(container, &flex, &children, &Edges::ZERO);
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].width, 50.0);
            // Start of the container going forward, flush to its end in
            // reverse. Either way no gap is laid down for an absent neighbour.
            let expected = if direction == FlexDirection::RowReverse {
                250.0
            } else {
                0.0
            };
            assert_eq!(results[0].x, expected, "{direction:?}");
        }
    }

    #[test]
    fn gaps_fall_between_items_and_never_outside_them() {
        // Three 50-wide items in a 300-wide row with a 20 gap: two gaps, so
        // the run is 190 wide and starts at 0. The same three in reverse end
        // flush against the right edge. Off-by-one in the gap count moves
        // every item after the first, so the interior positions are the
        // assertion that matters.
        let container = Size::new(300.0, 100.0);
        let children: Vec<_> = (0..3)
            .map(|_| (Size::new(50.0, 30.0), FlexItem::default()))
            .collect();

        let forward = flex_layout(
            container,
            &FlexLayout {
                gap: 20.0,
                ..FlexLayout::default()
            },
            &children,
            &Edges::ZERO,
        );
        assert_eq!(
            forward.iter().map(|b| b.x).collect::<Vec<_>>(),
            vec![0.0, 70.0, 140.0]
        );

        let reversed = flex_layout(
            container,
            &FlexLayout {
                direction: FlexDirection::RowReverse,
                gap: 20.0,
                ..FlexLayout::default()
            },
            &children,
            &Edges::ZERO,
        );
        assert_eq!(
            reversed.iter().map(|b| b.x).collect::<Vec<_>>(),
            vec![250.0, 180.0, 110.0]
        );
    }

    #[test]
    fn test_grow() {
        let container = Size::new(300.0, 100.0);
        let flex = FlexLayout::default();
        let children = vec![
            (
                Size::new(50.0, 30.0),
                FlexItem {
                    grow: 1.0,
                    ..Default::default()
                },
            ),
            (
                Size::new(50.0, 30.0),
                FlexItem {
                    grow: 2.0,
                    ..Default::default()
                },
            ),
        ];

        let results = flex_layout(container, &flex, &children, &Edges::ZERO);

        // Free space = 300 - 100 = 200. Split 1:2 = ~66.7 and ~133.3
        assert!((results[0].width - (50.0 + 200.0 / 3.0)).abs() < 0.1);
        assert!((results[1].width - (50.0 + 400.0 / 3.0)).abs() < 0.1);
    }

    #[test]
    fn test_column_layout() {
        let container = Size::new(100.0, 200.0);
        let flex = FlexLayout {
            direction: FlexDirection::Column,
            ..Default::default()
        };
        let children = vec![
            (Size::new(80.0, 40.0), FlexItem::default()),
            (Size::new(60.0, 50.0), FlexItem::default()),
        ];

        let results = flex_layout(container, &flex, &children, &Edges::ZERO);

        assert_eq!(results[0].y, 0.0);
        assert_eq!(results[0].height, 40.0);
        assert_eq!(results[1].y, 40.0);
        assert_eq!(results[1].height, 50.0);
    }

    #[test]
    fn test_center_justify() {
        let container = Size::new(200.0, 100.0);
        let flex = FlexLayout {
            justify: FlexJustify::Center,
            ..Default::default()
        };
        let children = vec![(Size::new(60.0, 30.0), FlexItem::default())];

        let results = flex_layout(container, &flex, &children, &Edges::ZERO);

        // Should be centered: (200 - 60) / 2 = 70
        assert!((results[0].x - 70.0).abs() < 0.1);
    }

    #[test]
    fn test_gap() {
        let container = Size::new(300.0, 100.0);
        let flex = FlexLayout {
            gap: 10.0,
            ..Default::default()
        };
        let children = vec![
            (Size::new(50.0, 30.0), FlexItem::default()),
            (Size::new(50.0, 30.0), FlexItem::default()),
            (Size::new(50.0, 30.0), FlexItem::default()),
        ];

        let results = flex_layout(container, &flex, &children, &Edges::ZERO);

        assert_eq!(results[0].x, 0.0);
        assert_eq!(results[1].x, 60.0); // 50 + 10 gap
        assert_eq!(results[2].x, 120.0); // 50 + 10 + 50 + 10
    }

    /// `n` items `w` by `h`, nothing else set.
    fn plain(n: usize, w: f32, h: f32) -> Vec<(Size, FlexItem)> {
        (0..n)
            .map(|_| (Size::new(w, h), FlexItem::default()))
            .collect()
    }

    fn xs(boxes: &[LayoutBox]) -> Vec<f32> {
        boxes.iter().map(|b| b.x).collect()
    }

    fn ys(boxes: &[LayoutBox]) -> Vec<f32> {
        boxes.iter().map(|b| b.y).collect()
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    /// **A wrapping row breaks where the next item would not fit**, a line
    /// at a time, the lines `line_gap` apart -- and a row that does not wrap
    /// keeps every item on one line, overflowing.
    #[test]
    fn a_wrapping_row_breaks_into_lines() {
        // Five 60-wide items in 200: three fit with two 10 gaps (200), the
        // fourth starts the next line.
        let flex = FlexLayout {
            wrap: FlexWrap::Wrap,
            gap: 10.0,
            line_gap: Some(5.0),
            align_content: AlignContent::Start,
            align_items: FlexAlign::Start,
            ..FlexLayout::default()
        };
        let boxes = flex_layout(
            Size::new(200.0, 300.0),
            &flex,
            &plain(5, 60.0, 20.0),
            &Edges::ZERO,
        );
        assert_eq!(xs(&boxes), [0.0, 70.0, 140.0, 0.0, 70.0]);
        assert_eq!(ys(&boxes), [0.0, 0.0, 0.0, 25.0, 25.0]);

        // `line_gap` unset is `gap`, as CSS's `gap` sets both.
        let both = FlexLayout {
            line_gap: None,
            ..flex.clone()
        };
        let boxes = flex_layout(
            Size::new(200.0, 300.0),
            &both,
            &plain(5, 60.0, 20.0),
            &Edges::ZERO,
        );
        assert_eq!(ys(&boxes)[3], 30.0);

        let one_line = flex_layout(
            Size::new(200.0, 300.0),
            &FlexLayout {
                wrap: FlexWrap::NoWrap,
                ..flex.clone()
            },
            &plain(5, 60.0, 20.0),
            &Edges::ZERO,
        );
        assert!(ys(&one_line).iter().all(|y| *y == 0.0));

        // A line always takes one item, however long it is.
        let boxes = flex_layout(
            Size::new(100.0, 300.0),
            &flex,
            &plain(2, 150.0, 20.0),
            &Edges::ZERO,
        );
        assert_eq!(xs(&boxes), [0.0, 0.0]);
        assert_eq!(ys(&boxes), [0.0, 25.0]);
    }

    /// **`WrapReverse` stacks the lines from the far side**: the first line
    /// at the bottom of the container.
    #[test]
    fn wrap_reverse_stacks_lines_from_the_far_side() {
        let flex = FlexLayout {
            wrap: FlexWrap::WrapReverse,
            align_content: AlignContent::Start,
            align_items: FlexAlign::Start,
            ..FlexLayout::default()
        };
        let boxes = flex_layout(
            Size::new(100.0, 200.0),
            &flex,
            &plain(3, 60.0, 20.0),
            &Edges::ZERO,
        );
        assert_eq!(ys(&boxes), [180.0, 160.0, 140.0]);
    }

    /// **The lines of a wrapping container share its spare height by
    /// `align_content`.** Two 20-high lines in 100: 60 spare.
    #[test]
    fn align_content_shares_the_spare_height_among_lines() {
        let lay = |align_content| {
            let flex = FlexLayout {
                wrap: FlexWrap::Wrap,
                align_content,
                align_items: FlexAlign::Start,
                ..FlexLayout::default()
            };
            let boxes = flex_layout(
                Size::new(100.0, 100.0),
                &flex,
                &plain(2, 60.0, 20.0),
                &Edges::ZERO,
            );
            (ys(&boxes), boxes[0].height)
        };
        assert_eq!(lay(AlignContent::Start).0, [0.0, 20.0]);
        assert_eq!(lay(AlignContent::End).0, [60.0, 80.0]);
        assert_eq!(lay(AlignContent::Center).0, [30.0, 50.0]);
        assert_eq!(lay(AlignContent::SpaceBetween).0, [0.0, 80.0]);
        assert_eq!(lay(AlignContent::SpaceAround).0, [15.0, 65.0]);
        assert_eq!(lay(AlignContent::SpaceEvenly).0, [20.0, 60.0]);
        // Stretch makes each line 50 high; a start-aligned item stays its
        // own height at the top of its line.
        assert_eq!(lay(AlignContent::Stretch), (vec![0.0, 50.0], 20.0));
    }

    /// **An item that reaches its `max` while growing stops there, and the
    /// rest share what it could not take** (CSS's freezing).
    #[test]
    fn growing_stops_at_max_and_the_rest_take_the_remainder() {
        let children = vec![
            (
                Size::new(50.0, 20.0),
                FlexItem {
                    grow: 1.0,
                    max: Size::new(80.0, f32::INFINITY),
                    ..FlexItem::default()
                },
            ),
            (
                Size::new(50.0, 20.0),
                FlexItem {
                    grow: 1.0,
                    ..FlexItem::default()
                },
            ),
        ];
        let boxes = flex_layout(
            Size::new(300.0, 50.0),
            &FlexLayout::default(),
            &children,
            &Edges::ZERO,
        );
        // Equal shares would be 150 each; the first stops at 80.
        assert_eq!(boxes[0].width, 80.0);
        assert_eq!(boxes[1].width, 220.0);
        assert_eq!(boxes[1].x, 80.0);
    }

    /// **Shrinking takes from each item in proportion to its base size
    /// times its `shrink`, and an item at its `min` gives no more.**
    #[test]
    fn shrinking_is_weighted_by_base_size_and_stops_at_min() {
        // 300 of items in 200: 100 to take, from a 200 and a 100 by 2:1.
        let boxes = flex_layout(
            Size::new(200.0, 50.0),
            &FlexLayout::default(),
            &[
                (Size::new(200.0, 20.0), FlexItem::default()),
                (Size::new(100.0, 20.0), FlexItem::default()),
            ],
            &Edges::ZERO,
        );
        assert!(close(boxes[0].width, 200.0 - 100.0 * 2.0 / 3.0));
        assert!(close(boxes[1].width, 100.0 - 100.0 / 3.0));

        // The 100 cannot go below 90: it gives 10, the 200 gives 90.
        let boxes = flex_layout(
            Size::new(200.0, 50.0),
            &FlexLayout::default(),
            &[
                (Size::new(200.0, 20.0), FlexItem::default()),
                (
                    Size::new(100.0, 20.0),
                    FlexItem {
                        min: Size::new(90.0, 0.0),
                        ..FlexItem::default()
                    },
                ),
            ],
            &Edges::ZERO,
        );
        assert!(close(boxes[1].width, 90.0));
        assert!(close(boxes[0].width, 110.0));

        // `shrink` 0 gives nothing: the other gives all of it.
        let boxes = flex_layout(
            Size::new(200.0, 50.0),
            &FlexLayout::default(),
            &[
                (
                    Size::new(150.0, 20.0),
                    FlexItem {
                        shrink: 0.0,
                        ..FlexItem::default()
                    },
                ),
                (Size::new(150.0, 20.0), FlexItem::default()),
            ],
            &Edges::ZERO,
        );
        assert_eq!(boxes[0].width, 150.0);
        assert!(close(boxes[1].width, 50.0));
    }

    /// **Grow factors that sum to less than one take only that share of the
    /// free space** (§9.7): two items of 0.25 leave half of it unused.
    #[test]
    fn fractional_grow_factors_take_a_fraction_of_the_space() {
        let quarter = FlexItem {
            grow: 0.25,
            ..FlexItem::default()
        };
        let boxes = flex_layout(
            Size::new(300.0, 50.0),
            &FlexLayout::default(),
            &[
                (Size::new(50.0, 20.0), quarter.clone()),
                (Size::new(50.0, 20.0), quarter),
            ],
            &Edges::ZERO,
        );
        // 200 free; half of it, 100, shared equally.
        assert!(close(boxes[0].width, 100.0));
        assert!(close(boxes[1].width, 100.0));
    }

    /// **A container with no end along its main axis has no free space**:
    /// an item that grows stays its own size rather than becoming infinite,
    /// and nothing wraps.
    #[test]
    fn an_unbounded_container_grows_nothing() {
        let children = vec![(
            Size::new(50.0, 20.0),
            FlexItem {
                grow: 1.0,
                ..FlexItem::default()
            },
        )];
        let boxes = flex_layout(
            Size::new(f32::INFINITY, f32::INFINITY),
            &FlexLayout {
                wrap: FlexWrap::Wrap,
                justify: FlexJustify::Center,
                ..FlexLayout::default()
            },
            &children,
            &Edges::ZERO,
        );
        assert_eq!(boxes[0].width, 50.0);
        assert_eq!(boxes[0].x, 0.0);
        assert!(boxes[0].height.is_finite());
    }

    /// **Margins keep their room**: the next item starts past them, the
    /// box's corner is its margin box's, and its border box is where
    /// `content_x` says -- on the side each margin was set on, in a reversed
    /// row too.
    #[test]
    fn margins_keep_their_room_on_their_own_side() {
        let margin = Edges {
            top: 2.0,
            right: 7.0,
            bottom: 0.0,
            left: 5.0,
        };
        let children = vec![
            (
                Size::new(50.0, 20.0),
                FlexItem {
                    margin,
                    ..FlexItem::default()
                },
            ),
            (Size::new(50.0, 20.0), FlexItem::default()),
        ];
        let flex = FlexLayout {
            align_items: FlexAlign::Start,
            ..FlexLayout::default()
        };
        let boxes = flex_layout(Size::new(300.0, 50.0), &flex, &children, &Edges::ZERO);
        assert_eq!((boxes[0].x, boxes[0].y), (0.0, 0.0));
        assert_eq!((boxes[0].content_x(), boxes[0].content_y()), (5.0, 2.0));
        assert_eq!(boxes[0].outer_width(), 62.0);
        assert_eq!(boxes[1].x, 62.0);

        let reversed = flex_layout(
            Size::new(300.0, 50.0),
            &FlexLayout {
                direction: FlexDirection::RowReverse,
                ..flex
            },
            &children,
            &Edges::ZERO,
        );
        // The first item ends 7 short of the right edge -- its right margin
        // is still on its right -- and its border box starts 5 into its
        // margin box.
        assert_eq!(reversed[0].content_x() + 50.0, 293.0);
        assert_eq!(reversed[0].x, 238.0);
        assert_eq!(reversed[1].x + 50.0, 238.0);
    }

    /// **`Baseline` lines the items' text up in a row**: the line is as tall
    /// as it needs to be to do it, and an item with no baseline offers its
    /// bottom edge.
    #[test]
    fn baseline_lines_text_up_in_a_row() {
        let at = |baseline| FlexItem {
            align_self: Some(FlexAlign::Baseline),
            baseline,
            ..FlexItem::default()
        };
        let boxes = flex_layout(
            Size::new(300.0, f32::INFINITY),
            &FlexLayout {
                wrap: FlexWrap::Wrap,
                align_content: AlignContent::Start,
                ..FlexLayout::default()
            },
            &[
                (Size::new(40.0, 30.0), at(Some(20.0))),
                (Size::new(40.0, 16.0), at(Some(12.0))),
                (Size::new(40.0, 10.0), at(None)),
            ],
            &Edges::ZERO,
        );
        // Baselines at 20 below each top: the second sits 8 down, the third
        // (its bottom as baseline) 10 down.
        assert_eq!(ys(&boxes), [0.0, 8.0, 10.0]);
        // A column has no baselines to line up: they start.
        let column = flex_layout(
            Size::new(100.0, 300.0),
            &FlexLayout {
                direction: FlexDirection::Column,
                ..FlexLayout::default()
            },
            &[(Size::new(40.0, 30.0), at(Some(20.0)))],
            &Edges::ZERO,
        );
        assert_eq!(column[0].x, 0.0);
    }

    /// **Stretch makes an item as thick as its line, but no thicker than
    /// its `max`.**
    #[test]
    fn stretch_stops_at_max() {
        let boxes = flex_layout(
            Size::new(300.0, 100.0),
            &FlexLayout::default(),
            &[
                (Size::new(50.0, 20.0), FlexItem::default()),
                (
                    Size::new(50.0, 20.0),
                    FlexItem {
                        max: Size::new(f32::INFINITY, 40.0),
                        ..FlexItem::default()
                    },
                ),
            ],
            &Edges::ZERO,
        );
        assert_eq!(boxes[0].height, 100.0);
        assert_eq!(boxes[1].height, 40.0);
    }

    /// **A reversed row mirrors `justify`**: its start is the right edge,
    /// so `Start` packs right and `End` packs left, as CSS's main-start
    /// being swapped means.
    #[test]
    fn a_reversed_row_mirrors_justify() {
        let lay = |justify| {
            xs(&flex_layout(
                Size::new(300.0, 50.0),
                &FlexLayout {
                    direction: FlexDirection::RowReverse,
                    justify,
                    ..FlexLayout::default()
                },
                &plain(2, 50.0, 20.0),
                &Edges::ZERO,
            ))
        };
        assert_eq!(lay(FlexJustify::Start), [250.0, 200.0]);
        assert_eq!(lay(FlexJustify::End), [50.0, 0.0]);
        assert_eq!(lay(FlexJustify::Center), [150.0, 100.0]);
    }

    /// **A line too long falls back as CSS's do**: `SpaceBetween` from the
    /// start, `SpaceAround` and `SpaceEvenly` from the middle, overflowing
    /// both ends.
    #[test]
    fn an_overflowing_line_falls_back() {
        let lay = |justify| {
            xs(&flex_layout(
                Size::new(100.0, 50.0),
                &FlexLayout {
                    justify,
                    ..FlexLayout::default()
                },
                &[
                    (
                        Size::new(80.0, 20.0),
                        FlexItem {
                            shrink: 0.0,
                            ..FlexItem::default()
                        },
                    ),
                    (
                        Size::new(80.0, 20.0),
                        FlexItem {
                            shrink: 0.0,
                            ..FlexItem::default()
                        },
                    ),
                ],
                &Edges::ZERO,
            ))
        };
        assert_eq!(lay(FlexJustify::SpaceBetween), [0.0, 80.0]);
        assert_eq!(lay(FlexJustify::SpaceAround), [-30.0, 50.0]);
        assert_eq!(lay(FlexJustify::SpaceEvenly), [-30.0, 50.0]);
    }

    /// **The container's padding moves everything in**, and the room the
    /// items share is inside it.
    #[test]
    fn padding_moves_everything_in() {
        let padding = Edges {
            top: 4.0,
            right: 10.0,
            bottom: 4.0,
            left: 10.0,
        };
        let boxes = flex_layout(
            Size::new(120.0, 50.0),
            &FlexLayout::default(),
            &[(
                Size::new(10.0, 10.0),
                FlexItem {
                    grow: 1.0,
                    ..FlexItem::default()
                },
            )],
            &padding,
        );
        assert_eq!((boxes[0].x, boxes[0].y), (10.0, 4.0));
        assert_eq!((boxes[0].width, boxes[0].height), (100.0, 42.0));
    }

    /// **A minimum above its maximum does not panic**: the minimum wins, as
    /// CSS has it.
    #[test]
    fn a_minimum_above_its_maximum_wins_rather_than_panicking() {
        let constraint = SizeConstraint {
            min_width: 100.0,
            max_width: 50.0,
            min_height: 0.0,
            max_height: f32::INFINITY,
        };
        assert_eq!(
            constraint.constrain(Size::new(70.0, 20.0)),
            Size::new(100.0, 20.0)
        );
        let boxes = flex_layout(
            Size::new(300.0, 50.0),
            &FlexLayout::default(),
            &[(
                Size::new(70.0, 20.0),
                FlexItem {
                    min: Size::new(100.0, 0.0),
                    max: Size::new(50.0, f32::INFINITY),
                    ..FlexItem::default()
                },
            )],
            &Edges::ZERO,
        );
        assert_eq!(boxes[0].width, 100.0);
    }
}
