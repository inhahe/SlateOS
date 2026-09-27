//! Which snapshots a retention policy keeps.

use std::collections::{BTreeMap, HashSet};
use std::hash::BuildHasher;

use crate::manifest::BackupMeta;

/// How many snapshots to keep, by count and by calendar period. Every limit
/// that is set selects its own snapshots, and a snapshot any of them selects
/// is kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Policy {
    /// The newest this many.
    pub keep_last: Option<u64>,
    /// The newest of each of the last this many days that have one.
    pub keep_daily: Option<u64>,
    /// The newest of each of the last this many weeks that have one.
    pub keep_weekly: Option<u64>,
    /// The newest of each of the last this many calendar months that have one.
    pub keep_monthly: Option<u64>,
}

impl Policy {
    /// Whether no limit is set, which keeps everything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keep_last.is_none()
            && self.keep_daily.is_none()
            && self.keep_weekly.is_none()
            && self.keep_monthly.is_none()
    }
}

/// What a retention policy decided.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Retention {
    /// Every snapshot id that must survive.
    pub keep: HashSet<String>,
    /// Ids the policy did *not* select but a kept snapshot is built on,
    /// reported so a prune that removes fewer than asked can say why.
    pub held_by_chain: Vec<String>,
    /// `(child, missing parent)` for every kept snapshot built on one that is
    /// gone. Those cannot be restored whatever the prune does, so they are
    /// reported rather than silently kept as if they were data.
    pub broken_chains: Vec<(String, String)>,
}

/// Decide what to keep.
///
/// `standalone` names the snapshots whose manifests list every file (all of
/// them from manifest version 3 on): those restore without their parent, so
/// keeping one does not keep its chain. A snapshot not in it -- an
/// incremental or differential one written before version 3 -- holds only
/// what changed, and keeping it keeps everything it is built on.
#[must_use]
pub fn compute_retention<S: BuildHasher>(
    metas: &[BackupMeta],
    policy: &Policy,
    standalone: &HashSet<String, S>,
) -> Retention {
    let mut keep = retention_by_policy(metas, policy);
    let (held_by_chain, broken_chains) = keep_ancestors(metas, &mut keep, standalone);
    Retention {
        keep,
        held_by_chain,
        broken_chains,
    }
}

/// Add to `keep` every snapshot a kept one is built on, returning the ids
/// added and the broken links found.
///
/// An incremental snapshot from before manifest version 3 stores only the
/// difference from its parent: restoring one walks `parent_id` back to a full
/// snapshot. The policies select purely by *age*, and for the usual
/// full-then-many-incrementals chain that selects the newest and drops the
/// full base they all depend on -- so `backup prune --keep-last 3` used to
/// delete the one backup that made the three it kept restorable, and then
/// garbage-collect its file contents too.
///
/// A `parent_id` naming a snapshot that is not in `metas` is *not* added:
/// keeping a phantom id would say we hold a snapshot that does not exist. It
/// is returned as a broken link, because its child cannot be restored either
/// way.
fn keep_ancestors<S: BuildHasher>(
    metas: &[BackupMeta],
    keep: &mut HashSet<String>,
    standalone: &HashSet<String, S>,
) -> (Vec<String>, Vec<(String, String)>) {
    let parents: BTreeMap<&str, &str> = metas
        .iter()
        .filter_map(|m| m.parent_id.as_deref().map(|p| (m.id.as_str(), p)))
        .collect();
    let known: HashSet<&str> = metas.iter().map(|m| m.id.as_str()).collect();
    let mut added = Vec::new();
    let mut broken = Vec::new();
    let mut pending: Vec<String> = keep.iter().cloned().collect();
    while let Some(id) = pending.pop() {
        if standalone.contains(&id) {
            // Every file is in its own manifest: it needs nothing else.
            continue;
        }
        let Some(parent) = parents.get(id.as_str()) else {
            continue;
        };
        if !known.contains(*parent) {
            broken.push((id, (*parent).to_string()));
            continue;
        }
        // `insert` returning false is also what ends a `parent_id` cycle in a
        // hand-edited or corrupt set of records.
        if keep.insert((*parent).to_string()) {
            added.push((*parent).to_string());
            pending.push((*parent).to_string());
        }
    }
    added.sort();
    broken.sort();
    (added, broken)
}

/// The day, from the epoch, that `timestamp` falls on.
fn day_of(timestamp: u64) -> u64 {
    timestamp / 86_400
}

