## TD-A-45-CALL-SITES-STILL-READ-A-FAILED-STAT-AS-ABSENCE (lane A, 2026-09-10) — FIXED 2026-09-11 at every site whose false answer licensed an action

**In short:** the kernel has a helper that answers "does this file exist?", and it
answers "no" both when the file really is missing and when the check itself failed —
for instance because permission was refused. Anything deciding what to do next gets
told "not there" when the truth is "could not tell". One such decision, which layer
of a stacked filesystem serves a file, has been fixed; 45 other places that use the
same helper have not been examined.

### What the helper does

`Vfs::exists` is `stat(path).is_ok()`, and `stat` passes through
`check_path_access` and `resolve_mount`, so `PermissionDenied`, `InvalidArgument`,
`InternalError` and symlink-loop errors all reach the caller as `false`.
`Vfs::is_directory` flattens the same way with `.unwrap_or(false)`.

`Vfs::exists_or_err` now exists beside it and returns `Ok(false)` only for
`NotFound`. The rule for choosing: if a `false` answer lets the caller *proceed*,
the call must be `exists_or_err`.

### What is fixed, and what is not

Fixed: `fs::overlay::which_layer`, where a non-NotFound stat failure on the upper
layer became `Layer::Lower` and served the base image's older content with no error.

Not examined: **37 `Vfs::exists` and 10 `Vfs::is_directory` call sites.** They are
recorded rather than converted blind, because most are the harmless use — *is there
a config file to read?* — and converting those would force callers to handle errors
that cannot arise there, which adds noise and hides the sites that matter.

The audit question per site is not "does it use `exists`" but **"if this returns
false because the check failed, does the caller go on to do something it would not
otherwise have done?"** Creating, overwriting, unmounting, freeing and choosing a
layer are yes. Reporting, listing and displaying are no.

### Severity: latent, and why that is still worth an entry

`check_path_access` returns `Ok` early when no file tags and no ACLs are configured,
and bypasses entirely for kernel tasks and pid 0. So today the reachable failures are
the resolve-level ones — a symlink loop, a path under no mount — which are possible
but exotic. It becomes live the moment ACLs or file tags are configured, a feature
that exists in this tree and is merely unused.

That is the argument for not waiting: a fail-open guard whose precondition is a
feature nobody has switched on is found by the person switching it on, and they will
be looking at the feature, not at this.

### Provenance

Lane B asked every lane to grep its own tree for the shape after `userspace/mkfs`
and `userspace/fsck` were found calling `is_mounted`, which returned `false` when
`/proc/mounts` could not be read — one non-UTF-8 mount name made every device look
unmounted, and `mkfs` would go on to format a live filesystem. Their general form:
*for any check guarding a destructive or privileged action, the error path must not
answer in the permissive direction.* Their remedy was `optionalfile::read_or_empty`;
`exists_or_err` is the same remedy one subsystem over.

Scoping note, because a naive grep here returns mostly noise: `kernel/src` holds 44
`.unwrap_or(false)` and 143 `.unwrap_or_default()`, almost all harmless. What decides
is the direction the name implies — `is_mounted() == false` means "go ahead", while
`is_allowed() == false` means "deny" and is safe. Filtering to bool functions whose
*false* answer is permissive (mounted, busy, locked, in_use, protected, exists, …)
returned exactly one, which is the one fixed above.

### Resolution 2026-09-11 — and a correction to what the count above was counting

Fixed: **16 sites** whose `false` answer licensed an action — one in `net/tftp.rs`
(`5eea3890b`), nine in `fs/overlay.rs` (`22f8c6575`), six rung verdicts in
`proc/spawn.rs` (`345652f83`). Deliberately left: the sites where a `false` answer makes the caller
*refuse*, which is the safe direction; `write_file`'s now carries a comment saying why.

