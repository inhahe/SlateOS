## 654. `SYS_FS_FCHMODAT_PINNED` (665): chmod takes WRITE not METADATA, masks the mode to twelve bits, rejects unknown flags, and checks policy against the resolved target

**Date:** 2026-08-31
**Lane:** A
**Decided by:** Claude (autonomous) — lane A, at lane B's request

**In short:** `chmod -R` on a directory tree you do not control is the classic
way a normal user tricks a privileged program into changing the permissions of
a file it never meant to touch: swap one of the directories for a symlink
partway through the walk, and the new mode lands on whatever the link points
at. Syscall 665 is the fix — it changes a file's permission bits *relative to
an already-open directory handle*, and refuses (`ESTALE`) if that handle no
longer refers to the directory it was opened on, so the swap is detected
instead of followed. Four smaller calls inside it each had two defensible
answers; this entry records which way each went and why.

It is the third member of the "pinned" family, after 662 `unlinkat` and 663
`fstatat`, and lane B named it their highest priority after `unlink`.

### The four decisions

| Question | Chosen | Rejected |
|---|---|---|
| Which capability right? | `Rights::WRITE` | `Rights::METADATA` (what 663 takes) |
| Mode bits above `0o777`? | mask to `0o7777` | reject as `InvalidArgument` |
| Unknown flag bits in `arg4`? | reject as `InvalidArgument` | ignore them |
| Sandbox/read-only checks run against… | the *resolved* object | the name as written |

### Why `WRITE` and not `METADATA`

663 `fstatat` takes `Rights::METADATA` because it only observes. Reading a
file's mode and being able to *set* it are not the same authority: a handle
granted so a program can look at a file must not also let it make that file
setuid. Sharing a right between the two would make "may I see this?" imply
"may I make this run as root?" — the two calls sit next to each other in the
same family precisely because the distinction is easy to lose.

### Why the mode is masked rather than rejected

§639 is the precedent, and it points the other way from the instinct. There, a
mask of `0o777` silently dropped the setuid bit — the request succeeded, the
bit did not arrive, and nothing said so. That is the worst available failure
mode for a permission change, so the resolved position was to *widen* the mask
to twelve bits (setuid, setgid, sticky, plus the nine rwx bits), not to start
refusing.

Above those twelve are the file-type bits of a POSIX `mode_t`. `chmod` has
ignored them since v7 Unix and Linux masks them off here too, so a caller
passing `S_IFREG | 0o755` — which is an ordinary thing for translated code to
do — gets `0o755` rather than an error. Rejecting would also buy nothing in
practice: lane B translates POSIX at the boundary and would have to mask before
calling anyway, so the strictness would be erased one layer up.

### Why unknown *flag* bits are rejected, which is the opposite rule

The asymmetry is deliberate, and it is the part most likely to look like an
inconsistency later. An unrecognised flag changes **what the operation does** —
a caller that mistranslated a Linux `AT_*` constant and had it silently ignored
would get a *follow* where it asked for a *no-follow*, which on this call is
exactly the escalation the syscall exists to prevent. The high bits of a mode,
by contrast, have a settled fifty-year meaning and no effect. Guessing is
dangerous in the first case and free in the second.

### Why the policy checks run against the resolved target

This is the one that was nearly got wrong. The first draft checked the sandbox
policy and the read-only flag against `dir.path + name` — the name as written.
But without `AT_SYMLINK_NOFOLLOW`, chmod *follows* a final symlink, so a link
sitting inside the pinned directory would have been checked where the link
lives and applied where the link points. A sandbox that permits
`/data/incoming` and forbids `/data/secret` would have been walked straight
through.

`Vfs::set_permissions`, the path-based route, resolves before checking for
exactly this reason. `set_permissions_at_pinned` now resolves the same way, so
the two routes cannot enforce different policies — a divergence between a
"safe" call and its unsafe sibling being a bug that hides for a long time.

Two consequences fall out of it:

- **When the name resolves to itself** (the ordinary case — not a symlink) the
  change is made under the pinned directory's own lock, verified a second time,
  and issued as `set_permissions_no_follow` *even when the caller asked to
  follow*. For a non-symlink the two are the same operation; if the name became
  a symlink in the window since the resolution, the no-follow form puts the
  mode on the link inode rather than on a target nothing checked.
- **When it does not resolve to itself**, the caller asked to follow a symlink
  out of the pinned directory, and pinning cannot cover that — following *is* a
  request to leave. The change goes through the target's own mount, which also
  avoids holding two filesystem locks at once. The residual race on the link's
  contents is the one plain `chmod` has.

### What was checked rather than assumed

The boot-time `/tmp/_pin` self-test now chmods `0o4751` through a live pin and
reads the mode back, because the twelve-bit mask in the syscall is worth
nothing if the filesystem underneath stores nine. And the stale-handle case
asserts the stronger form: refused **and** the impostor's mode provably
unchanged. A `chmod -R` that reported an error after already setting the setuid
bit would have failed in the only way that matters.

### How to reverse it

Unregister 665 in `dispatch.rs` and delete `Vfs::set_permissions_at_pinned`.
The mask width and the capability right are each a one-line change if the
operator wants them the other way; the resolved-target check is not — reverting
that reintroduces the symlink hole.
