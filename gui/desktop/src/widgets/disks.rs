//! The disk usage widget: each disk the system has mounted and how full it
//! is -- where it is mounted and how much of it is in use, a bar, and what is
//! free of what it holds; the bar red when the disk is nearly full
//! (`design-decisions.md` §1501).
//!
//! The disks are the shell's to find. While a disk usage widget is out, its
//! [`Sampler`] reads the mount table and asks the kernel each disk's space
//! (`fs_space::mounted_disks`) -- at the widget's tick, no oftener than
//! [`SAMPLE_SPACING_MS`]: space is not a reading that moves by the second,
//! and each disk asked is a system call. What it found is drawn as it was
//! told: [`DiskReading`]s, made once by [`reading`], so the drawing and what
//! tools are told say the same words.
//!
//! # One layout
//!
//! [`rows`] places the disks in the widget's content -- as many as fit, and
//! a last line saying how many more (`widgets::listing`) -- and the drawing
//! ([`render`]) and what tools are told (`widgets::accessible`) both read it.

use std::path::PathBuf;

use appearance::Palette;
use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;

use super::listing::{self, Rows};
use crate::Rect;
use crate::fs_space::{self, Disk, Space};

/// How long the shell waits between two readings of the disks.
pub const SAMPLE_SPACING_MS: u64 = 10_000;

/// What the widget says with no disk mounted.
pub const NO_DISKS: &str = "No disks are mounted";
/// What it says when the mount table would not read.
pub const UNLISTED: &str = "The disks could not be listed";
/// What a disk's line says when its space was not measured.
pub const NOT_MEASURED: &str = "Not measured";
/// What tools are told of a disk that is nearly full, after what is free:
/// the bar's red, in words.
pub const NEARLY_FULL: &str = "nearly full";

/// A disk's first line: where it is mounted, and at the right how much of
/// it is in use.
const NAME_SIZE: f32 = 12.0;
const NAME_WEIGHT: FontWeightHint = FontWeightHint::Bold;
/// Room above and below the bar.
const BAR_GAP: f32 = 3.0;
/// The bar's height: a system monitor's meter's.
const BAR_HEIGHT: f32 = 8.0;
/// The line under the bar: what is free of what it holds.
const DETAIL_SIZE: f32 = 11.0;
/// Room between two disks.
const GAP: f32 = 8.0;
/// Room between a disk's name and how much is in use.
const NAME_ROOM: f32 = 6.0;

fn name_line() -> f32 {
    guitk::text::line_height(NAME_SIZE, NAME_WEIGHT)
}

fn detail_line() -> f32 {
    guitk::text::line_height(DETAIL_SIZE, FontWeightHint::Regular)
}

fn row_height() -> f32 {
    name_line() + BAR_GAP + BAR_HEIGHT + BAR_GAP + detail_line()
}

/// A disk as the widget shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct DiskReading {
    /// Where it is mounted, as drawn: a byte of the path that is not text as
    /// an octal escape (`pathcodec::display_bytes`).
    pub name: String,
    /// How much of it is in use, 0.0 to 1.0 -- its bar; `None` when its
    /// space was not measured.
    pub fraction: Option<f32>,
    /// The same in whole percent, truncated: "100%" only when it is full.
    pub percent: Option<u32>,
    /// The line under the bar: what is free of what it holds ("12.1 GiB
    /// free of 45.0 GiB"), what a read-only one holds ("650.0 MiB,
    /// read-only"), or [`NOT_MEASURED`].
    pub detail: String,
    /// Whether it is nearly full (`fs_space::Space::nearly_full`).
    pub nearly_full: bool,
}

/// `disk` as the widget shows it. Sizes in binary units, as the house
/// writes what storage holds (`textfmt::bytes`).
#[must_use]
pub fn reading(disk: &Disk) -> DiskReading {
    let name = pathcodec::display_bytes(&disk.point);
    match &disk.space {
        Ok(space) => DiskReading {
            name,
            fraction: space.used_fraction(),
            percent: space.used_percent(),
            detail: detail(space),
            nearly_full: space.nearly_full(),
        },
        Err(_) => DiskReading {
            name,
            fraction: None,
            percent: None,
            detail: NOT_MEASURED.to_owned(),
            nearly_full: false,
        },
    }
}

/// What the line under a measured disk's bar says.
fn detail(space: &Space) -> String {
    let holds = guitk::bytes::iec(space.total_bytes());
    if space.read_only {
        format!("{holds}, read-only")
    } else {
        format!("{} free of {holds}", guitk::bytes::iec(space.free_bytes()))
    }
}

/// What the widget has to show.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Disks {
    /// Not read yet: the moment between a widget being put out and its
    /// first reading, drawn as nothing.
    #[default]
    Unread,
    /// The disks, in the mount table's order -- perhaps none.
    Listed(Vec<DiskReading>),
    /// The mount table would not read, and why.
    Unlisted(String),
}

/// What the shell keeps of the disks: the last reading, and when.
#[derive(Clone, Debug)]
pub struct Sampler {
    /// Where the mount table is read: the kernel's (`fs_space::MOUNT_TABLE`),
    /// or a test's own.
    table: PathBuf,
    /// When the last reading was taken, on the session's clock.
    last_ms: Option<u64>,
    /// What it found.
    disks: Disks,
}

impl Default for Sampler {
    fn default() -> Self {
        Self::of_table(PathBuf::from(fs_space::MOUNT_TABLE))
    }
}

impl Sampler {
    /// A sampler that reads the mount table at `table`, and has read
    /// nothing yet.
    #[must_use]
    pub fn of_table(table: PathBuf) -> Self {
        Self {
            table,
            last_ms: None,
            disks: Disks::Unread,
        }
    }

