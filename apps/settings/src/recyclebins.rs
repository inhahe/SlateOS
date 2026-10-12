//! The Recycle Bin page's model: every drive's bin, and how long each keeps
//! what is deleted (design-decisions §1238, §1240).
//!
//! Reading, writing and applying the limits are `recyclebin`'s; this is what
//! the page offers and how it words it. A drive's own limits are written in
//! its bin, so they go with the drive; the user's default -- `recyclebin.yaml`
//! in their settings -- is for any drive without its own, and the file
//! manager applies them when its Recycle Bin is opened.
//!
//! # Fixed choices
//!
//! A limit is a policy the user picks, like the lock delay on the page beside
//! it, so the page offers a list rather than a number to type. A limit
//! somebody wrote into a file by hand that is not on the list is offered as
//! well, as it is, so that opening the page never changes what a bin keeps.

use std::time::Duration;

use recyclebin::{Bins, DriveBin, Limits, Usage};

/// Seconds in a day.
const DAY: u64 = 24 * 60 * 60;
/// Bytes in a mebibyte, the unit a size limit is kept in.
const MEGABYTE: u64 = 1024 * 1024;

/// The ages offered, in days. `None` is no age limit.
const AGE_DAYS: [Option<u64>; 8] = [
    None,
    Some(1),
    Some(7),
    Some(14),
    Some(30),
    Some(60),
    Some(90),
    Some(365),
];
/// The sizes offered, in mebibytes.
const SIZE_MEGABYTES: [Option<u64>; 8] = [
    None,
    Some(100),
    Some(500),
    Some(1024),
    Some(2048),
    Some(5 * 1024),
    Some(10 * 1024),
    Some(50 * 1024),
];
/// The numbers of items offered.
const COUNTS: [Option<u64>; 6] = [
    None,
    Some(100),
    Some(500),
    Some(1000),
    Some(5000),
    Some(10_000),
];

/// One of a bin's three limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitKind {
    /// How long an item is kept.
    Age,
    /// How big the bin may grow.
    Size,
    /// How many items it may hold.
    Count,
}

impl LimitKind {
    /// The three, in the order the page shows them.
    pub const ALL: [Self; 3] = [Self::Age, Self::Size, Self::Count];

    /// The row's label; its value finishes the sentence.
    #[must_use]
    pub fn row_label(self) -> &'static str {
        match self {
            Self::Age => "Delete items after",
            Self::Size => "Keep the bin under",
            Self::Count => "Keep at most",
        }
    }

    /// This limit of `limits`, in base units: seconds, bytes, items.
    #[must_use]
    pub fn get(self, limits: &Limits) -> Option<u64> {
        match self {
            Self::Age => limits.max_age.map(|age| age.as_secs()),
            Self::Size => limits.max_bytes,
            Self::Count => limits.max_items.map(u64::from),
        }
    }

    /// Set this limit of `limits` to `value`, in base units.
    pub fn set(self, limits: &mut Limits, value: Option<u64>) {
        match self {
            Self::Age => limits.max_age = value.map(Duration::from_secs),
            Self::Size => limits.max_bytes = value,
            Self::Count => limits.max_items = value.and_then(|n| u32::try_from(n).ok()),
        }
    }

    /// The values offered, in base units, smallest first after "none".
    #[must_use]
    pub fn offered(self) -> Vec<Option<u64>> {
        let scaled = |list: &[Option<u64>], unit: u64| -> Vec<Option<u64>> {
            list.iter()
                .map(|v| v.and_then(|n| n.checked_mul(unit)))
                .collect()
        };
        match self {
            Self::Age => scaled(&AGE_DAYS, DAY),
            Self::Size => scaled(&SIZE_MEGABYTES, MEGABYTE),
            Self::Count => COUNTS.to_vec(),
        }
    }

    /// What the dropdown lists for `limits`' value of this limit, and which
    /// of them it is: the values offered, and the current value among them in
    /// its place if it is not one of them.
    #[must_use]
    pub fn options(self, limits: &Limits) -> (Vec<Option<u64>>, usize) {
        let mut values = self.offered();
        let current = self.get(limits);
        if let Some(at) = values.iter().position(|v| *v == current) {
            return (values, at);
        }
        // Not offered, so somebody wrote it by hand: shown where it falls,
        // after every smaller value.
        let at = values
            .iter()
            .position(|v| v.is_some_and(|v| current.is_some_and(|c| v > c)))
            .unwrap_or(values.len());
        values.insert(at, current);
        (values, at)
    }

    /// How a value of this limit reads on the page.
    #[must_use]
    pub fn label(self, value: Option<u64>) -> String {
        match (self, value) {
            (Self::Age, None) => "Never".to_string(),
            (Self::Age, Some(secs)) => match secs.div_ceil(DAY) {
                1 => "1 day".to_string(),
                7 => "1 week".to_string(),
                14 => "2 weeks".to_string(),
                365 => "1 year".to_string(),
                days => format!("{days} days"),
            },
            (Self::Size, None) => "Any size".to_string(),
            (Self::Size, Some(bytes)) => {
                let megabytes = bytes.div_ceil(MEGABYTE);
                if megabytes >= 1024 && megabytes % 1024 == 0 {
                    format!("{} GB", megabytes / 1024)
                } else {
                    format!("{megabytes} MB")
                }
            }
            (Self::Count, None) => "Any number".to_string(),
            (Self::Count, Some(1)) => "1 item".to_string(),
            (Self::Count, Some(n)) => format!("{n} items"),
        }
    }
}

/// A drive's bin as the page shows it.
#[derive(Clone, Debug)]
pub struct DriveRow {
    /// The drive, and its bin.
    pub drive: DriveBin,
    /// Its own limits: `Ok(None)` when it keeps to the default, and the
    /// reason when its limits file cannot be read -- which keeps everything
    /// (`RecycleBin::limits`), and is said so.
    pub own: Result<Option<Limits>, String>,
    /// What it holds, or why that could not be read.
    pub usage: Result<Usage, String>,
}

impl DriveRow {
    /// `drive`, read now.
    #[must_use]
    pub fn read(drive: DriveBin) -> Self {
        let own = drive.bin.own_limits().map_err(|e| e.to_string());
        let usage = drive.bin.usage().map_err(|e| e.to_string());
        Self { drive, own, usage }
    }

    /// What it holds, in words.
    #[must_use]
    pub fn holds(&self) -> String {
        match &self.usage {
            Ok(usage) if usage.items == 0 => "Nothing".to_string(),
            Ok(usage) => format!(
                "{} {}, {}",
                usage.items,
                if usage.items == 1 { "item" } else { "items" },
                guitk::bytes::iec(usage.bytes)
            ),
            Err(e) => format!("Could not be read: {e}"),
        }
    }
}

/// Every bin there is now, read.
#[must_use]
pub fn drive_rows(bins: &Bins) -> Vec<DriveRow> {
    bins.reachable().into_iter().map(DriveRow::read).collect()
}
