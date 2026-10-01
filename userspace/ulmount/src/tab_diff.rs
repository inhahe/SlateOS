//! libmount's `tab_diff.c`: what changed between two readings of the
//! kernel's mount table -- what `findmnt --poll` reports.
//!
//! An entry of the new table with no entry of the old one for the same
//! source and mount point was mounted; one whose options differ was
//! remounted; an entry of the old table missing from the new one was
//! unmounted -- or moved, when a new mount has its ID and its source.
//!
//! Upstream's quirk that an entry with an empty source matches nothing
//! (`mnt_table_find_pair` refuses an empty source) is kept: such a mount
//! is "mounted" in every new reading, and so "moved" from the old one,
//! whenever the table is compared at all.

use crate::tab::{Direction, Table};

/// `MNT_TABDIFF_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Mount = 1,
    Umount = 2,
    Move = 3,
    Remount = 4,
}

/// `struct tabdiff_entry`: a change, with the entry of each table it
/// concerns (indices into the old and the new table).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffEntry {
    pub oper: Change,
    pub old: Option<usize>,
    pub new: Option<usize>,
}

/// `tabdiff_get_mount(df, src, id)`: the "mount" change whose new entry has
/// the ID and exactly the source.
fn get_mount(
    changes: &mut [DiffEntry],
    new_tab: &Table,
    src: Option<&[u8]>,
    id: i32,
) -> Option<usize> {
    changes.iter().position(|de| {
        de.oper == Change::Mount
            && de
                .new
                .and_then(|n| new_tab.ents.get(n))
                .is_some_and(|fs| fs.id == id && fs.source.as_deref() == src)
    })
}

/// `mnt_diff_tables(df, old_tab, new_tab)`: the changes from `old_tab` to
/// `new_tab`, in upstream's order -- mounts and remounts in the new table's
/// order, then unmounts in the old one's (a move replaces the mount it
/// matched, in place).
///
/// The tables' caches are not consulted: `findmnt` clears them before
/// polling, and a table here does not carry one.
#[must_use]
pub fn diff_tables(old_tab: &Table, new_tab: &Table) -> Vec<DiffEntry> {
    let mut changes: Vec<DiffEntry> = Vec::new();
    let (no, nn) = (old_tab.nents(), new_tab.nents());
    if no == 0 && nn == 0 {
        return changes;
    }
    if no == 0 {
        for i in 0..nn {
            changes.push(DiffEntry {
                oper: Change::Mount,
                old: None,
                new: Some(i),
            });
        }
        return changes;
    }
    if nn == 0 {
        for i in 0..no {
            changes.push(DiffEntry {
                oper: Change::Umount,
                old: Some(i),
                new: None,
            });
        }
        return changes;
    }
    // Newly mounted or modified.
    for (i, fs) in new_tab.ents.iter().enumerate() {
        match old_tab.find_pair(
            fs.source.as_deref(),
            fs.target.as_deref(),
            Direction::Forward,
            None,
        ) {
            None => changes.push(DiffEntry {
                oper: Change::Mount,
                old: None,
                new: Some(i),
            }),
            Some(o) => {
                let Some(ofs) = old_tab.ents.get(o) else {
                    continue;
                };
                let differs = |a: &Option<Vec<u8>>, b: &Option<Vec<u8>>| matches!((a, b), (Some(x), Some(y)) if x != y);
                if differs(&ofs.vfs_optstr, &fs.vfs_optstr)
                    || differs(&ofs.fs_optstr, &fs.fs_optstr)
                {
                    changes.push(DiffEntry {
                        oper: Change::Remount,
                        old: Some(o),
                        new: Some(i),
                    });
                }
            }
        }
    }
    // Unmounted or moved.
    for (o, fs) in old_tab.ents.iter().enumerate() {
        if new_tab
            .find_pair(
                fs.source.as_deref(),
                fs.target.as_deref(),
                Direction::Forward,
                None,
            )
            .is_some()
        {
            continue;
        }
        match get_mount(&mut changes, new_tab, fs.source.as_deref(), fs.id)
            .and_then(|k| changes.get_mut(k))
        {
            Some(de) => {
                de.oper = Change::Move;
                de.old = Some(o);
            }
            None => changes.push(DiffEntry {
                oper: Change::Umount,
                old: Some(o),
                new: None,
            }),
        }
    }
    changes
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    reason = "tests index tables they have just built"
)]
mod tests {
    use super::*;
    use crate::fs::{Fs, MNT_FS_KERNEL};
    use crate::tab::Fmt;

    fn fs(id: i32, src: &str, target: &str, vfs: &str) -> Fs {
        let mut f = Fs {
            id,
            parent: 1,
            target: Some(target.as_bytes().to_vec()),
            root: Some(b"/".to_vec()),
            vfs_optstr: Some(vfs.as_bytes().to_vec()),
            fs_optstr: Some(b"rw".to_vec()),
            flags: MNT_FS_KERNEL,
            ..Fs::default()
        };
        f.set_source(Some(src.as_bytes().to_vec()));
        f
    }

    fn table(ents: Vec<Fs>) -> Table {
        Table {
            fmt: Fmt::Mountinfo,
            ents,
        }
    }

    #[test]
    fn changes_are_found_in_upstreams_order() {
        let old = table(vec![
            fs(1, "/dev/a", "/", "rw"),
            fs(2, "/dev/b", "/b", "rw"),
            fs(3, "/dev/c", "/c", "rw"),
            fs(4, "/dev/d", "/d", "rw"),
        ]);
        let new = table(vec![
            fs(1, "/dev/a", "/", "ro"),
            fs(3, "/dev/c", "/c", "rw"),
            fs(4, "/dev/d", "/moved", "rw"),
            fs(5, "/dev/e", "/e", "rw"),
        ]);
        let d = diff_tables(&old, &new);
        assert_eq!(
            d,
            vec![
                DiffEntry {
                    oper: Change::Remount,
                    old: Some(0),
                    new: Some(0)
                },
                DiffEntry {
                    oper: Change::Move,
                    old: Some(3),
                    new: Some(2)
                },
                DiffEntry {
                    oper: Change::Mount,
                    old: None,
                    new: Some(3)
                },
                DiffEntry {
                    oper: Change::Umount,
                    old: Some(1),
                    new: None
                },
            ]
        );
    }

    #[test]
    fn empty_tables_are_all_mounts_or_all_unmounts() {
        let t = table(vec![
            fs(1, "/dev/a", "/", "rw"),
            fs(2, "/dev/b", "/b", "rw"),
        ]);
        let e = table(Vec::new());
        assert!(diff_tables(&e, &e).is_empty());
        assert!(diff_tables(&e, &t).iter().all(|d| d.oper == Change::Mount));
        assert!(diff_tables(&t, &e).iter().all(|d| d.oper == Change::Umount));
    }

    #[test]
    fn an_empty_source_is_moved_every_time() {
        let t = table(vec![fs(1, "/dev/a", "/", "rw"), fs(7, "", "/x", "rw")]);
        let d = diff_tables(&t, &t.clone());
        assert_eq!(
            d,
            vec![DiffEntry {
                oper: Change::Move,
                old: Some(1),
                new: Some(1)
            }]
        );
    }
}
