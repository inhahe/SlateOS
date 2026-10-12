//! How long a recycle bin keeps what is in it: each drive's own limits.
//!
//! The operator's answer to E-Q4 (design-decisions §1238): every drive's bin
//! has limits of its own -- an age, a size, a number of items -- kept on that
//! drive, in the bin ([`FILE_NAME`]), so that they travel with it: a stick set
//! to keep a week keeps a week on any machine it is plugged into. A bin with
//! no file of its own keeps to the user's default ([`Limits::user_default`],
//! `recyclebin.yaml` in their settings), and a user who has set none to
//! [`Limits::default`] -- thirty days, which is what the bin always said it
//! did and never did: its ageing had no caller.
//!
//! The limits are applied by [`RecycleBin::prune`](crate::RecycleBin::prune),
//! from the deletion time recorded with each item rather than from any clock
//! of the bin's own, so a stick that was away for a month is pruned the moment
//! it is back.
//!
//! # Reading a limit
//!
//! A limit is a promise to delete. So a value that cannot be read -- absent,
//! not a whole number, zero or less -- sets *no* limit of its kind, never a
//! guessed one: "keep" is the direction a mistake can be undone in.

use std::time::Duration;

/// The file in a bin that holds its drive's limits.
pub const FILE_NAME: &str = "limits.yaml";

/// The user's settings file (`settingsfile`) holding their default limits,
/// under [`USER_SECTION`].
pub const USER_SETTINGS: &str = "recyclebin";

/// The section of [`USER_SETTINGS`] that holds the default limits.
pub const USER_SECTION: &str = "default_limits";

/// The key of the age limit, in whole days.
const AGE_KEY: &str = "max_age_days";
/// The key of the size limit, in whole mebibytes.
const SIZE_KEY: &str = "max_megabytes";
/// The key of the item limit.
const COUNT_KEY: &str = "max_items";

/// Seconds in a day.
const DAY_SECS: u64 = 24 * 60 * 60;
/// Bytes in a mebibyte, which is what a size limit is set in.
const MEGABYTE: u64 = 1024 * 1024;

/// What a bin may keep. Each limit is optional; a bin with none keeps
/// everything until it is emptied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Items deleted longer ago than this go.
    pub max_age: Option<Duration>,
    /// The bin is kept to this many bytes, the oldest items going first.
    pub max_bytes: Option<u64>,
    /// And to this many items, likewise.
    pub max_items: Option<u32>,
}

impl Default for Limits {
    /// Thirty days, and no limit on size or number.
    fn default() -> Self {
        Self {
            max_age: Some(Duration::from_secs(30 * DAY_SECS)),
            max_bytes: None,
            max_items: None,
        }
    }
}

impl Limits {
    /// No limits at all: everything is kept until the bin is emptied.
    pub const NONE: Self = Self {
        max_age: None,
        max_bytes: None,
        max_items: None,
    };

    /// The limits `doc` sets under `section` (the document's top level for
    /// an empty one): `max_age_days`, `max_megabytes` and `max_items`, each a
    /// whole number above zero. A key that is absent or cannot be read sets no
    /// limit of its kind.
    #[must_use]
    pub fn read(doc: &yamldoc::Document, section: &[&str]) -> Self {
        let whole = |key: &str| {
            let mut path = section.to_vec();
            path.push(key);
            doc.get_i64(&path)
                .and_then(|n| u64::try_from(n).ok())
                .filter(|n| *n > 0)
        };
        Self {
            max_age: whole(AGE_KEY)
                .and_then(|days| days.checked_mul(DAY_SECS))
                .map(Duration::from_secs),
            max_bytes: whole(SIZE_KEY).and_then(|mb| mb.checked_mul(MEGABYTE)),
            max_items: whole(COUNT_KEY).and_then(|n| u32::try_from(n).ok()),
        }
    }