**The count was roughly right and the composition was wrong, which mattered more.**
This entry said "most are the harmless use — *is there a config file to read?*", and
gave that as the reason not to convert them. Almost none are. The dominant category
is **assertions inside boot self-tests** — `fs/tmpwatch` ×6, `volume` ×3, `logpersist`,
`container`, `oci` ×2, and sixteen across `proc/spawn`'s ring-3 rungs — where
flattening does not "add noise that hides the sites that matter". It makes a rung
report a verdict it did not earn.

`fastpy-rm` is the example. Its check is introduced as *"the real verification: the
file must actually be gone. A no-op remove that returned 0 without deleting would
pass the exit check but fail here"*, and the rung's doc comment calls it *"the
false-pass-proof check"*. It was `let still_exists = Vfs::exists(RM_FILE);` — so a
stat that failed answered "gone" and the rung passed. The check designed to be
false-pass-proof had a false-pass mode of its own, and the same was true of
`fastpy-mv`, `fastpy-rmdir`, `fastpy-pkg gc` and the dash-relpath rung.

**The clearest single illustration is a pair two lines apart**, in the pkg-gc rung:

```rust
let keep_exists   = Vfs::exists(KEEP_BLOB);     // if !keep_exists  { FAIL }
let orphan_exists = Vfs::exists(ORPHAN_BLOB);   // if  orphan_exists { FAIL }
```

Identical call, opposite consequence. A failed stat makes the first **refuse** —
harmless — and the second **pass** — a false verdict. Nothing about either line tells
you which it is; only the direction of the assertion three lines later does. That is
why the audit question has to be *"does a false answer let the caller proceed?"* and
can never be *"does it call `exists`?"*.

### Why a grep could not have sorted these, and nearly misled me into thinking there was less work

`exists(` has **129 non-comment occurrences** in `kernel/src`; only ~36 are
`Vfs::exists`. The rest are unrelated `exists(handle)` lookups over in-memory tables
— `cgroup`, `container`, `netns`, `pidns`, `userns`, `tty`, `pty`, `epoll`,
`inotify`, `signalfd`, `timerfd`, `alsa_pcm`, `drm::card_fd`, `evdev_fd`, `volume`,
`cnetwork` — which are map lookups and cannot fail, so they have no error to flatten.
`is_directory(` returns 44, of which 34 are on parsed on-disk entries in `fat`,
`ntfs` and `iso9660` and have nothing to do with the VFS.

And a bare `grep exists` over the same tree returns mostly **prose**: the first forty
hits are all doc comments using the English word. That is the fifth mechanism in this
tree that reads prose as code, after a checker counting a comment, a fixture matching
its own scan, `argv[optind - 1]` inside module docs, and a comment opening
`# shellcheck`. The count in this entry's own title survived only because it was
spelled `Vfs::exists` — which also appears in prose, including in this file.

### Left open deliberately, with the reason

Six sites answer in the *refusing* direction and are safe, but their **messages** are
wrong when the stat failed rather than returned `NotFound`:

| site | says | truth when the stat failed |
|---|---|---|
| `spawn.rs` ×4 staging preconditions | "staged *X* but VFS reports it absent" | the VFS reported nothing |
| `pathz_missing` / `pathz_fixtures_missing` | `SKIP: <rung> — prerequisite missing: X` | the prerequisite may be present |

Both `pathz` gates now name the error on a separate line instead, because a SKIP line
claiming an artifact is absent sends the next reader hunting a staging bug that is not
there — and `check-boot-skips.py` ratchets these, so a wrong reason is a wrong reason
that persists. The four staging preconditions are cosmetic and stay as they are.

One site is flattened **on purpose** and now says so: `on_ext4 = Vfs::exists("/mnt")`
chooses between ext4 and tmpfs fixture paths, and its comment already states the
intent — *"a test whose only path to running is a fixture CI might stop providing is a
test with an expiry date on it"*. Falling back when the probe fails is the designed
behaviour, not a fail-open.
