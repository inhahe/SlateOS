## `A-DEVFS-NULL-AND-ZERO-STAT-AS-REGULAR-FILES` (lane A, 2026-08-26) — ✅ FIXED 2026-08-26

**Fixed.** The eleven root nodes are now `DevNode::chr_served(path, mode)` — a
new constructor, not a reuse of `chr`. That distinction is the substance of the
fix: `chr` documents itself as "served by the syscall layer, not by devfs" and
hardcodes `0o660`, whereas these eleven are served by devfs's own read/write
and carry conventional modes (`/dev/null` is `crw-rw-rw-`, `/dev/console` is
`crw-------`). Retyping them to `chr` would have made `cat /dev/zero` an error.
`uptime` stays `file`; it is the one node here that genuinely is one.

All four blast-radius sites listed below were walked. Three findings:

1. **The retype fixes a second bug that was never filed.** `container.rs`'s
   `CharDevice | BlockDevice` arm skips device nodes when archiving, arguing
   that writing one as an empty regular file "would be worse than skipping it:
   extracting the archive would replace a device with a plain file of the same
   name." That is exactly what a `tar` of `/dev` did until now, because `null`
   was typed `File`. The walk now skips it.
2. **`/dev/random` was escaping the page cache only by accident.**
   `read_file_routed` skips the cache when `entry_type != File` **or**
   `ino == 0`; devfs reports `ino: 0` from `FileMeta::minimal`, so it was the
   second clause doing all the work. Had devfs ever gained stable inodes,
   `/dev/random` would have silently begun serving the same bytes forever, and
   nothing would have caught it. The exclusion is now structural, by type.
3. **`read_file` and `read_at` serve different sets of nodes**, which the new
   rung had to be written around. `read_file` handles null/zero/full/random/
   urandom/console/tty; `stdin`, `stdout`, `stderr` and `kmsg` fall through to
   `unserved` → `NotSupported` ("served here, just not by the whole-file
   path"). Not a regression — `read_file` dispatches on path, never on
   `EntryType`, so the retype did not change it — but it means a whole-file
   read of `/dev/kmsg` through the VFS is refused while `read_at` works. Filed
   below as its own entry.

**Regression test.** A new rung asserts the pairing that nothing else did:
these eleven are `CharDevice` **and** readable via `read_at`. The two halves
catch opposite regressions — typed `file`, `[ -c ]` is false; typed `chr`, the
syscall layer intercepts and devfs refuses the read. The existing
`/input`,`/dri`,`/snd` loop pins the opposite pairing (CharDevice, read refused
with `NotSupported`). `/uptime` is asserted to remain `File`, so a careless
"make everything in here a device" fails.

**Original report follows.**

**In short:** `stat("/dev/null")` reports `S_IFREG` — a regular file — instead
of `S_IFCHR`, a character device. Eleven of the twelve nodes at the devfs root
are declared with `DevNode::file`, so `ls -l /dev/null` shows `-rw-rw-rw-`
rather than `crw-rw-rw-`, and the standard shell test `[ -c /dev/null ]` is
false. The devices *work* — reads and writes do the right thing — so nothing
fails loudly; only the type is wrong.

**The affected nodes**, all in `DEV_NODES` at `kernel/src/fs/devfs.rs:215`:

| node | declared | should be |
|---|---|---|
| `null`, `zero`, `full`, `random`, `urandom` | `file` | `chr` |
| `console`, `tty` | `file` | `chr` |
| `stdin`, `stdout`, `stderr` | `file` | `chr` (Linux makes these symlinks to `/proc/self/fd/N`; `chr` is the closer of the two available answers) |
| `kmsg` | `file` | `chr` |
| `uptime` | `file` | **stays `file`** — it is a text file, and the only one here that genuinely is one |

The nested nodes are already correct: `input/event0`, `input/event1`,
`dri/card0`, `dri/renderD128` and the three `snd/*` are `DevNode::chr`.

**The tree already knows why this matters** — it just never applied the
reasoning at the root. `syscall/linux.rs:19852`, on the `CharDevice` arm of
`meta_mode_bits`:

```
// The point of the variant: libinput refuses a node that is not
// S_ISCHR, and libdrm and ALSA make the same check.
```

That is exactly the argument for `null` and `tty` as well; the variant was
added for the device *directories* and the root nodes were left as they were.

**Why it is worth fixing rather than shrugging at.** `[ -c /dev/null ]` and
`[ -c /dev/tty ]` are ordinary idioms in configure scripts and shell libraries,
and a program that special-cases a character device to avoid seeking, buffering
or truncating will take the regular-file path on all eleven. The failure is
quiet in the same way the others in this file are: the node behaves correctly
when used, so only code that *asks what it is* gets a wrong answer, and that
code usually responds by silently choosing a different strategy rather than by
erroring.

**Care needed — this is not a one-word change.** `EntryType` is load-bearing in
routing, not only in `stat`:

1. `Vfs::read_at_routed` page-caches only `EntryType::File` with a stable
   inode. Retyping these to `CharDevice` removes them from the page cache —
   which is *correct* (a device must not be cached; that is the property that
   makes block nodes safe for verify-after-write) but it is a behaviour change
   on the read path and needs checking, not assuming.
2. `container.rs` merges `CharDevice | BlockDevice` into an arm that refuses
   `read_file`, so an archive walk does not descend into a device. `/dev/null`
   moving into that arm changes what a recursive walk does with it.
3. `kshell`'s `ls -F`, `file`, and long-listing arms all switch on the type, as
   does `d_type` in `getdents64`.
4. `devfs`'s own self-tests assert entry types in several places.

**The proper fix:** change the eleven declarations, leave `uptime` alone, then
walk each of the four sites above and confirm the new routing is the intended
one — in particular re-run the devfs and container self-tests, which is where a
wrong answer will show up first.

**How it was found.** A block-device self-test asserted that registering a disk
named `null` could not shadow `/dev/null`, and expected the survivor to stat as
`CharDevice`. It stats as `File`. The anti-shadowing property held; the
expectation was wrong, and it was wrong because the node is mistyped. That rung
now asserts "the disk did not win" instead of pinning the neighbouring value.

**Not a regression.** True since devfs was written.
