//! Tests for the disk usage widget: what it says of a disk, where its disks
//! go, what it draws them in, and when the shell reads them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::*;
use appearance::palette_check::assert_drawn_from;

/// A veil no default gives a widget, so no wash is right by accident.
const ODD_ALPHA: u8 = 137;

/// A disk at `point` with `total` blocks of 4 KiB, `free` of them free.
fn disk(point: &[u8], total: u64, free: u64) -> Disk {
    Disk {
        point: point.to_vec(),
        space: Ok(Space {
            block_size: 4096,
            total_blocks: total,
            free_blocks: free,
            read_only: false,
        }),
    }
}

/// `n` disks as the widget is told them: the first nearly full, the
/// second not measured, the rest half full.
fn readings(n: usize) -> Vec<DiskReading> {
    (0..n)
        .map(|i| match i {
            0 => reading(&disk(b"/", 1000, 50)),
            1 => reading(&Disk {
                point: b"/gone".to_vec(),
                space: Err(5),
            }),
            _ => reading(&disk(format!("/mnt/{i}").as_bytes(), 1000, 500)),
        })
        .collect()
}

/// A wide widget's content, one cell high.
fn wide() -> Rect {
    Rect::new(100.0, 200.0, 356.0, 96.0)
}

/// A large widget's content.
fn large() -> Rect {
    Rect::new(100.0, 200.0, 356.0, 258.0)
}

/// The texts `commands` draws, each with where.
fn texts(commands: &[RenderCommand]) -> Vec<(&str, f32, f32, Color)> {
    commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::Text {
                text, x, y, color, ..
            } => Some((text.as_str(), *x, *y, *color)),
            _ => None,
        })
        .collect()
}

/// The rectangles `commands` fills: `(x, y, width, colour)`.
fn fills(commands: &[RenderCommand]) -> Vec<(f32, f32, f32, Color)> {
    commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::FillRect {
                x, y, width, color, ..
            } => Some((*x, *y, *width, *color)),
            _ => None,
        })
        .collect()
}

/// **A disk is said by where it is mounted, how much is in use and what is
/// free of what it holds**, in binary units; nearly full at nine tenths.
#[test]
fn a_disk_is_said_by_its_point_its_share_and_its_room() {
    let half = reading(&disk(b"/mnt", 262_144, 131_072));
    assert_eq!(half.name, "/mnt");
    assert_eq!(half.fraction, Some(0.5));
    assert_eq!(half.percent, Some(50));
    assert_eq!(half.detail, "512.0 MiB free of 1.0 GiB");
    assert!(!half.nearly_full);
    let full = reading(&disk(b"/", 1000, 100));
    assert_eq!(full.percent, Some(90));
    assert!(full.nearly_full);
}

/// **A read-only disk says what it holds**, and is never nearly full; one
/// not measured says so, with no share; a byte of its name that is not
/// text is drawn as an escape, not as a replacement character.
#[test]
fn a_disc_says_what_it_holds_and_one_unmeasured_says_so() {
    let disc = reading(&Disk {
        point: b"/media/cd".to_vec(),
        space: Ok(Space {
            block_size: 2048,
            total_blocks: 332_800,
            free_blocks: 0,
            read_only: true,
        }),
    });
    assert_eq!(disc.detail, "650.0 MiB, read-only");
    assert_eq!(disc.percent, Some(100));
    assert!(!disc.nearly_full);
    let unmeasured = reading(&Disk {
        point: b"/caf\xe9".to_vec(),
        space: Err(38),
    });
    assert_eq!(unmeasured.name, "/caf\\351");
    assert_eq!(unmeasured.detail, NOT_MEASURED);
    assert_eq!((unmeasured.fraction, unmeasured.percent), (None, None));
    assert!(!unmeasured.nearly_full);
}

/// **The disks are listed in the content, and what does not fit is
/// counted** under them.
#[test]
fn the_disks_are_listed_and_the_rest_counted() {
    let shown = readings(4);
    let all = rows(&shown, large());
    assert_eq!(all.rows.len(), 4);
    assert_eq!(all.more, None);
    let cut = rows(&shown, wide());
    assert!(!cut.rows.is_empty(), "no disk fitted a wide widget");
    let (more, at) = cut.more.expect("four disks fitted one cell's height");
    assert_eq!(more, 4 - cut.rows.len());
    assert!(at.y + at.h <= wide().y + wide().h, "the count is below");
}

/// **Each disk is drawn in its row**: its name and share on the first
/// line, its bar under them filled as far as it is used, and what is free
/// under the bar -- the bar the disk's colour, or red, as the share's
/// text, when it is nearly full.
#[test]
fn each_disk_is_drawn_in_its_row() {
    let p = Palette::for_mode(false);
    let shown = readings(3);
    let listed = Disks::Listed(shown.clone());
    let mut commands = Vec::new();
    render(&listed, large(), &p, 255, &mut commands);
    let said = texts(&commands);
    let filled = fills(&commands);
    let laid = rows(&shown, large());
    for &(index, bounds) in &laid.rows {
        let disk = &shown[index];
        for line in [
            Some(disk.name.clone()),
            disk.percent.map(|n| format!("{n}%")),
            Some(disk.detail.clone()),
        ]
        .into_iter()
        .flatten()
        {
            let (_, x, y, _) = said
                .iter()
                .find(|(t, ..)| *t == line)
                .unwrap_or_else(|| panic!("{line:?} is not drawn"));
            assert!(
                *y >= bounds.y && *y < bounds.y + bounds.h && *x >= bounds.x,
                "{line:?} outside its row {bounds:?}"
            );
        }
        let trough = bar(bounds);
        let at_bar: Vec<_> = filled.iter().filter(|(_, y, ..)| *y == trough.y).collect();
        assert_eq!(at_bar[0].2, trough.w, "the trough is the row's width");
        match disk.fraction {
            Some(fraction) => {
                assert_eq!(at_bar.len(), 2, "disk {index}: a trough and its fill");
                assert!((at_bar[1].2 - trough.w * fraction).abs() < 0.01);
                let want = if disk.nearly_full { p.red } else { p.peach };
                assert_eq!(at_bar[1].3, want, "disk {index}'s fill");
            }
            None => assert_eq!(at_bar.len(), 1, "an unmeasured disk is filled"),
        }
    }
    let share = said
        .iter()
        .find(|(t, ..)| *t == "95%")
        .expect("the full one's share");
    assert_eq!(share.3, p.ink(p.red));
    let share = said
        .iter()
        .find(|(t, ..)| *t == "50%")
        .expect("a half one's share");
    assert_eq!(share.3, p.text);
}

