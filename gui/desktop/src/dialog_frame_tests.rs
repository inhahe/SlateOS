//! Tests for the frame the shell's dialogs wear: where its parts go, and
//! what it draws, under the built-in frame and under themes that change
//! each part.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]

use super::*;
use appearance::decorations::{ButtonShape, ButtonSide, TitleAlign};

/// A frame of `style`, in the default settings otherwise, at scale 1.
fn frame(style: DecorationStyle) -> DialogFrame {
    let mut settings = AppearanceSettings::default();
    settings.decoration_theme = appearance::themes::DecorationTheme::from_style("test", style);
    DialogFrame::from_settings(&settings, 1.0)
}

const OUTER: Rect = Rect {
    x: 100.0,
    y: 50.0,
    w: 450.0,
    h: 200.0,
};

/// The palette the frames here are drawn in.
fn palette() -> Palette {
    Palette::for_mode(false)
}

/// **The built-in frame is a window's**: a 30-pixel bar inside a 1-pixel
/// border, the content under it, and the close button -- 20 square --
/// at the bar's right end, centred down it, 4 from the end.
#[test]
fn the_built_in_frame_is_a_windows() {
    let f = frame(DecorationStyle::AERO);
    let layout = f.layout(OUTER);
    assert_eq!(layout.title_bar(), Rect::new(101.0, 51.0, 448.0, 30.0));
    assert_eq!(layout.content, Rect::new(101.0, 81.0, 448.0, 168.0));
    let close = layout.close().expect("a dialog has a close button");
    assert_eq!(close, Rect::new(101.0 + 448.0 - 24.0, 56.0, 20.0, 20.0));
    assert!(layout.is_close(close.x + 10.0, close.y + 10.0));
    assert!(!layout.is_close(close.x - 1.0, close.y + 10.0));
    // A dialog's content of 448 by 168 needs that whole outside.
    assert_eq!(f.outer_size(448.0, 168.0), (450.0, 200.0));
}

/// **A theme's frame moves the parts**: a taller bar, no border, the close
/// button at the left end.
#[test]
fn a_themes_frame_moves_the_parts() {
    let f = frame(DecorationStyle {
        title_height: 40,
        border: 0,
        button_side: ButtonSide::Left,
        ..DecorationStyle::AERO
    });
    let layout = f.layout(OUTER);
    assert_eq!(layout.title_bar(), Rect::new(100.0, 50.0, 450.0, 40.0));
    assert_eq!(layout.content, Rect::new(100.0, 90.0, 450.0, 160.0));
    assert_eq!(layout.close().unwrap().x, 104.0);
    assert_eq!(f.outer_size(450.0, 160.0), (450.0, 200.0));
}

/// The commands the frame draws for `style`, titled `title`.
fn drawn(style: DecorationStyle, title: &str, lit: bool) -> Vec<RenderCommand> {
    let f = frame(style);
    f.render(&f.layout(OUTER), title, &palette(), lit)
}

/// **The built-in frame draws the run box's shadow, the body, a bar in the
/// frame's colour, the border and the title at the bar's left**, the title
/// regular, and the close button's face in the close colour, rounded as the
/// windows are.
#[test]
fn the_built_in_frame_draws_a_windows_frame() {
    let settings = AppearanceSettings::default();
    let colors = DecorationColors::from_palette(&palette());
    let cmds = drawn(DecorationStyle::AERO, "Run", false);
    match &cmds[0] {
        RenderCommand::BoxShadow {
            offset_y,
            blur,
            spread,
            ..
        } => assert_eq!((*offset_y, *blur, *spread), (4.0, 16.0, 2.0)),
        other => panic!("first is not the shadow: {other:?}"),
    }
    assert!(cmds.iter().any(|c| matches!(c,
        RenderCommand::FillRect { color, width, .. } if *color == palette().base && *width == OUTER.w)));
    assert!(cmds.iter().any(|c| matches!(c,
        RenderCommand::FillRect { color, height, .. }
            if *color == colors.title_focused_bg && *height == 30.0)));
    assert!(cmds.iter().any(|c| matches!(c,
        RenderCommand::StrokeRect { line_width, color, .. }
            if *line_width == 1.0 && *color == colors.border_focused)));
    let (x, weight) = cmds
        .iter()
        .find_map(|c| match c {
            RenderCommand::Text {
                x,
                text,
                font_weight,
                ..
            } if text == "Run" => Some((*x, *font_weight)),
            _ => None,
        })
        .expect("no title");
    assert_eq!(x, 101.0 + DecorationStyle::TITLE_INSET);
    assert_eq!(weight, FontWeightHint::Regular);
    let radius = settings.window_corners.radius().min(10.0);
    assert!(cmds.iter().any(|c| matches!(c,
        RenderCommand::FillRect { color, width, corner_radii, .. }
            if *color == colors.close_button && *width == 20.0
                && *corner_radii == CornerRadii::all(radius))));
}

