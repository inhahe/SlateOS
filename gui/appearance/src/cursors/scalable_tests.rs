#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use std::fs;
use std::path::PathBuf;

use scratchdir::ScratchDir;

use super::*;

/// A square SVG of `side` user units, filled with `fill`.
fn square(side: u32, fill: &str) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{side}" height="{side}" viewBox="0 0 {side} {side}"><rect width="{side}" height="{side}" fill="{fill}"/></svg>"#
    )
}

/// A scalable cursor's folder in `dir`, its pictures `files` and its
/// metadata `metadata`.
fn cursor(dir: &ScratchDir, name: &str, files: &[(&str, String)], metadata: &str) -> PathBuf {
    let folder = dir.path(name);
    fs::create_dir_all(&folder).unwrap();
    for (file, svg) in files {
        fs::write(folder.join(file), svg).unwrap();
    }
    fs::write(folder.join(METADATA_FILE), metadata).unwrap();
    folder
}

/// **A scalable cursor is drawn at the size asked**: a picture drawn 24
/// units for a nominal 24, asked at 48, is 48 pixels, its hot spot twice as
/// far in, its pixels the picture's, premultiplied.
#[test]
fn a_scalable_cursor_is_drawn_at_the_size_asked() {
    let dir = ScratchDir::new("scalable-size");
    let folder = cursor(
        &dir,
        "default",
        &[("default.svg", square(24, "#ff0000"))],
        r#"[{"filename": "default.svg", "nominal_size": 24, "hotspot_x": 3, "hotspot_y": 5}]"#,
    );
    for (size, side, hot) in [(24, 24, (3, 5)), (48, 48, (6, 10)), (36, 36, (5, 8))] {
        let images = read(&folder, size).expect("a cursor");
        assert_eq!(images.nominal, size);
        let frame = &images.frames[0];
        assert_eq!((frame.width, frame.height), (side, side), "at {size}");
        assert_eq!((frame.hot_x, frame.hot_y), hot, "at {size}");
        assert_eq!(frame.pixels.len(), (side * side) as usize);
        assert_eq!(frame.pixels[0], 0xFFFF_0000, "opaque red");
    }
}

/// **A picture drawn larger than its nominal size keeps its proportion**: a
/// 32-unit picture for a nominal 24 is a third larger than the size asked,
/// as Breeze draws its pictures with room around the pointer.
#[test]
fn a_picture_larger_than_its_nominal_size_keeps_its_proportion() {
    let dir = ScratchDir::new("scalable-margin");
    let folder = cursor(
        &dir,
        "default",
        &[("default.svg", square(32, "#00ff00"))],
        r#"[{"filename": "default.svg", "nominal_size": 24, "hotspot_x": 8, "hotspot_y": 8}]"#,
    );
    let frame = &read(&folder, 48).unwrap().frames[0];
    assert_eq!((frame.width, frame.height), (64, 64));
    assert_eq!((frame.hot_x, frame.hot_y), (16, 16));
}

/// **An animated cursor is its frames in order, each with its delay**; a
/// translucent picture's pixels are premultiplied.
#[test]
fn an_animated_cursor_is_its_frames_in_order() {
    let dir = ScratchDir::new("scalable-frames");
    let folder = cursor(
        &dir,
        "wait",
        &[
            ("wait-01.svg", square(24, "#0000ff")),
            ("wait-02.svg", square(24, "rgba(255,255,255,0.5)")),
        ],
        r#"[
            {"filename": "wait-01.svg", "nominal_size": 24, "hotspot_x": 12, "hotspot_y": 12, "delay": 30},
            {"filename": "wait-02.svg", "nominal_size": 24, "hotspot_x": 12, "hotspot_y": 12, "delay": 45}
        ]"#,
    );
    let images = read(&folder, 24).unwrap();
    assert_eq!(images.frames.len(), 2);
    assert_eq!(images.frames[0].delay_ms, 30);
    assert_eq!(images.frames[1].delay_ms, 45);
    assert_eq!(images.frames[0].pixels[0], 0xFF00_00FF);
    let half = images.frames[1].pixels[0];
    let alpha = half >> 24;
    assert!((126..=129).contains(&alpha), "{half:#010x}");
    assert_eq!(
        half & 0xFF,
        alpha,
        "white at half alpha is premultiplied to half"
    );
}

