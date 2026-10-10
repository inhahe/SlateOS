## TD-B-RM-ONE-FILE-SYSTEM-AND-PRESERVE-ROOT-ALL-ARE-IMPLEMENTED-BUT-UNCERTIFIED (lane B, 2026-08-30)

**Status:** RESOLVED -- fixed 2026-10-03 (lane B), boot-confirmed on `main` 2026-10-07: the fix is in `8a7cdaf26`, whose boot passed and which was published to `main` as `73d857e1b` -- certified,
and nothing needed changing. `scripts/rm-diff.sh` section 16b does what this
entry proposed: each side runs in its own `unshare -mUr` namespace with
`tree/sub` a tmpfs mount, and the program's output, its status and what is
left of the tree (mount included) are taken inside the namespace, before the
mount vanishes with it. Fifteen cases -- `--one-file-system` across the mount
and on the mount point itself, `--preserve-root=all` on a mount-point operand,
a plain operand, a tree holding one, and alongside `--one-file-system`,
`--preserve-root` and `--no-preserve-root`, with and without `-r` and `-d` --
all agree with GNU 9.4: 211 of 211. Checked by hand that the reference does
meet the mount (`skipping 'tree/sub', since it's on a different device`,
`Device or resource busy`, `and --preserve-root=all is in effect`), so the
agreement is not two programs that never saw it.

*The entry as filed:*

**In short:** `rm` has two options that only mean anything when a *mount point*
is involved — a place in the directory tree where a second disk (or a second
filesystem of any kind) is attached. `--one-file-system` says "delete this
tree, but stop at any place where a different disk begins"; `--preserve-root=all`
says "refuse outright to delete a directory that is such a place". Both are
implemented, and both are covered by unit tests that exercise the code paths.
Neither has been compared against GNU's `rm` the way every other option has,
because creating a mount point requires privileges the test harness does not
have and should not be given. Nothing is known to be wrong; what is missing is
the evidence that nothing is.

**Where.** `userspace/coreutils/src/bin/rm.rs` — `Rm::crosses_into_its_parent`
(that is `--preserve-root=all`) and the `level > 0 && self.options.one_file_system`
branch at the top of `Rm::entry`. The device number both of them compare comes
from `device_of`, which is `st_dev` on unix and `None` everywhere else.

**Why it could not be measured.** `scripts/rm-diff.sh` runs both binaries inside
WSL as an ordinary user. `mount -t tmpfs none dir` there fails with `Operation
not permitted`, and there is no pre-existing mount point inside a directory the
harness may destroy — which is what these two options need, since both are about
what happens when a *descendant* of the operand is on a different filesystem
from the operand. Every other option in the program is certified case by case
against GNU coreutils 9.4 by that harness.

**What would close it.** `unshare --map-root-user --mount` gives an unprivileged
process a mount namespace in which it may mount a tmpfs, and WSL2 supports it.
A section of `rm-diff.sh` guarded on `unshare -r -m true` succeeding could build
`tree/sub` as a tmpfs mount and then run the same case on both sides:

    rm -rv --one-file-system tree     # sub is skipped, tree/rmdir then fails
    rm -rv tree                       # sub is descended into and removed
    rm -rf --preserve-root=all tree/sub

That is the right fix and it is not large. It was left out of the first version
of the harness deliberately, so that the harness could go in green rather than
carrying a section that silently skips on every host and is therefore never
looked at again.

**Related and *not* the same gap.** The recursive root failsafe (`rm -rf /`) is
also absent from the harness, for a different and permanent reason: a test whose
failure mode is deleting the operator's filesystem is not worth running at any
level of confidence. It is covered instead by `rm.rs`'s unit test
`a_recursive_operand_that_is_the_root_is_refused`, which points `Rm::root` —
GNU's `x.root_dev_ino` — at a scratch directory, so the comparison under test is
the real one with no `/` anywhere near it. The harness does carry the
non-recursive `/` cases (`rm /`, `rm -d /`), which are safe whatever the code
does because `unlink("/")` and `rmdir("/")` cannot succeed. See
`scripts/rm-diff.sh`'s header, "What this harness will not do".

**Off unix, both options degrade to doing nothing**, which is the safe
direction: `device_of` answers `None`, nothing is ever "on a different device",
and `rm` removes what it was asked to remove instead of silently stopping short.
The only host that runs this today is the Windows development machine, where the
unit tests run; the shipped target is SlateOS, which is unix.
