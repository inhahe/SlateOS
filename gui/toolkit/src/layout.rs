//! Flexbox-inspired layout engine.
//!
//! Computes widget positions and sizes based on flex properties.
//! Each container widget specifies direction, wrapping, and alignment.
//! Children specify flex-grow, flex-shrink, and basis.
//!
//! This is a simplified Flexbox implementation suitable for desktop UIs.
//! It handles the common cases: rows, columns, spacing, alignment,
//! and grow/shrink behavior.

use crate::style::Edges;

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

/// Layout properties for a flex container.
#[derive(Clone, Debug)]
pub struct FlexLayout {
    pub direction: FlexDirection,
    pub wrap: FlexWrap,
    pub justify: FlexJustify,
    pub align_items: FlexAlign,
    pub align_content: FlexAlign,
    pub gap: f32,
}

impl Default for FlexLayout {
    fn default() -> Self {
        Self {
            direction: FlexDirection::Row,
            wrap: FlexWrap::NoWrap,
            justify: FlexJustify::Start,
            align_items: FlexAlign::Stretch,
            align_content: FlexAlign::Stretch,
            gap: 0.0,
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
}

impl Default for FlexItem {
    fn default() -> Self {
        Self {
            grow: 0.0,
            shrink: 1.0,
            basis: None,
            align_self: None,
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

    pub fn constrain(&self, size: Size) -> Size {
        Size {
            width: size.width.clamp(self.min_width, self.max_width),
            height: size.height.clamp(self.min_height, self.max_height),
        }
    }
}

/// Perform flex layout on a list of child sizes within a container.
///
/// Returns computed positions and sizes for each child along the main axis.
/// This is the core layout algorithm.
pub fn flex_layout(
    container_size: Size,
    flex: &FlexLayout,
    children: &[(Size, FlexItem)],
    padding: &Edges,
) -> Vec<LayoutBox> {
    if children.is_empty() {
        return Vec::new();
    }

    let available_main = match flex.direction.main_axis() {
        Axis::Horizontal => container_size.width - padding.horizontal(),
        Axis::Vertical => container_size.height - padding.vertical(),
    };
    let available_cross = match flex.direction.main_axis() {
        Axis::Horizontal => container_size.height - padding.vertical(),
        Axis::Vertical => container_size.width - padding.horizontal(),
    };

    // Calculate initial sizes
    let mut items: Vec<FlexLayoutItem> = children
        .iter()
        .map(|(size, item)| {
            let main_size = item.basis.unwrap_or(match flex.direction.main_axis() {
                Axis::Horizontal => size.width,
                Axis::Vertical => size.height,
            });
            let cross_size = match flex.direction.main_axis() {
                Axis::Horizontal => size.height,
                Axis::Vertical => size.width,
            };
            FlexLayoutItem {
                main_size,
                cross_size,
                grow: item.grow,
                shrink: item.shrink,
                align_self: item.align_self,
                computed_main: main_size,
                computed_cross: cross_size,
                computed_main_pos: 0.0,
                computed_cross_pos: 0.0,
            }
        })
        .collect();

    // Total gap space
    let total_gap = if children.len() > 1 {
        flex.gap * (children.len() - 1) as f32
    } else {
        0.0
    };

    // Calculate total main size
    let total_main: f32 = items.iter().map(|item| item.main_size).sum::<f32>() + total_gap;

    // Distribute remaining space (grow/shrink)
    let free_space = available_main - total_main;

    if free_space > 0.0 {
        // Grow
        let total_grow: f32 = items.iter().map(|item| item.grow).sum();
        if total_grow > 0.0 {
            for item in &mut items {
                if item.grow > 0.0 {
                    item.computed_main += free_space * (item.grow / total_grow);
                }
            }
        }
    } else if free_space < 0.0 {
        // Shrink
        let total_shrink: f32 = items
            .iter()
            .map(|item| item.shrink * item.main_size)
            .sum();
        if total_shrink > 0.0 {
            let overflow = -free_space;
            for item in &mut items {
                let shrink_factor = item.shrink * item.main_size;
                item.computed_main -= overflow * (shrink_factor / total_shrink);
                if item.computed_main < 0.0 {
                    item.computed_main = 0.0;
                }
            }
        }
    }

    // Position items along main axis
    let computed_total: f32 = items.iter().map(|i| i.computed_main).sum::<f32>() + total_gap;
    let remaining = (available_main - computed_total).max(0.0);

    let mut main_pos = match flex.justify {
        FlexJustify::Start => 0.0,
        FlexJustify::End => remaining,
        FlexJustify::Center => remaining / 2.0,
        FlexJustify::SpaceBetween => 0.0,
        FlexJustify::SpaceAround => {
            if !items.is_empty() {
                remaining / (items.len() as f32 * 2.0)
            } else {
                0.0
            }
        }
        FlexJustify::SpaceEvenly => {
            remaining / (items.len() as f32 + 1.0)
        }
    };

    let between_space = match flex.justify {
        FlexJustify::SpaceBetween
            if items.len() > 1 => {
                remaining / (items.len() - 1) as f32
            }
        FlexJustify::SpaceAround
            if !items.is_empty() => {
                remaining / items.len() as f32
            }
        FlexJustify::SpaceEvenly => remaining / (items.len() as f32 + 1.0),
        _ => 0.0,
    };

    let item_count = items.len();

    if flex.direction.is_reversed() {
        // Reverse: start from end
        main_pos = available_main;
        for (i, item) in items.iter_mut().enumerate() {
            main_pos -= item.computed_main;
            item.computed_main_pos = main_pos;
            if i < item_count - 1 {
                main_pos -= flex.gap + between_space;
            }
        }
    } else {
        for (i, item) in items.iter_mut().enumerate() {
            item.computed_main_pos = main_pos;
            main_pos += item.computed_main;
            if i < item_count - 1 {
                main_pos += flex.gap + between_space;
            }
        }
    }

    // Align items along cross axis
    for item in &mut items {
        let align = item
            .align_self
            .unwrap_or(flex.align_items);

        match align {
            FlexAlign::Start => {
                item.computed_cross_pos = 0.0;
            }
            FlexAlign::End => {
                item.computed_cross_pos = available_cross - item.computed_cross;
            }
            FlexAlign::Center => {
                item.computed_cross_pos = (available_cross - item.computed_cross) / 2.0;
            }
            FlexAlign::Stretch => {
                item.computed_cross_pos = 0.0;
                item.computed_cross = available_cross;
            }
            FlexAlign::Baseline => {
                // Simplified: treat as start
                item.computed_cross_pos = 0.0;
            }
        }
    }

    // Convert to LayoutBox results
    items
        .iter()
        .map(|item| {
            let (x, y, width, height) = match flex.direction.main_axis() {
                Axis::Horizontal => (
                    item.computed_main_pos + padding.left,
                    item.computed_cross_pos + padding.top,
                    item.computed_main,
                    item.computed_cross,
                ),
                Axis::Vertical => (
                    item.computed_cross_pos + padding.left,
                    item.computed_main_pos + padding.top,
                    item.computed_cross,
                    item.computed_main,
                ),
            };

            LayoutBox {
                x,
                y,
                width,
                height,
                padding: Edges::ZERO,
                border_widths: Edges::ZERO,
                margin: Edges::ZERO,
            }
        })
        .collect()
}

/// Internal struct for layout computation.
#[allow(dead_code)]
struct FlexLayoutItem {
    main_size: f32,
    cross_size: f32,
    grow: f32,
    shrink: f32,
    align_self: Option<FlexAlign>,
    computed_main: f32,
    computed_cross: f32,
    computed_main_pos: f32,
    computed_cross_pos: f32,
}

#[cfg(test)]
mod tests {
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
}