/// The calendar month `timestamp` falls in, as `year * 12 + month`.
///
/// It was `timestamp / (86400 * 30)`, "approximate": thirty-day blocks that
/// drift across the calendar, so the first and the last of one January could
/// land in two "months" while a December snapshot shared one with a January
/// one. `tzrules::civil_from_days` is the calendar.
fn month_of(timestamp: u64) -> i64 {
    let days = i64::try_from(day_of(timestamp)).unwrap_or(i64::MAX);
    let (year, month, _) = tzrules::civil_from_days(days);
    year.saturating_mul(12).saturating_add(i64::from(month))
}

/// Apply the age-based policies, ignoring chains.
fn retention_by_policy(metas: &[BackupMeta], policy: &Policy) -> HashSet<String> {
    let mut keep = HashSet::new();
    if policy.is_empty() {
        keep.extend(metas.iter().map(|m| m.id.clone()));
        return keep;
    }
    let newest_first = || metas.iter().rev();
    if let Some(n) = policy.keep_last {
        let n = usize::try_from(n).unwrap_or(usize::MAX);
        keep.extend(newest_first().take(n).map(|m| m.id.clone()));
    }
    // One per period: the newest in each of the last `n` periods that have
    // one. Generic over the period's key so the three cannot drift apart.
    let mut per_period = |n: Option<u64>, period: &dyn Fn(u64) -> i64| {
        let Some(n) = n else { return };
        let n = usize::try_from(n).unwrap_or(usize::MAX);
        let mut seen = HashSet::new();
        for meta in newest_first() {
            if seen.len() >= n {
                break;
            }
            if seen.insert(period(meta.timestamp)) {
                keep.insert(meta.id.clone());
            }
        }
    };
    per_period(policy.keep_daily, &|t| {
        i64::try_from(day_of(t)).unwrap_or(i64::MAX)
    });
    per_period(policy.keep_weekly, &|t| {
        i64::try_from(day_of(t) / 7).unwrap_or(i64::MAX)
    });
    per_period(policy.keep_monthly, &month_of);
    keep
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::manifest::BackupType;
    use std::path::PathBuf;

    fn meta(id: &str, parent: Option<&str>, timestamp: u64) -> BackupMeta {
        BackupMeta {
            id: id.to_string(),
            backup_type: if parent.is_some() {
                BackupType::Incremental
            } else {
                BackupType::Full
            },
            timestamp,
            source: PathBuf::from("/src"),
            parent_id: parent.map(str::to_string),
            file_count: 0,
            total_size: 0,
            new_blobs: 0,
            dedup_blobs: 0,
            unread_count: 0,
        }
    }

    fn last(n: u64) -> Policy {
        Policy {
            keep_last: Some(n),
            ..Policy::default()
        }
    }

    /// Nothing is standalone: the chains of manifests before version 3.
    fn legacy() -> HashSet<String> {
        HashSet::new()
    }

    #[test]
    fn keep_last_keeps_the_newest() {
        let metas = [
            meta("1", None, 100),
            meta("2", None, 200),
            meta("3", None, 300),
        ];
        let keep = compute_retention(&metas, &last(2), &legacy()).keep;
        assert!(!keep.contains("1"));
        assert!(keep.contains("2") && keep.contains("3"));
    }

    /// The newest of each of the last two days that have one.
    #[test]
    fn keep_daily_keeps_the_newest_of_each_day() {
        let day = 86_400u64;
        let metas = [
            meta("d1_a", None, day * 10 + 100),
            meta("d1_b", None, day * 10 + 200),
            meta("d2_a", None, day * 11 + 100),
            meta("d3_a", None, day * 12 + 100),
        ];
        let policy = Policy {
            keep_daily: Some(2),
            ..Policy::default()
        };
        let keep = compute_retention(&metas, &policy, &legacy()).keep;
        assert!(keep.contains("d3_a") && keep.contains("d2_a"));
        assert!(!keep.contains("d1_a") && !keep.contains("d1_b"));
    }

    #[test]
    fn no_policy_keeps_everything() {
        let metas = [meta("1", None, 100), meta("2", None, 200)];
        let keep = compute_retention(&metas, &Policy::default(), &legacy()).keep;
        assert!(keep.contains("1") && keep.contains("2"));
    }

    /// Months are the calendar's. Thirty-day blocks put 2026-01-01 and
    /// 2025-12-15 in one "month" and 2026-01-31 in the next, so two months
    /// kept Jan 31 and Jan 1 and dropped December.
    #[test]
    fn keep_monthly_counts_calendar_months() {
        let day = 86_400u64;
        let metas = [
            meta("dec15", None, 20_437 * day),
            meta("jan01", None, 20_454 * day),
            meta("jan31", None, 20_484 * day),
        ];
        let policy = Policy {
            keep_monthly: Some(2),
            ..Policy::default()
        };
        let keep = compute_retention(&metas, &policy, &legacy()).keep;
        assert!(keep.contains("jan31"), "January's newest");
        assert!(keep.contains("dec15"), "December's");
        assert!(!keep.contains("jan01"), "January is already represented");
    }

    /// One full, then incrementals, from before manifest version 3. Keeping
    /// the newest two used to delete the full backup they are both
    /// differences *from*, leaving two that could not be restored.
    #[test]
    fn a_legacy_chain_keeps_the_full_backup_the_kept_ones_are_built_on() {
        let metas = [
            meta("full", None, 100),
            meta("inc1", Some("full"), 200),
            meta("inc2", Some("inc1"), 300),
            meta("inc3", Some("inc2"), 400),
        ];
        let retention = compute_retention(&metas, &last(2), &legacy());
        for id in ["inc3", "inc2", "inc1", "full"] {
            assert!(retention.keep.contains(id), "{id}");
        }
        assert_eq!(
            retention.held_by_chain,
            vec!["full".to_string(), "inc1".to_string()],
            "the two the policy dropped and the chain reinstated are reported"
        );
    }

    /// From version 3 every manifest is complete, and a kept incremental
    /// needs nothing it was taken against: the policy applies as written.
    #[test]
    fn a_standalone_snapshot_does_not_hold_its_parent() {
        let metas = [
            meta("full", None, 100),
            meta("inc1", Some("full"), 200),
            meta("inc2", Some("inc1"), 300),
        ];
        let standalone: HashSet<String> = ["full", "inc1", "inc2"]
            .iter()
            .map(ToString::to_string)
            .collect();
        let retention = compute_retention(&metas, &last(1), &standalone);
        assert!(retention.keep.contains("inc2"));
        assert!(!retention.keep.contains("inc1") && !retention.keep.contains("full"));
        assert!(retention.held_by_chain.is_empty());
    }

    #[test]
    fn independent_full_backups_are_pruned_as_the_policy_says() {
        let metas = [
            meta("f1", None, 100),
            meta("f2", None, 200),
            meta("f3", None, 300),
        ];
        let retention = compute_retention(&metas, &last(2), &legacy());
        assert!(!retention.keep.contains("f1"));
        assert!(retention.keep.contains("f2") && retention.keep.contains("f3"));
        assert!(retention.held_by_chain.is_empty());
    }

    #[test]
    fn an_ancestor_of_nothing_kept_is_still_pruned() {
        let metas = [
            meta("oldfull", None, 100),
            meta("oldinc", Some("oldfull"), 150),
            meta("newfull", None, 300),
            meta("newinc", Some("newfull"), 400),
        ];
        let retention = compute_retention(&metas, &last(1), &legacy());
        assert!(retention.keep.contains("newinc") && retention.keep.contains("newfull"));
        assert!(!retention.keep.contains("oldfull") && !retention.keep.contains("oldinc"));
    }

    /// Corrupt or hand-edited records can name a parent that is a
    /// descendant. Walking that must end.
    #[test]
    fn a_cycle_in_the_parent_links_does_not_hang_the_prune() {
        let metas = [meta("a", Some("b"), 100), meta("b", Some("a"), 200)];
        let retention = compute_retention(&metas, &last(1), &legacy());
        assert!(retention.keep.contains("a") && retention.keep.contains("b"));
    }

    #[test]
    fn a_parent_that_is_already_gone_is_not_invented() {
        let metas = [meta("orphan", Some("vanished"), 100)];
        let retention = compute_retention(&metas, &last(1), &legacy());
        assert!(retention.keep.contains("orphan"));
        assert!(!retention.keep.contains("vanished"));
        assert!(retention.held_by_chain.is_empty());
        assert_eq!(
            retention.broken_chains,
            vec![("orphan".to_string(), "vanished".to_string())]
        );
    }
}
