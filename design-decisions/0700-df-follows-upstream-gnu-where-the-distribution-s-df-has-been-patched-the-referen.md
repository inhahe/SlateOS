## 700. `df` follows upstream GNU where the distribution's `df` has been patched — the reference binary is evidence, not authority

**Date:** 2026-08-29
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** `df` is the command that reports how full each disk is. Ours is
now a line-by-line transcription of GNU coreutils 9.4, checked case by case
against the `df` installed on the dev machine. On 37 of those cases the two
disagreed in exactly one way: ours printed a row for `/dev` and the reference
did not. The reference turned out to be right about *its own* machine and wrong
about GNU — Ubuntu patches `df` to hide two filesystem types that upstream does
not hide. The decision is to **match upstream and not the binary we compare
against**, and to teach the comparison harness to drop those rows from both
sides rather than to introduce the patch into our source.

### Context

`df` decides which mount points to print by classifying some of them as
"dummy" — pseudo-filesystems that exist for the kernel's convenience and would
be noise in a disk-usage table. gnulib's `mountlist.c` spells the list out:

```c
#define ME_DUMMY_0(Fs_name, Fs_type)            \
  (strcmp (Fs_type, "autofs") == 0              \
   || strcmp (Fs_type, "proc") == 0             \
   || strcmp (Fs_type, "subfs") == 0            \
   ...
   || strcmp (Fs_type, "kernfs") == 0           \
   || strcmp (Fs_type, "ignore") == 0)
```

`devtmpfs` — the type `/dev` is mounted with — is **not** in it, in 9.4 or in
gnulib master. Our transcription therefore prints `/dev`, and the reference
`df` does not.

Establishing which side was wrong took the whole chain of elimination, because
every cheap explanation was false: the mount was in `/proc/self/mountinfo`; its
device number collided with nothing (checked against all five other mounts on
the same `st_dev`); a Python reimplementation of `filter_mount_list` kept it;
`statvfs("/dev")` reported non-zero blocks, so it was not dropped by the
`fsu_blocks == 0` rule. What settled it was interposing `stat` with
`LD_PRELOAD` and watching the reference `df` run: it never `stat`s `/dev` at
all, which means the row is gone *before* the stat — i.e. classified dummy.
And then:

```text
$ strings -a /usr/bin/df | grep -n devtmpfs
devtmpfs
$ # sitting inside the run of dummy-type literals, next to squashfs
```

Debian and Ubuntu carry a patch adding `devtmpfs` and `squashfs` to
`ME_DUMMY_0`. `squashfs` is why a machine with snaps installed does not get
twenty `/snap/...` rows from `df`.

### Decision

1. **`df.rs` implements upstream's list, unpatched.** `/dev` prints, and a
   `squashfs` mount would print.
2. **`scripts/df-diff.sh` removes the divergence from the comparison rather
   than from the subject.** It reads `/proc/self/mountinfo` for every mount
   whose type is `devtmpfs` or `squashfs`, and deletes rows naming those mount
   points from *both* programs' output before diffing (`drop_hidden`).
3. **A case that is *about* one of those file systems opts out** with the
   harness's `=` marker — `=~df /dev` compares the `/dev` row in full, because
   deleting it would leave nothing to compare.
4. **A case with no `target` column and no operand stays an xfail**
   (`df --output=source`), because the mount point is the only thing
   `drop_hidden` can identify a row by. It is kept deliberately, as the
   harness's own standing record that the divergence is still there.

### Rationale

The reference binary is a *measurement instrument*, and the whole value of the
differential harness is that it settles arguments about what GNU does. That
only holds while the instrument is understood. Copying the patch would make 37
cases go green while making the source wrong about the thing it claims to be a
transcription of — and wrong invisibly, since the comment above the list would
still cite gnulib. The next person to compare our list against `mountlist.c`
would find a discrepancy with no explanation attached.

There is also a substantive argument for upstream's behaviour on *this* OS.
The Ubuntu patch is a workaround for two conditions we do not share: `/dev` on
`devtmpfs` is a Linux-ism, and the `squashfs` noise is created by snap, which
this OS does not have. Adopting a distribution's workaround for a problem the
system does not have is how a transcription accumulates cruft that nobody
later dares remove.

### Alternatives considered

- **Add `devtmpfs` and `squashfs` to our dummy list.** Rejected above: it
  makes the harness green by making the source wrong, and it imports a
  distribution policy under a comment citing gnulib. If we ever *want* to hide
  these — a reasonable thing for a desktop OS to want — it should be a
  documented decision of ours with its own section, not a silent copy.
- **Compare against a locally-built pristine coreutils 9.4 instead.**
  Genuinely correct, and rejected only on cost/benefit: it means building
  coreutils inside the WSL environment as a harness prerequisite, and every
  other `*-diff.sh` in `scripts/` compares against the installed binary. One
  harness with a different reference is a trap for whoever reads two of them.
  If a *second* distribution patch is ever found, this becomes the right
  answer and should be revisited for all the harnesses at once.
- **Mask the `/dev` row with the existing `~` digit mask.** Does not work:
  the difference is a whole extra line, not a digit, so the outputs differ in
  line count however the digits are rewritten.
- **Drop the affected cases from the harness.** Rejected — 37 cases is most of
  the whole-table coverage, and they test far more than the one row.

### Where it lives

- `userspace/coreutils/src/bin/df.rs` — `me_dummy_0`, with the upstream list
  and a comment recording that the distribution's is longer.
- `scripts/df-diff.sh` — `hidden_mounts`, `drop_hidden`, and the `=` marker
  documented in the marker table at the head of the case list.
- The xfail line `!Ubuntu hides devtmpfs; with no target column the extra row
  cannot be dropped|~df --output=source`.

### How to reverse

If the operator wants a desktop-quiet `df`, add the two types to `me_dummy_0`
behind a comment citing *this* section rather than gnulib, delete
`drop_hidden` and its call sites from the harness, and turn the `=` markers
and the `--output=source` xfail back into ordinary cases — they should then
pass unmodified, which is the check that the change was complete.
