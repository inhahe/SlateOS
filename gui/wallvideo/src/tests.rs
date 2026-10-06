#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::*;

/// **A frame larger than the screen needs is made just large enough to
/// cover it, its shape kept; one no larger is left alone.**
#[test]
fn a_frame_is_sent_just_large_enough_to_cover() {
    assert_eq!(
        cover_size((3840, 2160), (1920, 1080)),
        (1920, 1080),
        "4K on 1080p"
    );
    assert_eq!(
        cover_size((4000, 1000), (1000, 500)),
        (2000, 500),
        "a wide film: as tall as the screen, wider than it"
    );
    assert_eq!(cover_size((3840, 2400), (1920, 1080)), (1920, 1200));
    assert_eq!(
        cover_size((1000, 3000), (1920, 1080)),
        (1000, 3000),
        "narrower than the screen: already short of covering it"
    );
    assert_eq!(
        cover_size((1280, 720), (1920, 1080)),
        (1280, 720),
        "never up"
    );
    assert_eq!(
        cover_size((1920, 1080), (0, 0)),
        (1920, 1080),
        "no screen yet"
    );
}

/// **Shrinking averages the pixels each new one covers**: a block of four
/// becomes their mean, channel by channel, and every source pixel counts.
#[test]
fn shrinking_averages_what_each_pixel_covers() {
    // 2 by 2 -> 1 by 1: the mean of red, green, blue and white.
    let px = [0xFFFF_0000, 0xFF00_FF00, 0xFF00_00FF, 0xFFFF_FFFF];
    assert_eq!(shrink(&px, (2, 2), (1, 1)), [0xFF80_8080]);
    // 4 by 1 -> 2 by 1: two pairs, each its own mean.
    let row = [0xFF00_0000, 0xFF00_0002, 0xFF00_0010, 0xFF00_0030];
    assert_eq!(shrink(&row, (4, 1), (2, 1)), [0xFF00_0001, 0xFF00_0020]);
    // 3 by 1 -> 2 by 1: no pixel lost or counted twice.
    let three = [0xFF00_0000, 0xFF00_0003, 0xFF00_0009];
    assert_eq!(shrink(&three, (3, 1), (2, 1)), [0xFF00_0000, 0xFF00_0006]);
    // The same size, or a larger one, or pixels that do not fill: as was.
    assert_eq!(shrink(&px, (2, 2), (2, 2)), px);
    assert_eq!(shrink(&px, (2, 2), (4, 4)), px);
    assert_eq!(shrink(&px[..3], (2, 2), (1, 1)), &px[..3]);
}

/// **A frame is due its time after the first one's, from when playing
/// started**; one before the first is due at once.
#[test]
fn a_frame_is_due_its_time_after_the_first() {
    let start = Instant::now();
    assert_eq!(due(start, 1_000, 1_000), start);
    assert_eq!(
        due(start, 1_000, 41_667_000),
        start + Duration::from_micros(41_666)
    );
    assert_eq!(due(start, 5_000, 1_000), start, "before the first: at once");
}