    /// Read the disks at `now_ms` on the session's clock, unless the last
    /// reading was taken less than [`SAMPLE_SPACING_MS`] before; whether it
    /// read.
    pub fn sample(&mut self, now_ms: u64) -> bool {
        if self
            .last_ms
            .is_some_and(|last| now_ms.saturating_sub(last) < SAMPLE_SPACING_MS)
        {
            return false;
        }
        self.last_ms = Some(now_ms);
        self.disks = match fs_space::mounted_disks(&self.table) {
            Ok(found) => Disks::Listed(found.iter().map(reading).collect()),
            Err(e) => Disks::Unlisted(e.to_string()),
        };
        true
    }

    /// What the last reading found.
    #[must_use]
    pub fn disks(&self) -> &Disks {
        &self.disks
    }
}

/// The disks of `disks` placed in `content`, top to bottom: as many as fit,
/// and -- when some do not -- a last line saying how many more.
#[must_use]
pub fn rows(disks: &[DiskReading], content: Rect) -> Rows {
    listing::stack(disks.len(), row_height(), GAP, detail_line(), content)
}

/// Where the bar of a disk laid out at `bounds` is drawn: under its name,
/// the width of its row.
#[must_use]
pub fn bar(bounds: Rect) -> Rect {
    Rect::new(
        bounds.x,
        bounds.y + name_line() + BAR_GAP,
        bounds.w,
        BAR_HEIGHT,
    )
}

/// What the widget says in place of a list, if anything: nothing until
/// the disks are read, that they could not be listed, or that there are
/// none.
#[must_use]
pub fn in_place_of_a_list(disks: &Disks) -> Option<&'static str> {
    match disks {
        Disks::Unread => None,
        Disks::Unlisted(_) => Some(UNLISTED),
        Disks::Listed(listed) if listed.is_empty() => Some(NO_DISKS),
        Disks::Listed(_) => None,
    }
}

/// Draw `disks` in `content`, in `p` at `alpha`: each disk's name and how
/// much of it is in use, its bar -- the disk's colour, as on a system
/// monitor, or red when it is nearly full -- and what is free of what it
/// holds; how many more did not fit; or, in place of a list, why there is
/// none.
pub fn render(
    disks: &Disks,
    content: Rect,
    p: &Palette,
    alpha: u8,
    commands: &mut Vec<RenderCommand>,
) {
    let wash = |c: Color| Color::rgba(c.r, c.g, c.b, alpha);
    let text = |commands: &mut Vec<RenderCommand>,
                (x, y, width): (f32, f32, f32),
                said: String,
                (size, weight): (f32, FontWeightHint),
                ink: Color| {
        commands.push(RenderCommand::Text {
            x,
            y,
            text: said,
            font_size: size,
            color: wash(ink),
            font_weight: weight,
            max_width: Some(width.max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    };
    let detail_font = (DETAIL_SIZE, FontWeightHint::Regular);
    if let Some(said) = in_place_of_a_list(disks) {
        text(
            commands,
            (content.x, content.y, content.w),
            said.to_owned(),
            detail_font,
            p.subtext0,
        );
        return;
    }
    let Disks::Listed(listed) = disks else {
        return;
    };
    let laid = rows(listed, content);
    for &(index, bounds) in &laid.rows {
        let Some(disk) = listed.get(index) else {
            continue;
        };
        // How much is in use at the line's right, the name in what is left.
        let used = disk.percent.map(|percent| format!("{percent}%"));
        let used_w = used.as_deref().map_or(0.0, |said| {
            guitk::text::measure(said, NAME_SIZE, NAME_WEIGHT).min(bounds.w)
        });
        let warning = if disk.nearly_full {
            p.ink(p.red)
        } else {
            p.text
        };
        text(
            commands,
            (bounds.x, bounds.y, bounds.w - used_w - NAME_ROOM),
            disk.name.clone(),
            (NAME_SIZE, NAME_WEIGHT),
            p.text,
        );
        if let Some(said) = used {
            text(
                commands,
                (bounds.x + bounds.w - used_w, bounds.y, used_w),
                said,
                (NAME_SIZE, NAME_WEIGHT),
                warning,
            );
        }
        // The trough either way, as a meter's: an empty trough is a gauge
        // with no needle, which is what a disk not measured is.
        let trough = bar(bounds);
        commands.push(RenderCommand::FillRect {
            x: trough.x,
            y: trough.y,
            width: trough.w,
            height: trough.h,
            color: wash(p.surface1),
            corner_radii: CornerRadii::all(BAR_HEIGHT / 2.0),
        });
        if let Some(fraction) = disk.fraction {
            // The disk's colour on a system monitor, so one disk is one
            // colour on the desktop; red, which no accent changes, when it
            // is nearly full.
            let fill = if disk.nearly_full { p.red } else { p.peach };
            commands.push(RenderCommand::FillRect {
                x: trough.x,
                y: trough.y,
                width: trough.w * fraction.clamp(0.0, 1.0),
                height: trough.h,
                color: wash(fill),
                corner_radii: CornerRadii::all(BAR_HEIGHT / 2.0),
            });
        }
        text(
            commands,
            (bounds.x, trough.y + trough.h + BAR_GAP, bounds.w),
            disk.detail.clone(),
            detail_font,
            p.subtext1,
        );
    }
    if let Some((more, bounds)) = laid.more {
        text(
            commands,
            (bounds.x, bounds.y, bounds.w),
            listing::more_text(more),
            detail_font,
            p.subtext0,
        );
    }
}

#[cfg(test)]
#[path = "disks_tests.rs"]
mod tests;