/// **What the theme leaves out is not drawn**: no shadow at 0, no border at
/// 0.
#[test]
fn what_the_theme_leaves_out_is_not_drawn() {
    let cmds = drawn(
        DecorationStyle {
            shadow: 0,
            border: 0,
            ..DecorationStyle::AERO
        },
        "Run",
        false,
    );
    assert!(
        !cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::BoxShadow { .. }))
    );
    assert!(
        !cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::StrokeRect { .. }))
    );
}

/// **The title is written as the theme writes titles**: bold, in the middle
/// of the bar, and a long one cut as it says.
#[test]
fn the_title_is_written_as_the_theme_writes_titles() {
    let title = |cmds: &[RenderCommand]| {
        cmds.iter()
            .find_map(|c| match c {
                RenderCommand::Text {
                    x,
                    text,
                    font_weight,
                    ..
                } => Some((*x, text.clone(), *font_weight)),
                _ => None,
            })
            .expect("no title")
    };
    let (x, _, weight) = title(&drawn(
        DecorationStyle {
            title_bold: true,
            title_align: TitleAlign::Center,
            ..DecorationStyle::AERO
        },
        "Run",
        false,
    ));
    assert_eq!(weight, FontWeightHint::Bold);
    let width = text::measure("Run", 13.0, FontWeightHint::Bold);
    assert!((x - (101.0 + (448.0 - width) / 2.0)).abs() < 0.5, "{x}");

    let long = "a very long title ".repeat(10);
    let (_, shown, _) = title(&drawn(
        DecorationStyle {
            title_overflow: guitk::text::Overflow::KeepTail,
            ..DecorationStyle::AERO
        },
        &long,
        false,
    ));
    assert!(shown.starts_with('…'), "{shown}");
    assert!(shown.ends_with("title "), "{shown}");
}

/// **A glyph button has no face until the pointer is on it**, and draws its
/// cross either way; the other shapes have a face and no cross.
#[test]
fn a_glyph_button_is_its_mark() {
    let glyph = DecorationStyle {
        button_shape: ButtonShape::Glyph,
        ..DecorationStyle::AERO
    };
    let close_colour = DecorationColors::from_palette(&palette()).close_button;
    let faces = |cmds: &[RenderCommand]| {
        cmds.iter()
            .filter(|c| matches!(c, RenderCommand::FillRect { width, .. } if *width == 20.0))
            .count()
    };
    let lines = |cmds: &[RenderCommand]| {
        cmds.iter()
            .filter(|c| matches!(c, RenderCommand::Line { .. }))
            .count()
    };
    let rest = drawn(glyph, "Run", false);
    assert_eq!((faces(&rest), lines(&rest)), (0, 2));
    let lit = drawn(glyph, "Run", true);
    assert_eq!((faces(&lit), lines(&lit)), (1, 2));
    let circle = drawn(
        DecorationStyle {
            button_shape: ButtonShape::Circle,
            ..DecorationStyle::AERO
        },
        "Run",
        false,
    );
    assert_eq!((faces(&circle), lines(&circle)), (1, 0));
    assert!(circle.iter().any(|c| matches!(c,
        RenderCommand::FillRect { color, corner_radii, .. }
            if *color == close_colour && *corner_radii == CornerRadii::all(10.0))));
    // Lit, the face is a shade apart from at rest.
    let face = |cmds: &[RenderCommand]| {
        cmds.iter().find_map(|c| match c {
            RenderCommand::FillRect { width, color, .. } if *width == 20.0 => Some(*color),
            _ => None,
        })
    };
    let at_rest = drawn(DecorationStyle::AERO, "Run", false);
    let lit = drawn(DecorationStyle::AERO, "Run", true);
    assert_ne!(face(&at_rest), face(&lit));
}

/// **Scale grows every part**: at 2, the bar is 60 and the button 40.
#[test]
fn scale_grows_every_part() {
    let f = DialogFrame::from_settings(&AppearanceSettings::default(), 2.0);
    let layout = f.layout(OUTER);
    assert_eq!(layout.title_bar().h, 60.0);
    assert_eq!(layout.close().unwrap().w, 40.0);
    // A scale that is not one is taken as 1.
    let nonsense = DialogFrame::from_settings(&AppearanceSettings::default(), f32::NAN);
    assert_eq!(nonsense.layout(OUTER).title_bar().h, 30.0);
}
