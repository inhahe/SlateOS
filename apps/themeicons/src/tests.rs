use super::*;
use appearance::themes::ThemeDirs;
use guitk::render::RenderTree;
use std::ffi::OsStr;
use std::path::PathBuf;

/// A theme with no folder anywhere, and no programs' icons: only the
/// built-in icons draw, so nothing of the machine running the test is read.
fn theme(id: &str) -> IconTheme {
    IconTheme::named(
        OsStr::new(id),
        ThemeDirs {
            user: None,
            system: PathBuf::from("/nonexistent/slateos-test/themes"),
        },
    )
    .with_data_dirs(Vec::new())
}

/// A frame naming each of `ids` as an image.
fn frame(ids: &[u64]) -> Vec<RenderCommand> {
    let mut tree = RenderTree::new();
    for id in ids {
        tree.push(RenderCommand::Image {
            x: 0.0,
            y: 0.0,
            width: 16.0,
            height: 16.0,
            image_id: *id,
        });
    }
    tree.commands
}

/// The ids `changes` uploads, and those it drops, in order.
fn uploads_and_drops(changes: &[ImageChange]) -> (Vec<u64>, Vec<u64>) {
    let mut uploads = Vec::new();
    let mut drops = Vec::new();
    for change in changes {
        match change {
            ImageChange::Upload { id, .. } => uploads.push(*id),
            ImageChange::Drop(id) => drops.push(*id),
            ImageChange::Patch { .. } => panic!("an icon is never patched"),
        }
    }
    (uploads, drops)
}

/// **An icon drawn is uploaded once, before the frame, at the size asked**,
/// however many frames name it; a picture of the program's own is left to
/// the program.
#[test]
fn an_icon_drawn_is_uploaded_once_at_its_size() {
    let mut icons = Icons::new(theme("aero"));
    let folder = icons.icon("folder", 16, Color::BLACK);
    let own_picture = 7;
    icons.frame_drawn(&frame(&[folder, own_picture]));
    let changes = icons.take_images();
    assert_eq!(uploads_and_drops(&changes), (vec![folder], vec![]));
    let ImageChange::Upload {
        width,
        height,
        stride,
        bytes,
        ..
    } = &changes[0]
    else {
        panic!("not an upload: {changes:?}");
    };
    assert_eq!((*width, *height, *stride), (16, 16, 64));
    assert_eq!(bytes.as_slice().len(), 16 * 16 * 4);

    let again = icons.icon("folder", 16, Color::BLACK);
    assert_eq!(again, folder, "the same icon is another id");
    icons.frame_drawn(&frame(&[again]));
    assert!(icons.take_images().is_empty(), "uploaded twice");
}

/// **A name nothing draws uploads nothing, and is not looked for again.**
#[test]
fn a_name_nothing_draws_uploads_nothing() {
    let mut icons = Icons::new(theme("aero"));
    let nothing = icons.icon("no-such-picture-anywhere", 16, Color::BLACK);
    icons.frame_drawn(&frame(&[nothing]));
    assert!(icons.take_images().is_empty());
    // Its fallback draws where it does not.
    let fallen = icons.icon_or("no-such-picture-anywhere", "folder", 16, Color::BLACK);
    icons.frame_drawn(&frame(&[fallen]));
    assert_eq!(
        uploads_and_drops(&icons.take_images()),
        (vec![fallen], vec![])
    );
}

/// **Another colour, or another theme, drops every icon the window holds**,
/// and the next frame uploads its icons afresh; the same theme again
/// changes nothing.
#[test]
fn another_appearance_drops_every_icon_and_draws_them_again() {
    let mut icons = Icons::new(theme("aero"));
    let folder = icons.icon("folder", 16, Color::BLACK);
    let trash = icons.icon("user-trash", 24, Color::BLACK);
    icons.frame_drawn(&frame(&[folder, trash]));
    let _sent = icons.take_images();

    icons.set_theme(theme("aero"));
    assert!(
        icons.take_images().is_empty(),
        "the same theme dropped icons"
    );

    icons.forget();
    let (uploads, mut drops) = uploads_and_drops(&icons.take_images());
    drops.sort_unstable();
    let mut both = vec![folder, trash];
    both.sort_unstable();
    assert_eq!((uploads, drops), (vec![], both));
    // Forgotten: the frame's icons are sent again.
    let white = icons.icon("folder", 16, Color::WHITE);
    assert_ne!(white, folder, "a colour is not part of the id");
    icons.frame_drawn(&frame(&[white]));
    assert_eq!(
        uploads_and_drops(&icons.take_images()),
        (vec![white], vec![])
    );

    icons.set_theme(theme("another"));
    assert_eq!(icons.theme().id(), OsStr::new("another"));
    assert_eq!(
        uploads_and_drops(&icons.take_images()),
        (vec![], vec![white])
    );
}

/// **A picture the program uploads itself is never the icons' to drop**,
/// even under an id that happens to carry the icon tag: only an id the
/// registry gave is held, so only those are dropped when the colours change.
#[test]
fn a_picture_of_the_programs_own_is_never_dropped() {
    let mut icons = Icons::new(theme("aero"));
    let own = appearance::icons::ICON_ID_TAG | 0x7448;
    let folder = icons.icon("folder", 16, Color::BLACK);
    icons.frame_drawn(&frame(&[own, folder]));
    assert_eq!(
        uploads_and_drops(&icons.take_images()),
        (vec![folder], vec![])
    );
    icons.forget();
    assert_eq!(
        uploads_and_drops(&icons.take_images()),
        (vec![], vec![folder]),
        "the program's own picture was dropped with the icons"
    );
}

/// **An icon drawn is known as one; a picture of the program's own is not**,
/// whatever its id -- and nothing is, once the icons are forgotten.
#[test]
fn an_icon_drawn_is_told_from_a_picture_of_the_programs_own() {
    let mut icons = Icons::new(theme("aero"));
    let folder = icons.icon("folder", 16, Color::BLACK);
    assert!(icons.drew(folder));
    assert!(!icons.drew(appearance::icons::ICON_ID_TAG | 0x7448));
    assert!(!icons.drew(7));
    icons.forget();
    assert!(!icons.drew(folder), "a forgotten icon is still known");
}