/// **What is not a scalable cursor is none**: no metadata, metadata that is
/// not JSON or not a list of frames, a frame naming a file elsewhere, a
/// missing picture, no size, or a size past the bound.
#[test]
fn what_is_not_a_scalable_cursor_is_none() {
    let dir = ScratchDir::new("scalable-bad");
    let svg = || vec![("a.svg", square(24, "#ff0000"))];
    let good = r#"[{"filename": "a.svg", "nominal_size": 24}]"#;
    assert!(
        read(&cursor(&dir, "fine", &svg(), good), 24).is_some(),
        "the control"
    );
    let empty = dir.path("nothing");
    fs::create_dir_all(&empty).unwrap();
    assert!(read(&empty, 24).is_none(), "no metadata");
    for (name, metadata) in [
        ("garbled", "[{"),
        ("object", r#"{"filename": "a.svg", "nominal_size": 24}"#),
        ("no-frames", "[]"),
        ("no-file", r#"[{"nominal_size": 24}]"#),
        ("no-nominal", r#"[{"filename": "a.svg"}]"#),
        (
            "zero-nominal",
            r#"[{"filename": "a.svg", "nominal_size": 0}]"#,
        ),
        (
            "text-nominal",
            r#"[{"filename": "a.svg", "nominal_size": "24"}]"#,
        ),
        (
            "negative-hot",
            r#"[{"filename": "a.svg", "nominal_size": 24, "hotspot_x": -1}]"#,
        ),
        (
            "elsewhere",
            r#"[{"filename": "../a.svg", "nominal_size": 24}]"#,
        ),
        (
            "absolute",
            r#"[{"filename": "/etc/a.svg", "nominal_size": 24}]"#,
        ),
        ("missing", r#"[{"filename": "b.svg", "nominal_size": 24}]"#),
        (
            "trailing",
            r#"[{"filename": "a.svg", "nominal_size": 24}] x"#,
        ),
        (
            "negative-hot-down",
            r#"[{"filename": "a.svg", "nominal_size": 24, "hotspot_y": -1}]"#,
        ),
        (
            "negative-delay",
            r#"[{"filename": "a.svg", "nominal_size": 24, "delay": -5}]"#,
        ),
        (
            "text-hot",
            r#"[{"filename": "a.svg", "nominal_size": 24, "hotspot_x": "3"}]"#,
        ),
        (
            "endless-hot",
            r#"[{"filename": "a.svg", "nominal_size": 24, "hotspot_x": 1e999}]"#,
        ),
        ("number-file", r#"[{"filename": 7, "nominal_size": 24}]"#),
    ] {
        let folder = cursor(&dir, name, &svg(), metadata);
        assert!(read(&folder, 24).is_none(), "{name}");
    }
    let folder = cursor(&dir, "sizes", &svg(), good);
    assert!(read(&folder, 0).is_none(), "no size");
    assert!(read(&folder, MAX_SIDE + 1).is_none(), "past the bound");
    // Asked so large that the picture would be past the bound.
    let huge = cursor(
        &dir,
        "huge",
        &[("a.svg", square(24, "#ff0000"))],
        r#"[{"filename": "a.svg", "nominal_size": 1}]"#,
    );
    assert!(
        read(&huge, 64).is_none(),
        "24 units at 64 times is past the side"
    );
}

/// **JSON is read as the grammar says**: every kind of value, escapes and
/// surrogate pairs included -- and what the grammar does not allow is not
/// read.
#[test]
fn json_is_read_as_the_grammar_says() {
    assert_eq!(
        parse(r#" { "a" : [1, -2.5e1, true, false, null, "x\"\\\/\n\u00e9\ud83d\ude00"] } "#),
        Some(Json::Object(vec![(
            "a".to_owned(),
            Json::Array(vec![
                Json::Number(1.0),
                Json::Number(-25.0),
                Json::Bool(true),
                Json::Bool(false),
                Json::Null,
                Json::Text("x\"\\/\n\u{e9}\u{1F600}".to_owned()),
            ]),
        )]))
    );
    for bad in [
        "",
        "01",
        "1.",
        "-",
        ".5",
        "[1,]",
        "{\"a\":1,}",
        "{a:1}",
        "\"\\x\"",
        "\"\\ud83d\"",
        "\"\u{1}\"",
        "tru",
        "[1] [2]",
        "[[[[[[[[[[[[1]]]]]]]]]]]]",
        "\"\\ud83d\\u0041\"",
        "\"\\u+041\"",
        "\"\\q\"",
        "[01]",
        "1.e5",
    ] {
        assert_eq!(parse(bad), None, "{bad:?}");
    }
}

/// **A picture's own size is its width and height where it says them**, its
/// view box's only where it does not: a 16-unit view box drawn 32 by 24 is
/// 32 by 24 at its nominal size.
#[test]
fn a_pictures_own_size_is_its_width_and_height() {
    let dir = ScratchDir::new("scalable-own-size");
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="24" viewBox="0 0 16 16"><rect width="16" height="16" fill="#ff0000"/></svg>"##;
    let folder = cursor(
        &dir,
        "default",
        &[("a.svg", svg.to_owned())],
        r#"[{"filename": "a.svg", "nominal_size": 32}]"#,
    );
    let frame = &read(&folder, 32).unwrap().frames[0];
    assert_eq!((frame.width, frame.height), (32, 24));
}

/// **A hot spot past the picture is held to its last pixel**, as an XCursor
/// file's must be on its picture.
#[test]
fn a_hot_spot_past_the_picture_is_held_to_its_edge() {
    let dir = ScratchDir::new("scalable-hot-edge");
    let folder = cursor(
        &dir,
        "default",
        &[("a.svg", square(24, "#ff0000"))],
        r#"[{"filename": "a.svg", "nominal_size": 24, "hotspot_x": 100, "hotspot_y": 50}]"#,
    );
    let frame = &read(&folder, 24).unwrap().frames[0];
    assert_eq!((frame.hot_x, frame.hot_y), (23, 23));
}

/// **A frame's delay is held to a minute**, its pixels' channels kept in
/// their order, red to blue.
#[test]
fn a_delay_is_held_to_a_minute() {
    let dir = ScratchDir::new("scalable-delay");
    let folder = cursor(
        &dir,
        "default",
        &[("a.svg", square(24, "#102030"))],
        r#"[{"filename": "a.svg", "nominal_size": 24, "delay": 100000}]"#,
    );
    let frame = &read(&folder, 24).unwrap().frames[0];
    assert_eq!(frame.delay_ms, 60_000);
    assert_eq!(frame.pixels[0], 0xFF10_2030);
}

/// **Only a plain name in the cursor's own folder names a picture** -- and a
/// real picture beside the folder, named through `..`, is not read.
#[test]
fn only_a_plain_file_name_names_a_picture() {
    for name in ["a.svg", "a b.svg", ".hidden.svg", "a..svg"] {
        assert!(is_plain_file_name(name), "{name:?}");
    }
    for name in [
        "",
        ".",
        "..",
        "../a.svg",
        "/etc/a.svg",
        "a/b.svg",
        "a\\b.svg",
        "c:a.svg",
        "a\0.svg",
    ] {
        assert!(!is_plain_file_name(name), "{name:?}");
    }
    let dir = ScratchDir::new("scalable-outside");
    fs::write(dir.path("outside.svg"), square(24, "#ff0000")).unwrap();
    let folder = cursor(
        &dir,
        "default",
        &[("a.svg", square(24, "#ff0000"))],
        r#"[{"filename": "../outside.svg", "nominal_size": 24}]"#,
    );
    assert!(read(&folder, 24).is_none());
}

/// **A cursor's frames are bounded**: as many as [`MAX_FRAMES`], not one
/// more; and all their pixels together, so seventeen frames of a million
/// pixels each are no cursor.
#[test]
fn a_cursors_frames_are_bounded() {
    let dir = ScratchDir::new("scalable-bounds");
    let list = |n: usize, file: &str| {
        let one = format!(r#"{{"filename": "{file}", "nominal_size": 24}}"#);
        format!("[{}]", vec![one; n].join(","))
    };
    let small = &[("a.svg", square(24, "#ff0000"))];
    let most = cursor(&dir, "most", small, &list(MAX_FRAMES, "a.svg"));
    assert_eq!(read(&most, 24).unwrap().frames.len(), MAX_FRAMES);
    let more = cursor(&dir, "more", small, &list(MAX_FRAMES + 1, "a.svg"));
    assert!(read(&more, 24).is_none(), "one frame past the bound");
    // Each frame drawn 1024 square: seventeen are past the pixels' bound.
    let large = cursor(&dir, "large", small, &list(17, "a.svg"));
    assert_eq!(MAX_SIDE, 1024);
    assert!(read(&large, MAX_SIDE).is_none(), "past the pixels' bound");
}
