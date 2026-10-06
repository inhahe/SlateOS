#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::cell::RefCell;
use std::rc::Rc;

use super::*;

/// A report as `kernel/src/fs/procfs.rs`'s `gen_brightness` writes one:
/// `"  {:<3} {:<20} {:>3}%  min {:>3}%  [{}]"` a display.
pub(crate) const REPORT: &str = "display_count: 2\n\
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

/// A setter that keeps what it was asked, and answers `refuse` while it is
/// set.
#[derive(Clone, Default)]
pub(crate) struct Recording {
    pub(crate) asked: Rc<RefCell<Vec<(u32, u8)>>>,
    pub(crate) refuse: Rc<RefCell<Option<i32>>>,
}

impl Setter for Recording {
    fn set(&mut self, display: u32, level: u8) -> Result<(), i32> {
        self.asked.borrow_mut().push((display, level));
        match *self.refuse.borrow() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

/// A report file of its own for one test.
pub(crate) fn report_file(text: &str) -> (scratchdir::ScratchDir, PathBuf) {
    let dir = scratchdir::ScratchDir::new("backlight");
    let path = dir.path("brightness");
    std::fs::write(&path, text).unwrap();
    (dir, path)
}

/// **Where the desktop may set it, the reading is live -- found by setting
/// the level it already has -- and a write sets only a change.**
#[test]
fn where_the_desktop_may_set_it_the_reading_is_live() {
    let (_dir, path) = report_file(REPORT);
    let setter = Recording::default();
    let mut b = Backlight::with(path, Box::new(setter.clone()));
    assert_eq!(b.read(), Reading::Live(80));
    assert!(b.is_live());
    assert_eq!(*setter.asked.borrow(), [(0, 80)], "set to itself, unseen");
    assert_eq!(b.write(80), Ok(()));
    assert_eq!(setter.asked.borrow().len(), 1, "no change, no call");
    assert_eq!(b.write(35), Ok(()));
    assert_eq!(
        setter.asked.borrow().last(),
        Some(&(0, 35)),
        "the first display"
    );
}

/// **Where it may not, the level is read and said to be fixed**; and a
/// refusal after a live reading fixes it from then on.
#[test]
fn where_it_may_not_the_level_is_fixed() {
    let (_dir, path) = report_file(REPORT);
    let setter = Recording::default();
    *setter.refuse.borrow_mut() = Some(1);
    let mut b = Backlight::with(path.clone(), Box::new(setter.clone()));
    assert_eq!(b.read(), Reading::Fixed(Some(80), CANNOT_SET));
    assert!(!b.is_live());
    assert_eq!(b.write(50), Err(CANNOT_SET));

    let setter = Recording::default();
    let mut b = Backlight::with(path, Box::new(setter.clone()));
    assert_eq!(b.read(), Reading::Live(80));
    *setter.refuse.borrow_mut() = Some(1);
    assert_eq!(b.write(50), Err(CANNOT_SET));
    assert!(!b.is_live());
}

/// **With no report, or no display in it, there is no level, and why; a
/// shell that asked for no screen reads nothing.**
#[test]
fn with_no_report_there_is_no_level() {
    let (_dir, path) = report_file("display_count: 0\nDisplays:\n");
    let mut b = Backlight::with(path, Box::new(Recording::default()));
    assert_eq!(b.read(), Reading::Fixed(None, NO_REPORT));
    let (dir, _) = report_file("");
    let mut b = Backlight::with(dir.path("missing"), Box::new(Recording::default()));
    assert_eq!(b.read(), Reading::Fixed(None, NO_REPORT));
    assert_eq!(Backlight::own().read(), Reading::Own);
}

/// **The kernel's call is no call off SlateOS.**
#[test]
#[cfg(not(all(target_os = "linux", target_vendor = "slateos")))]
fn the_kernels_call_is_none_elsewhere() {
    assert_eq!(KernelSetter.set(0, 50), Err(38));
}
