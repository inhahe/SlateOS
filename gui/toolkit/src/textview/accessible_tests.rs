//! Tests for the text views as tools see them: each a document holding its
//! text, scrolled as the wheel scrolls it; a rich view's blocks as what they
//! are, shown only while in sight, and its links, named by their words,
//! holding where they go, pressed as clicked -- scrolled into sight first.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::*;
use crate::textview::{HeadingLevel, ListKind, RichFontWeight, RichSpanStyle, RichTextViewConfig};

/// A plain view 300 by 100 holding `lines` lines, "line 0" on.
fn plain(lines: usize) -> SimpleTextView {
    let mut view = SimpleTextView::new(300.0, 100.0);
    view.set_text(
        &(0..lines)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    view
}

fn scroll(view: &mut SimpleTextView, y: f32) -> Result<Option<core::convert::Infallible>, Refusal> {
    view.invoke(
        &TextViewPart::View,
        Action::ScrollTo { x: 0.0, y },
        0.0,
        0.0,
    )
}

/// A paragraph of `spans`.
fn paragraph(spans: Vec<RichSpan>) -> RichBlock {
    RichBlock::Paragraph {
        spans,
        spacing_above: 0.0,
        spacing_below: 0.0,
    }
}

/// `count` paragraphs, "para 0" on: taller than a view 100 high.
fn paragraphs(count: usize) -> Vec<RichBlock> {
    (0..count)
        .map(|n| paragraph(vec![RichSpan::plain(format!("para {n}"))]))
        .collect()
}

fn act(
    view: &mut RichTextView,
    part: RichTextPart,
    action: Action,
) -> Result<Option<RichTextEvent>, Refusal> {
    view.invoke(&part, action, 0.0, 0.0)
}

fn node(view: &RichTextView, part: RichTextPart) -> Node<RichTextPart> {
    view.automation(0.0, 0.0)
        .walk()
        .find(|n| n.id == part)
        .unwrap_or_else(|| panic!("no {part:?}"))
        .clone()
}

/// **A plain view is a document holding its text** -- every line, as it is
/// shown, its colours' escapes gone -- where its host draws it, and says it
/// takes no keyboard of its own.
#[test]
fn a_plain_view_is_a_document_holding_its_text() {
    let mut view = plain(3);
    let root = view.automation(0.0, 0.0);
    assert_eq!((root.role, root.name.as_str()), (Role::Document, "Text"));
    assert_eq!(
        root.value,
        Some(Value::Text("line 0\nline 1\nline 2".to_owned()))
    );
    assert_eq!(root.bounds, Rect::new(0.0, 0.0, 300.0, 100.0));
    assert!(!root.focusable && root.children.is_empty());

    view.set_text("\u{1b}[31mred\u{1b}[0m and plain");
    assert_eq!(
        view.automation(0.0, 0.0).value,
        Some(Value::Text("red and plain".to_owned()))
    );
}

/// **A plain view is scrolled as the wheel scrolls it, by whole lines** --
/// to the line the height asked for falls on, as far as the text goes;
/// a height that is no number is refused, and nothing else is a tool's to
/// do.
#[test]
fn a_plain_view_scrolls_by_whole_lines_as_far_as_it_goes() {
    let mut view = plain(50);
    let line = view.config.line_height;
    assert_eq!(scroll(&mut view, line * 3.5), Ok(None));
    assert_eq!(view.scroll_offset, 3);
    assert_eq!(scroll(&mut view, 1.0e9), Ok(None));
    assert!(view.is_at_bottom() && view.scroll_offset > 3);
    assert_eq!(scroll(&mut view, -40.0), Ok(None));
    assert_eq!(view.scroll_offset, 0);

    assert_eq!(scroll(&mut view, line * 2.0), Ok(None));
    assert_eq!(scroll(&mut view, f32::NAN), Err(Refusal::NotANumber));
    assert_eq!(
        view.invoke(
            &TextViewPart::View,
            Action::ScrollTo {
                x: f32::INFINITY,
                y: 0.0
            },
            0.0,
            0.0
        ),
        Err(Refusal::NotANumber)
    );
    assert_eq!(view.scroll_offset, 2, "a refusal moves nothing");
    assert_eq!(
        view.invoke(&TextViewPart::View, Action::Press, 0.0, 0.0),
        Err(Refusal::NotApplicable {
            role: Role::Document,
            action: "press"
        })
    );
}

/// A rich view 400 by 600 holding one block of each kind, the paragraph
/// with a link in it.
fn rich() -> RichTextView {
    let mut view = RichTextView::new(400.0, 600.0);
    view.set_blocks(vec![
        RichBlock::Heading {
            level: HeadingLevel::H1,
            spans: vec![RichSpan::plain("Intro")],
        },
        paragraph(vec![
            RichSpan::plain("See "),
            RichSpan::link("the docs", "https://docs.example"),
            RichSpan::plain(" now."),
        ]),
        RichBlock::ListItem {
            kind: ListKind::Bullet,
            index: 1,
            indent_level: 0,
            spans: vec![RichSpan::plain("one")],
        },
        RichBlock::CodeBlock {
            code: "let x = 1;".to_owned(),
            language: None,
        },
        RichBlock::HorizontalRule,
        RichBlock::ImagePlaceholder {
            width: 40.0,
            height: 30.0,
            alt_text: "A logo".to_owned(),
        },
    ]);
    view
}

/// **A rich view is a document holding its text as copying all of it gives
/// it, and under it each block as what it is**, in order down the view,
/// each named by its words -- a picture by the words that stand for it.
#[test]
fn a_rich_view_shows_its_blocks_as_what_they_are() {
    let view = rich();
    let root = view.automation(0.0, 0.0);
    assert_eq!(
        (root.role, root.name.as_str()),
        (Role::Document, "Document")
    );
    assert_eq!(root.value, Some(Value::Text(view.plain_text())));
    let blocks: Vec<(Role, &str)> = root
        .children
        .iter()
        .map(|n| (n.role, n.name.as_str()))
        .collect();
    assert_eq!(
        blocks,
        [
            (Role::Heading, "Intro"),
            (Role::Paragraph, "See the docs now."),
            (Role::ListItem, "one"),
            (Role::Paragraph, "let x = 1;"),
            (Role::Separator, ""),
            (Role::Image, "A logo"),
        ]
    );
    assert!(root.children.iter().all(|n| n.shown));
    for pair in root.children.windows(2) {
        assert!(
            pair[0].bounds.bottom() <= pair[1].bounds.y + 0.01,
            "{:?} above {:?}",
            pair[0].bounds,
            pair[1].bounds
        );
    }
}

/// **A link is named by its words and holds where it goes, in its block,
/// where its words are drawn** -- and a run of spans side by side linking
/// to one place, one bold, is one link.
#[test]
fn a_link_is_named_by_its_words_and_holds_where_it_goes() {
    let view = rich();
    let block = node(&view, RichTextPart::Block(1));
    assert_eq!(block.children.len(), 1, "one link in the paragraph");
    let link = &block.children[0];
    assert_eq!(link.id, RichTextPart::Link { block: 1, span: 1 });
    assert_eq!((link.role, link.name.as_str()), (Role::Link, "the docs"));
    assert_eq!(
        link.value,
        Some(Value::Text("https://docs.example".to_owned()))
    );
    assert!(link.shown);
    assert!(
        link.bounds.x > block.bounds.x
            && link.bounds.right() <= block.bounds.right()
            && link.bounds.y >= block.bounds.y
            && link.bounds.bottom() <= block.bounds.bottom() + 0.01,
        "{:?} after `See ` in {:?}",
        link.bounds,
        block.bounds
    );
    let words = view.span_width(&RichSpan::plain("the docs"), None);
    assert!(
        (link.bounds.w - words).abs() < 0.5,
        "as wide as its words, {words}, not to the line's end: {:?}",
        link.bounds
    );

    let mut runs = RichTextView::new(400.0, 200.0);
    let bold = RichSpanStyle {
        weight: RichFontWeight::Bold,
        link: Some("one".to_owned()),
        ..RichSpanStyle::default()
    };
    runs.set_blocks(vec![paragraph(vec![
        RichSpan::link("click ", "one"),
        RichSpan::styled("here", bold),
        RichSpan::link("elsewhere", "two"),
    ])]);
    let links: Vec<(RichTextPart, String, Option<Value>)> = node(&runs, RichTextPart::Block(0))
        .children
        .into_iter()
        .map(|n| (n.id, n.name, n.value))
        .collect();
    assert_eq!(
        links,
        [
            (
                RichTextPart::Link { block: 0, span: 0 },
                "click here".to_owned(),
                Some(Value::Text("one".to_owned()))
            ),
            (
                RichTextPart::Link { block: 0, span: 2 },
                "elsewhere".to_owned(),
                Some(Value::Text("two".to_owned()))
            ),
        ]
    );
}

/// **A block scrolled out of sight is not shown**, and is once the view is
/// scrolled to it, as the wheel scrolls -- as far as the text goes; a
/// height that is no number is refused.
#[test]
fn a_block_scrolled_out_of_sight_is_not_shown() {
    let mut view = RichTextView::new(400.0, 100.0);
    view.set_blocks(paragraphs(30));
    let root = view.automation(0.0, 0.0);
    assert!(root.children[0].shown && !root.children[29].shown);
    assert!(root.children[29].bounds.y >= 100.0, "below the view");

    let to = RichTextPart::View;
    assert_eq!(
        act(&mut view, to, Action::ScrollTo { x: 0.0, y: 1.0e9 }),
        Ok(None)
    );
    let root = view.automation(0.0, 0.0);
    assert!(!root.children[0].shown && root.children[29].shown);
    assert!(root.children[29].bounds.bottom() <= 100.0 + 0.01);
    assert_eq!(
        act(
            &mut view,
            to,
            Action::ScrollTo {
                x: 0.0,
                y: f32::NAN
            }
        ),
        Err(Refusal::NotANumber)
    );
    assert_eq!(
        act(
            &mut view,
            to,
            Action::ScrollTo {
                x: f32::INFINITY,
                y: 0.0
            }
        ),
        Err(Refusal::NotANumber),
        "across, too"
    );
    assert!(node(&view, RichTextPart::Block(29)).shown, "nothing moved");

    // To the height asked for, exactly: the third block at the top.
    let third =
        node(&view, RichTextPart::Block(2)).bounds.y - node(&view, RichTextPart::Block(0)).bounds.y;
    assert_eq!(
        act(&mut view, to, Action::ScrollTo { x: 0.0, y: third }),
        Ok(None)
    );
    let top = node(&view, RichTextPart::Block(2)).bounds.y;
    assert!(top.abs() < 0.01, "the third block at {top}");
}

/// **A link pressed is the view's own click on it, scrolled into sight
/// first** -- by as little as shows it, as the user scrolls down to it: it
/// answers what the click does, for the host to follow, and selects
/// nothing.
#[test]
fn a_link_pressed_is_clicked_scrolled_into_sight_first() {
    let mut view = RichTextView::new(400.0, 100.0);
    let mut blocks = paragraphs(30);
    blocks.push(paragraph(vec![
        RichSpan::plain("Read "),
        RichSpan::link("more", "https://more.example"),
    ]));
    blocks.extend(paragraphs(30));
    view.set_blocks(blocks);
    let link = RichTextPart::Link { block: 30, span: 1 };
    assert!(!node(&view, link).shown, "out of sight below");

    assert_eq!(
        act(&mut view, link, Action::Press),
        Ok(Some(RichTextEvent::LinkClicked(
            "https://more.example".to_owned()
        )))
    );
    let shown = node(&view, link);
    assert!(
        shown.shown && (shown.bounds.bottom() - 100.0).abs() < 0.01,
        "at the view's foot, scrolled no further: {:?}",
        shown.bounds
    );
    assert_eq!(view.selection, None, "a link's click selects nothing");
    assert!(!view.dragging, "and is let go of");
}

/// **A link is followed in a view whose text cannot be selected**: not
/// being able to select the words is no reason the link in them goes
/// nowhere.
#[test]
fn a_link_is_followed_where_the_text_cannot_be_selected() {
    let mut view = RichTextView::with_config(
        400.0,
        200.0,
        RichTextViewConfig {
            selectable: false,
            ..RichTextViewConfig::default()
        },
    );
    view.set_blocks(vec![paragraph(vec![RichSpan::link("Home", "home")])]);
    assert_eq!(
        act(
            &mut view,
            RichTextPart::Link { block: 0, span: 0 },
            Action::Press
        ),
        Ok(Some(RichTextEvent::LinkClicked("home".to_owned())))
    );
}

/// **A link a click could not reach is refused, and nothing moves**: in a
/// view too short to show it whole, its middle is outside the view.
#[test]
fn a_link_the_view_cannot_show_is_refused() {
    let mut view = RichTextView::new(400.0, 4.0);
    let mut blocks = paragraphs(3);
    blocks.push(paragraph(vec![RichSpan::link("far", "far")]));
    view.set_blocks(blocks);
    assert_eq!(
        act(
            &mut view,
            RichTextPart::Link { block: 3, span: 0 },
            Action::Press
        ),
        Err(Refusal::Hidden)
    );
    assert_eq!(view.scroll_offset_px, 0.0, "not scrolled");
}

/// **What is not a part's to do is refused** -- a block is only read, a
/// link only pressed -- and what is no part is none.
#[test]
fn what_a_part_cannot_do_is_refused() {
    let mut view = rich();
    let refused = |role, action| Err(Refusal::NotApplicable { role, action });
    assert_eq!(
        act(&mut view, RichTextPart::Block(1), Action::Press),
        refused(Role::Paragraph, "press")
    );
    assert_eq!(
        act(&mut view, RichTextPart::Block(0), Action::Focus),
        refused(Role::Heading, "focus")
    );
    assert_eq!(
        act(
            &mut view,
            RichTextPart::Link { block: 1, span: 1 },
            Action::Toggle
        ),
        refused(Role::Link, "toggle")
    );
    assert_eq!(
        act(&mut view, RichTextPart::View, Action::Press),
        refused(Role::Document, "press")
    );
    for part in [
        RichTextPart::Block(99),
        RichTextPart::Link { block: 1, span: 0 },
        RichTextPart::Link { block: 99, span: 0 },
    ] {
        assert_eq!(
            act(&mut view, part, Action::Press),
            Err(Refusal::NoSuchWidget),
            "{part:?}"
        );
    }
    assert_eq!(
        act(
            &mut view,
            RichTextPart::Link { block: 1, span: 0 },
            Action::Toggle
        ),
        Err(Refusal::NoSuchWidget),
        "a plain span is no link"
    );

    // A link with no words is drawn nowhere, and is none.
    let mut empty = RichTextView::new(400.0, 200.0);
    empty.set_blocks(vec![paragraph(vec![
        RichSpan::plain("a"),
        RichSpan::link("", "nowhere"),
    ])]);
    assert!(node(&empty, RichTextPart::Block(0)).children.is_empty());
    assert_eq!(
        act(
            &mut empty,
            RichTextPart::Link { block: 0, span: 1 },
            Action::Press
        ),
        Err(Refusal::NoSuchWidget)
    );
}

/// **A view whose rows are stale is seen as it will be drawn**: narrowed
/// with its rows not yet worked out again -- as whatever changes a view
/// leaves them until it is next drawn -- its words wrap onto more rows, and
/// its blocks' boxes say so; and seeing it lays nothing out.
#[test]
fn a_view_is_seen_as_it_will_be_drawn() {
    let mut view = RichTextView::new(400.0, 600.0);
    view.set_blocks(vec![paragraph(vec![RichSpan::plain(
        "a few words that wrap when the view narrows",
    )])]);
    let wide = node(&view, RichTextPart::Block(0)).bounds;
    view.width = 80.0;
    view.layout_dirty = true;
    let narrow = node(&view, RichTextPart::Block(0)).bounds;
    assert!(narrow.h > wide.h, "{narrow:?} against {wide:?}");
    assert_eq!(narrow.w, 80.0);
    assert!(view.layout_dirty, "seeing it lays nothing out");
}