/// **In place of a list it says why there is none** -- the disks could not
/// be listed, or there are none -- and nothing at all before the first
/// reading.
#[test]
fn in_place_of_a_list_it_says_why() {
    let p = Palette::for_mode(true);
    let drawn = |disks: &Disks| {
        let mut commands = Vec::new();
        render(disks, large(), &p, 255, &mut commands);
        texts(&commands)
            .iter()
            .map(|(t, ..)| (*t).to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(drawn(&Disks::Unlisted("denied".to_owned())), [UNLISTED]);
    assert_eq!(drawn(&Disks::Listed(Vec::new())), [NO_DISKS]);
    assert!(drawn(&Disks::Unread).is_empty());
}

/// **Everything it draws is its palette's**, in both modes, at any veil.
#[test]
fn every_colour_it_draws_comes_from_its_palette() {
    for light in [false, true] {
        let mut p = Palette::for_mode(light);
        p.accent = appearance::PINK;
        for disks in [
            Disks::Listed(readings(4)),
            Disks::Listed(Vec::new()),
            Disks::Unlisted("denied".to_owned()),
        ] {
            for content in [large(), wide()] {
                let mut commands = Vec::new();
                render(&disks, content, &p, ODD_ALPHA, &mut commands);
                assert_drawn_from(&p, &commands, &[], &format!("disk usage (light={light})"));
            }
        }
    }
}

/// **Nothing it draws follows the accent**: how full a disk is, is a
/// measurement, not something the user chose -- in either mode.
#[test]
fn nothing_it_draws_follows_the_accent() {
    for light in [false, true] {
        let drawn = |accent: Color| {
            let mut p = Palette::for_mode(light);
            p.accent = accent;
            let mut commands = Vec::new();
            render(&Disks::Listed(readings(4)), large(), &p, 255, &mut commands);
            commands
        };
        assert_eq!(
            drawn(appearance::PINK),
            drawn(appearance::TEAL),
            "light={light}"
        );
    }
}

/// **Everything it draws is under the widget's veil**: on a translucent
/// widget its bars and words are as translucent as its background.
#[test]
fn everything_it_draws_is_under_the_widgets_veil() {
    let p = Palette::for_mode(false);
    let mut commands = Vec::new();
    render(
        &Disks::Listed(readings(3)),
        large(),
        &p,
        ODD_ALPHA,
        &mut commands,
    );
    assert!(!commands.is_empty());
    for command in &commands {
        let color = match command {
            RenderCommand::Text { color, .. } | RenderCommand::FillRect { color, .. } => *color,
            other => panic!("{other:?} drawn"),
        };
        assert_eq!(color.a, ODD_ALPHA, "{command:?}");
    }
}

/// **The shell reads the disks no oftener than its spacing**: at once, then
/// not again until ten seconds have passed.
#[test]
fn the_disks_are_read_no_oftener_than_the_spacing() {
    let dir = scratchdir::ScratchDir::new("disks-sampler");
    let table = dir.path("mounts");
    std::fs::write(&table, b"none / ext4 rw 0 0\nnone /proc procfs rw 0 0\n").unwrap();
    let mut sampler = Sampler::of_table(table.clone());
    assert_eq!(sampler.disks(), &Disks::Unread);
    assert!(sampler.sample(5_000));
    let Disks::Listed(first) = sampler.disks().clone() else {
        panic!("{:?}", sampler.disks());
    };
    let names: Vec<&str> = first.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["/"]);
    std::fs::write(&table, b"none / ext4 rw 0 0\nnone /mnt ext4 rw 0 0\n").unwrap();
    assert!(!sampler.sample(5_000 + SAMPLE_SPACING_MS - 1));
    assert_eq!(sampler.disks(), &Disks::Listed(first));
    assert!(sampler.sample(5_000 + SAMPLE_SPACING_MS));
    let Disks::Listed(second) = sampler.disks() else {
        panic!("{:?}", sampler.disks());
    };
    assert_eq!(second.len(), 2);
}

/// **A mount table that will not read is said**, with why, rather than
/// taken for no disks.
#[test]
fn a_table_that_will_not_read_is_said() {
    let dir = scratchdir::ScratchDir::new("disks-no-table");
    let mut sampler = Sampler::of_table(dir.path("mounts"));
    assert!(sampler.sample(0));
    assert!(
        matches!(sampler.disks(), Disks::Unlisted(why) if !why.is_empty()),
        "{:?}",
        sampler.disks()
    );
}