    /// Write these limits into `doc` under `section`, leaving everything else
    /// in it -- a user's comments among it -- as it was. A limit that is not
    /// set is removed. An age is written in whole days and a size in whole
    /// mebibytes, rounded up, so a limit read back is never tighter than the
    /// one written.
    pub fn write(&self, doc: &mut yamldoc::Document, section: &[&str]) {
        let path = |key: &'static str| {
            let mut path = section.to_vec();
            path.push(key);
            path
        };
        let set = |doc: &mut yamldoc::Document, key: &'static str, value: Option<u64>| {
            let at = path(key);
            match value.and_then(|v| i64::try_from(v).ok()) {
                Some(v) => doc.set_i64(&at, v),
                None => {
                    // Whether it was there does not matter: it is not now.
                    let _was_there = doc.remove(&at);
                }
            }
        };
        set(
            doc,
            AGE_KEY,
            self.max_age.map(|age| age.as_secs().div_ceil(DAY_SECS)),
        );
        set(doc, SIZE_KEY, self.max_bytes.map(|b| b.div_ceil(MEGABYTE)));
        set(doc, COUNT_KEY, self.max_items.map(u64::from));
    }

    /// The user's own default, for a drive whose bin sets none: the
    /// `default_limits` section of `recyclebin.yaml` in their settings, or
    /// [`Limits::default`] when they have not written one.
    #[must_use]
    pub fn user_default() -> Self {
        let doc = settingsfile::load(USER_SETTINGS);
        if doc.keys(&[USER_SECTION]).is_empty() {
            Self::default()
        } else {
            Self::read(&doc, &[USER_SECTION])
        }
    }

    /// Keep `self` as the user's default.
    ///
    /// # Errors
    ///
    /// As `settingsfile::store`.
    pub fn store_as_user_default(&self) -> std::io::Result<()> {
        let mut doc = settingsfile::load(USER_SETTINGS);
        self.write(&mut doc, &[USER_SECTION]);
        // A section with every limit removed would read back as "no section",
        // which is the built-in default -- not the "keep everything" the user
        // chose. An explicit zero cannot be written (it reads as no limit,
        // above), so "none" is a key of its own.
        if *self == Self::NONE {
            doc.set_bool(&[USER_SECTION, "keep_everything"], true);
        } else {
            let _was_there = doc.remove(&[USER_SECTION, "keep_everything"]);
        }
        settingsfile::store(USER_SETTINGS, &doc)
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    #[test]
    fn limits_read_back_as_written() {
        let limits = Limits {
            max_age: Some(Duration::from_secs(7 * DAY_SECS)),
            max_bytes: Some(512 * MEGABYTE),
            max_items: Some(200),
        };
        let mut doc = yamldoc::Document::parse("# This stick's bin.\nmax_items: 5\n");
        limits.write(&mut doc, &[]);
        let text = doc.to_text();
        assert!(text.starts_with("# This stick's bin.\n"), "{text}");
        assert!(text.contains("max_items: 200"), "{text}");
        assert!(!text.contains("max_items: 5"), "{text}");
        assert!(text.contains("max_age_days: 7"), "{text}");
        assert_eq!(Limits::read(&yamldoc::Document::parse(&text), &[]), limits);

        // A limit taken away goes from the file.
        let fewer = Limits {
            max_bytes: None,
            ..limits
        };
        fewer.write(&mut doc, &[]);
        assert!(!doc.to_text().contains("max_megabytes"));
        assert_eq!(Limits::read(&doc, &[]), fewer);
    }

    #[test]
    fn a_limit_that_cannot_be_read_is_no_limit() {
        let doc = yamldoc::Document::parse("max_age_days: 0\nmax_megabytes: lots\nmax_items: -5\n");
        assert_eq!(Limits::read(&doc, &[]), Limits::NONE);
        assert_eq!(
            Limits::read(&yamldoc::Document::parse(""), &[]),
            Limits::NONE
        );
    }

    #[test]
    fn a_limit_is_written_whole_and_never_tighter() {
        let limits = Limits {
            max_age: Some(Duration::from_secs(DAY_SECS + 1)),
            max_bytes: Some(MEGABYTE + 1),
            max_items: None,
        };
        let mut doc = yamldoc::Document::parse("");
        limits.write(&mut doc, &["default_limits"]);
        let back = Limits::read(&doc, &["default_limits"]);
        assert_eq!(back.max_age, Some(Duration::from_secs(2 * DAY_SECS)));
        assert_eq!(back.max_bytes, Some(2 * MEGABYTE));
    }

    #[test]
    fn the_users_default_is_thirty_days_until_they_set_one() {
        settingsfile::testing::with_scratch_config("recyclebin_default", |_| {
            assert_eq!(Limits::user_default(), Limits::default());
            let week = Limits {
                max_age: Some(Duration::from_secs(7 * DAY_SECS)),
                ..Limits::NONE
            };
            week.store_as_user_default().unwrap();
            assert_eq!(Limits::user_default(), week);
            Limits::NONE.store_as_user_default().unwrap();
            assert_eq!(
                Limits::user_default(),
                Limits::NONE,
                "keeping everything read back as the built-in thirty days"
            );
        });
    }
}
