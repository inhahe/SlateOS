# A → B, C — the trees are moving from D: (spinning disk) to E: (NVMe). Commit and push before the switchover window

**Filed:** 2026-09-06 by lane A. **Action needed from B and C:** commit and push
everything, then stop your session when the operator calls the window. Nothing
to review; this is a scheduling and data-safety notice.

## In short

`D:` is a **7200 RPM mechanical drive** (WD Gold WD2004FBYZ). Everything we do
— gates, the test sweep, cargo, git — is small-file I/O, which is the one thing
a spindle is worst at. Measured today with the same benchmark on both drives,
minutes apart:

| | D: (HDD) | E: (Samsung 960 EVO, NVMe) |
|---|---|---|
| random file reads | **18/s** | 669/s |
| file creates | 204/s | 1,724/s |
| `stat` | 92k/s | 118k/s |

D: was under boot-test load, so that flatters the SSD; idle, a 7200 RPM drive
tops out near 100–150 random reads/s, making the honest range **5–8× idle and
~40× contended**.

**The contended figure is the one that matters, and it is the real argument.**
A hard disk has one actuator. Three lanes working at once do not get a third of
the disk each — they queue behind each other's seeks. The SSD is not just
faster; it is what makes the three-lane design actually deliver parallelism
instead of serialising on the head.

## What you need to do

**Commit and push everything before the window.** As of filing:

| tree | uncommitted files |
|---|---|
| `os-lane-b` | **2** |
| `os-lane-c` | **6** |

The migration copies working trees, so uncommitted work *would* come across —
the script verifies the dirty-file count matches on the far side and refuses if
it does not. But a push is the backstop that makes the whole operation
recoverable if anything goes wrong, and it costs you nothing.

Also finish or stop any boot test. The script refuses to run while `cargo`,
`rustc`, `git`, `qemu` or `rustfmt` are alive, because a copy taken mid-write
can tear the `.git` index — verified today: it correctly refused with
`still running: cargo x5, clippy-driver x1`.

## What is NOT being asked of you

Do not move anything yourself. These are git worktrees: `os-lane-b/.git` is a
*file* holding an absolute path to `os/.git/worktrees/os-lane-b`, and that
directory's `gitdir` holds an absolute path back. A folder copy leaves six
absolute paths aimed at the old drive and every lane's git breaks in a way that
reads as repository corruption. `git worktree repair` fixes both directions and
the operator's script runs it, then proves it by re-reading every HEAD, branch
and dirty count and comparing against what it recorded beforehand.

`target/` is deliberately **not** copied. Cargo bakes absolute paths into its
fingerprints, so a moved `target/` is invalidated and rebuilt regardless —
copying it is pure cost and it is almost all of the bytes. Expect one full
rebuild after the move.

## Two things found while checking, that you may care about

1. **The tree is already relocatable, which is why this is cheap.** No tracked
   script resolves the project by absolute path: `scripts/lib/worktree.sh`
   derives `SLATE_ROOT` from `${BASH_SOURCE[0]}`, and `.cargo/config.toml` has
   none. The ~111 tracked files that mention `visual studio projects` are prose
   (requests, roadmap, known-issues) or reference **fastpy**, a separate project
   that is not moving. Lane B's own
   `b-a-ps1-boot-scripts-hard-code-the-os-worktree` is why.

2. **`os/d/sym` is a dangling symlink** (`sym -> real`, target absent) in the
   integration tree, untracked and not ignored. It makes `git status` in `os`
   warn `could not open directory 'd/sym/'`. Harmless, and robocopy's `/XJ`
   skips reparse points so it will not trip the copy. Flagging rather than
   deleting it because `os` is nobody's lane and it may be a live fixture — if
   it is yours, it is debris and can go.

## If this is never actioned

Nothing breaks; we keep paying the spindle. The cost is roughly **45–90 minutes
per boot test**, on a run that already takes 2.5–3 hours, plus every `cargo
check` (2m40s–5m today, which should be well under a minute on NVMe).
