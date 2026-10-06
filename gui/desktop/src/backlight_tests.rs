#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::*;

/// A report as `kernel/src/fs/procfs.rs`'s `gen_brightness` writes one:
/// `"  {:<3} {:<20} {:>3}%  min {:>3}%  [{}]"` a display.
const REPORT: &str = "display_count: 2\n\
total_adjustments: 3\n\
total_auto: 0\n\
ops: 7\n\
Displays:\n  \
0   Built-in display      80%  min   5%  [manual]\n  \
1   HDMI 1               100%  min   0%  [auto]\n";

/// **Each display's row is read, a name with spaces in it included.**
#[test]
fn each_displays_row_is_read() {
    assert_eq!(
        parse(REPORT),
        [
            Display {
                id: 0,
                name: "Built-in display".to_owned(),
                level: 80,
                min: 5,
                mode: "manual".to_owned(),
            },
            Display {
                id: 1,
                name: "HDMI 1".to_owned(),
                level: 100,
                min: 0,
                mode: "auto".to_owned(),
            },
        ]
    );
}

/// **What is not a display's row is passed over**: the counters above the
/// section, a row cut short, a level past 100, and nothing at all.
#[test]
fn what_is_not_a_row_is_passed_over() {
    assert!(parse("display_count: 0\nDisplays:\n").is_empty());
    assert!(parse("").is_empty());
    assert!(
        parse("  0 a 80%  min 5%  [manual]\n").is_empty(),
        "no section"
    );
    let odd = "Displays:\n  0 cut short 80%\n  1 too bright 180%  min 0%  [x]\n  \
               2 fine 40%  min 1%  [manual]\nafter: 1\n  3 past 9%  min 0%  [x]\n";
    let rows = parse(odd);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!((rows[0].id, rows[0].level), (2, 40));
}

/// A report file of its own for one test.
fn report_file(text: &str) -> (scratchdir::ScratchDir, PathBuf) {
    let dir = scratchdir::ScratchDir::new("backlight");
    let path = dir.path("brightness");
    std::fs::write(&path, text).unwrap();
    (dir, path)
}

/// **The first display's level is what the pane shows, with why it cannot
/// be changed** -- and with no report, or no display in it, no level and
/// why. A shell that asked for no screen reads nothing.
#[test]
fn the_first_displays_level_is_read_with_why() {
    let (_dir, path) = report_file(REPORT);
    assert_eq!(Source::Report(path).read(), Some((Some(80), CANNOT_SET)));
    let (_dir, path) = report_file("display_count: 0\nDisplays:\n");
    assert_eq!(Source::Report(path).read(), Some((None, NO_REPORT)));
    let (dir, _) = report_file("");
    assert_eq!(
        Source::Report(dir.path("missing")).read(),
        Some((None, NO_REPORT))
    );
    assert_eq!(Source::Own.read(), None);
    assert_eq!(
        Source::kernel(),
        Source::Report(PathBuf::from("/proc/brightness"))
    );
}
