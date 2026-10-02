### TD-OPENAT2-BENEATH-INROOT. `openat2` `RESOLVE_IN_ROOT` is safely refused, not implemented (`RESOLVE_BENEATH` now enforced) — ACCEPTED LIMITATION 2026-07-22, narrowed 2026-08-29

**Where:** `kernel/src/syscall/linux.rs::sys_openat2` (the resolve-flag gates)
and, since 2026-08-29, `posix/src/file.rs::openat2` step 7.

**What it is:** `openat2`'s `RESOLVE_BENEATH` (forbid the walk from escaping
the `dirfd` directory via `..`/absolute paths/escaping symlinks) returns
`EXDEV`, and `RESOLVE_IN_ROOT` (chroot-like rooted resolution) returns
`EOPNOTSUPP`. Both are *refused* rather than enforced because we have no
per-fd root/base tracking threaded through the VFS resolver, and a
half-correct containment check would be a sandbox-escape hazard (worse than
refusing). `RESOLVE_NO_SYMLINKS`, `RESOLVE_NO_XDEV`, `RESOLVE_NO_MAGICLINKS`,
and `RESOLVE_CACHED` are all handled correctly (see the resolved
BUG-OPENAT2-RESOLVE-NO-SYMLINKS-IGNORED entry).

**Proper fix (when needed):** thread a containment base (the `dirfd`
directory, or the resolution root) through `Vfs::resolve_inner` and, at
every component step *including symlink-target expansion*, verify the
running resolved path stays at/below the base — rejecting with the Linux
error (`EXDEV` for BENEATH escape, `-EXDEV`/`ELOOP` per Linux semantics).
This must be containment-checked per hop (not just on the input path) to be
safe against symlinks that point outside the base. Deferred until a real
consumer (container runtime / sandbox) needs beneath/in-root resolution;
the current refusal is safe in the meantime.

