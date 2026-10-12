## 1240. A drive's recycle bin is at its top, one for each user, and its limits are kept in it

**Date:** 2026-10-09 · **Decided by:** Claude (operator-approved scope: §1238 chose a bin on every drive with each drive's own limits; the details below are Claude's) · **Lane:** E

**In short:** §1238 said each drive gets a recycle bin of its own, with limits
of its own that go where the drive goes. This records where on the drive the
bin is, whose it is, where its limits are written, and what happens when
something cannot be read. The user sees: deleting from a USB stick is instant
and fills nothing on the system disk; the file manager's Recycle Bin lists
every drive's bin in one list; and opening it applies each drive's limits and
says what that removed.

**Where the bin is.** The home folder's drive keeps `~/.recycle`, where the
one bin always was, so nothing deleted before this is stranded. Every other
drive keeps `<mount point>/.recycle-<uid>`, `<uid>` being the number of the
user who owns the home folder.

| Alternative | Why not |
|---|---|
| `<mount point>/.recycle`, shared by every user | One user's deletions readable by the next person to plug the stick in. |
| `<mount point>/.Trash-<uid>` (the freedesktop name) | Says another desktop's format; ours is `meta.txt` + `data`, not `.trashinfo`, and a freedesktop tool that found the folder would misread it. |
| Everything in the home bin, copied across | What §1238 was decided to end. |

**Whose it is.** A bin found already on a drive is used only if it is a folder,
not a link, and belongs to the user or to the administrator (uid 0). A link
could send what is deleted anywhere; another user's folder could be read by
them. The administrator is accepted because a drive whose files have no
owners -- FAT, which most sticks are -- shows every file as one owner, which
is the administrator's or the mounting user's, and refusing it would mean no
bin on any stick. A new bin is made `0700`.

**Where the limits are.** In the bin, `limits.yaml` (`max_age_days`,
`max_megabytes`, `max_items`), so they go with the drive. A bin without one
keeps to the user's default -- `default_limits` in `recyclebin.yaml` in their
settings -- and a user without one to thirty days, which is what the bin always
claimed and nothing applied: `purge_old` had no caller.

**What cannot be read deletes nothing.** A limit value that is absent, not a
whole number, or zero or less sets no limit of its kind; a `limits.yaml` that
is there but cannot be read gives no limits at all, not the default, because
the drive's own may be looser than the default and the default would then
delete what the user chose to keep; an entry whose record is damaged is never
taken by any limit. Every one of these is a choice of keeping over deleting,
because keeping is the direction a mistake can be undone in.

**Where they are set.** Settings -> System -> Recycle Bin: the default's
three limits, then each drive's bin with what it holds and a "Limits of its
own" switch -- turned on, the drive's limits start as the default's, so
nothing it keeps changes until one is chosen. Each limit is a fixed list
(a week, 30 days, a year; 500 MB, 2 GB; 1000 items ...) rather than a number
to type, like the lock delay beside it; a value written into a file by hand
that is not on the list is offered as it is, so opening the page never
changes what a bin keeps.

**When the limits are applied.** When the file manager opens the Recycle Bin,
to every bin it can reach, and it says what that removed. §1238 also says "when
a drive appears"; nothing in lane E hears that, and the recycle-bin service
(lane D, when it is built) is where it belongs -- `known-issues/E-a-drives-recycle-bin-limits-wait-until-the-bin-is-opened.md`.

**A drive that cannot be told from the home drive.** When the home folder's
drive cannot be found among the mounts (no `/proc/mounts`, or a home that is
not under any of them), every file goes to the home bin, as before: a "drive"
that might be the home drive would otherwise get a second bin at its top.

**Reversal.** Each choice is one function: `Bins::bin_for` and `Bins::claim`
(`apps/recyclebin/src/bins.rs`), `RecycleBin::limits` and `RecycleBin::prune`
(`apps/recyclebin/src/lib.rs`). The on-disk names are constants there.