**Amended 2026-08-29 — "the current refusal is safe" was only ever true of
half of it.** This entry scoped itself to `kernel/src/syscall/linux.rs`, and
the ABI has *two* implementations in this tree. libc's `posix/src/file.rs::
openat2` validated the `resolve` word, discarded it, and delegated to plain
`openat` — so a native caller asking for `RESOLVE_BENEATH` got a working
descriptor and no confinement whatsoever, while a Linux-ABI caller making the
identical request got `EXDEV`. Fail-open on one side, fail-closed on the other,
for as long as both have existed. Fixed the same day: libc now refuses every
restriction it cannot enforce, with the kernel's exact errnos (`EAGAIN` /
`EOPNOTSUPP` / `EXDEV`), so the two agree. Seven tests pin it, each asserting
the refusal errno *differs from the unrestricted call's* so they cannot pass
vacuously against a deleted gate.

One deliberate residual divergence: `RESOLVE_NO_SYMLINKS` is **enforced** by
the kernel (threaded to the VFS resolver as `OpenFlags::NO_SYMLINKS`) but
**refused** by libc, because libc's `openat` flattens `dirfd` + `path` into one
absolute path and calls `open`, whose flag word has no per-component no-follow
bit — `O_NOFOLLOW` covers the final component only. Refusing is safe; it is
still a capability a native binary cannot reach. The structural fix is a native
syscall number for `openat2` so libc forwards instead of re-implementing, which
is what let the two drift apart in the first place. Both that and the VFS work
were asked for in `requests/b-a-openat2-resolve-beneath-is-fail-open-in-libc-
and-unenforceable-in-the-vfs.md`, which carries a `Status:` stamp recording
which of its two asks landed — the reply, and the current state of both, is
`requests/a-b-openat2-resolve-beneath-is-enforced.md` (VFS work done; the
native syscall number is waiting on lane B to say it will forward, because an
unused number is an ABI commitment). **This divergence therefore still stands**
and is the live remainder of the `NO_SYMLINKS` row.

**The generalisable lesson, which is worth more than the bug:** when a syscall
has two implementations in this tree, a known-limitation note about one of them
is not a note about the syscall. Nobody re-read the libc side because this
entry read as though it covered `openat2` rather than `sys_openat2`. The ABI
surface is exactly where we keep having two implementations.

**Consumer note:** `TD` above says this is deferred "until a real consumer
needs beneath/in-root resolution". One now does — `userspace/coreutils/src/bin/
tar.rs` emulates `RESOLVE_BENEATH` in userspace (`Dir::locate`) because neither
target could supply it. See `design-decisions.md` §702 for why emulating beat
waiting. The emulation is correct and race-free, so this is still not urgent.

**Resolved for `RESOLVE_BENEATH` 2026-08-29 (lane A) — `RESOLVE_IN_ROOT`
remains an accepted limitation.** The kernel now enforces `RESOLVE_BENEATH`
rather than refusing it, so this entry's title is half-wrong and the half that
stays is `IN_ROOT`. What landed:

- `Vfs::beneath_step` — the rule itself, per hop and **syntactic**. An absolute
  symlink target is refused without ever being compared to the base, and a `..`
  is refused at the moment the walk would step above the base, not judged by
  where it eventually lands. The "proper fix" paragraph above said
  "verify the running resolved path stays at/below the base", and that phrasing
  is **wrong in the permissive direction** on three of lane B's ten measured
  rows (`$PWD/sub`, `$PWD`, `../d/sub` — all allowed by a prefix check, all
  refused by Linux). A resolved path has forgotten how it got there; the
  implementation tracks a depth counter instead, which is what makes those
  questions answerable at all.
- The base is threaded through `Vfs::resolve_inner`'s symlink expansion and
  checked *before* `normalize_path`, which is the call that destroys the `..`
  the decision depends on.
- `Vfs::resolve_beneath`, `fs::handle::open_beneath`, and `sys_openat_beneath`,
  which shares the whole post-resolution tail (`open_resolved`) with the
  ordinary open path — a second fd-install path would be this entry's own
  lesson repeated one layer down.
- `Vfs::beneath_fragment_ok` — the part of the rule decidable without a base,
  so the syscall can refuse an escaping fragment *before* it looks up `dirfd`
  and its answer cannot be used to probe the caller's fd table.

Covered by three suites at different depths: `fs::vfs` (the rule, plus real
symlinks), `fs::handle` (that a contained open actually succeeds and reads the
right bytes — the case a refuse-everything implementation would pass), and
`syscall::linux` (the ABI's refusals). `RESOLVE_IN_ROOT` is deliberately still
`EOPNOTSUPP`: it is the same machinery with `..` clamping at the base and
absolute targets re-rooted, but lane B has no consumer for it and an unused
ABI is a commitment we would have to keep.

**Closed for `NO_SYMLINKS` too, 2026-08-30 (lane A) — `SYS_FS_OPENAT2` = 661
exists, so libc forwards instead of re-implementing.** The "deliberate residual
divergence" three paragraphs up is gone, and so is the structural cause this
entry named for it. Lane B said yes in
`requests/b-a-yes-forward-openat2-and-here-is-the-shape-we-want.md`; the number
is `SYS_FS_OPENAT2(path_ptr, path_len, flags, mode, resolve, dirfd)`, six flat
arguments rather than Linux's `open_how` struct, per `design-decisions.md`
§639. libc no longer has to reach `NO_SYMLINKS` through an `openat` that
flattens `dirfd` into an absolute path, because it no longer goes through
`openat` at all.

Two things about it are worth reading before touching it:

- **Our `resolve` bits are deliberately nowhere near Linux's.**
  `RESOLVE_NO_SYMLINKS` is `1 << 16` and `RESOLVE_BENEATH` is `1 << 17`, not
  `0x04`/`0x08`. Every Linux resolve value lies in `0x00..=0x3f`, so an
  untranslated one has no known bit here and at least one unknown bit, and is
  refused on the first call. Reusing Linux's numbers would have meant a dropped
  translation line silently turning one restriction into another — a caller
  told its confinement was applied when it was not, which is precisely this
  entry's original failure mode reintroduced by the fix for it.
  `test_dispatch_openat2_native` case (a) passes `0x08` and requires
  `InvalidArgument`, so a later "harmonisation" of the constants fails a test
  rather than shipping.
- **The handle lookup happens after the containment check, on purpose.** An
  absolute fragment under `RESOLVE_BENEATH` is refused with `CrossDevice`
  before `dirfd` is resolved at all, so the reply cannot be used to tell a
  valid handle from an invalid one. Case (b) of the same test pins that by
  asking with a deliberately bogus handle and requiring `CrossDevice` rather
  than `InvalidHandle`.

`RESOLVE_IN_ROOT` remains the sole open item on this entry, unchanged and still
without a consumer.

**The libc half landed 2026-08-30 (lane B) — `openat2` forwards, and both
refusals are gone.** `posix/src/file.rs::openat2` step 7 is now a three-way
decision rather than a list of refusals: `plan_resolve` returns `Delegate`
(nothing to carry — plain `openat`), `Forward(k_resolve)` (`BENEATH` and/or
`NO_SYMLINKS`, translated to the kernel's bit values and sent to
`SYS_FS_OPENAT2`), or `Refuse(errno)` (`IN_ROOT` → `EOPNOTSUPP`, `CACHED` →
`EAGAIN`). A native binary can now reach containment; before today it could
only be told no.

Three things on the libc side that are not obvious from the kernel side:

- **`dirfd == 0` is not usable for a relative path, and the reason is a
  divergence nobody had written down.** The native ABI reads handle 0 as "the
  process working directory", meaning the one `pcb::get_cwd` returns — but this
  libc's `chdir` (`posix/src/unistd.rs`) keeps its answer in a libc-side buffer
  and *never tells the kernel*. Passing 0 would confine the walk beneath
  whichever directory the kernel last recorded, which is a containment check
  against the wrong base, and it fails **open**: a wrong base still returns a
  valid descriptor, which is this entry's original failure mode again. So
  `openat2_forward` opens libc's own cwd as a scratch directory handle for
  `AT_FDCWD` and closes it on every exit path. 0 is passed only for an absolute
  path, where the base is provably never read — without `BENEATH` the handler
  takes the fragment as the whole answer, and with `BENEATH` it refuses an
  absolute fragment before the handle lookup (the ordering pinned by case (b)
  above).
- **`/dev/ptmx` and `/dev/pts/<n>` are refused with `EOPNOTSUPP` when a
  restriction is in play**, because they are answered inside libc and do not
  exist in the kernel's namespace. Forwarding one would come back `ENOENT` — a
  lie, since the file exists to that caller. The predicate is factored out as
  `file::is_pty_device_path` and `open_pty_device` gates on it too, so the
  refusal cannot name a different set than the claim.
- **The bit translation is tested directly, not end to end.** A host test cannot
  open anything (every native syscall is stubbed off-target), so an end-to-end
  assertion about `RESOLVE_BENEATH` degenerates into "the stub said `ENOSYS`"
  and would stay green with the translation deleted — exactly the untested
  marshalling lane A warned about. `plan_resolve` is therefore a pure function
  with its own tests, including one that asserts every Linux resolve value is
  `< 0x40` and both kernel values are `>= 0x40`, so a "harmonisation" of the
  constants fails on this side too and not only in `test_dispatch_openat2_native`.

The five tests that pinned the old refusals for `BENEATH`/`NO_SYMLINKS` are
gone, replaced rather than inverted: they cannot become "must succeed" on a
host that cannot open. `IN_ROOT` and `CACHED` keep their end-to-end differential
tests unchanged.

**The `dirfd == 0` route into this entry's failure mode is closed, 2026-08-31
(lane A) — `0` no longer means the cwd.** The bullet three paragraphs up
("`dirfd == 0` is not usable for a relative path") described the native ABI as
reading handle `0` as "the process working directory". It no longer does: `0`
now means **no base supplied**, and is legal only in the one shape where the
base is provably never read. Rationale in `design-decisions.md` §648; lane B's
report is `requests/b-a-the-kernels-cwd-and-libcs-cwd-are-two-different-
directories.md`, answered in place.

Why this belongs on *this* entry rather than only on §648: the fail-open mode
this entry exists to document — *a containment check against the wrong base
does not fail, it succeeds, on the wrong directory, and returns a valid
descriptor that looks exactly like working confinement* — had `dirfd == 0` as
a live route into it, on a base the caller never named. That route is gone
structurally, not by discipline. For `SYS_FS_OPENAT2` (661):

| `resolve` | fragment | `dirfd == 0` |
|---|---|---|
| none | absolute | **allowed**, unchanged — the base is never read |
| none | relative | `InvalidArgument` (was: the kernel's cwd) |
| `RESOLVE_BENEATH` | absolute | `CrossDevice`, already, before the base is looked at |
| `RESOLVE_BENEATH` | relative | `InvalidArgument` |

So under containment `0` is now *always* an error, which is what containment
wants. `SYS_FS_UNLINKAT_PINNED`/`FSTATAT_PINNED`/`GETDENTS_PINNED` (662/663/664)
lose the branch outright rather than narrowing it — they take a single-component
name, never an absolute path, so no shape lets them proceed with no base; `0`
falls through to the ordinary handle lookup and returns `InvalidHandle`, the
honest answer since handle `0` does not exist.

Two things worth keeping:

- **The decisive argument was not cost.** Lane B recommended this outcome from
  the cost of keeping two cwds in step across `fork`/`exec`/`spawn`, which is a
  real cost and therefore re-litigable by someone who finds it affordable. The
  non-negotiable argument is `CLAUDE.md`'s *"Every kernel object accessed via
  unforgeable handles. **No ambient authority.**"* — `dirfd == 0` was a base the
  caller did not name, could not have been denied, and could not delegate. The
  alternative (a native `SYS_FS_SET_CWD` so the two cwds agree) would have added
  a syscall whose purpose is to move the ambient base around.
- **Lane B's stated trigger fired twice, not once.** They wrote that the trigger
  to revisit was "the first native syscall that resolves a *relative* path
  against `pcb::get_cwd`. Today `SYS_FS_OPENAT2` is the only one." It was two:
  `pinned_dir_arg` (the 662/663/664 helper, landed the same day, §647) carried
  the identical branch and was not visible from their side. Both are gone, so the
  count is zero and the trigger is retired.

Covered by `test_dispatch_openat2_native` cases (f)/(g)/(h) — absolute-with-`0`
(reading the byte back, so a refuse-everything implementation cannot pass),
relative-with-`0`, and relative-with-`0`-under-`BENEATH`. That test's own doc
comment previously claimed `dirfd == 0` was "covered from ring 3 rather than
here"; the ring-3 coverage it pointed at goes through the **Linux** ABI's
`AT_FDCWD`, a different mechanism, so native `0` was covered nowhere. It is
testable from kernel context precisely because it no longer depends on a cwd.
