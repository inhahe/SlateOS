# B → A — `/bin/make` is now on the image, with fixtures; it needs a self-test rung

**Status:** ✅ **DONE** — the rung exists as
`spawn.rs::self_test_linux_real_glibc_make` and runs on every boot.

**It is currently RED, and not because of this request.** The rung does its job:
it found two real bugs in succession. The first was a `stat` EACCES, fixed in
`0153b147c` (see
`requests/a-b-make-eacces-was-the-abi-switch-your-rootfs-change-made.md` — in
short, `make` had stopped being a Linux-ABI binary and nobody noticed). With
that cleared, `make` now dies in ring 3 on a near-null read, because
`posix_spawn_file_actions_t` is 4,624 bytes against musl's 80 and
`posix_spawn_file_actions_init` smashes its caller's stack. That is **lane B's**
and is filed as
`requests/a-b-posix-spawn-file-actions-init-smashes-the-callers-stack.md`,
escalated to a merge blocker in `c96d6040e`. Do not read the red as this rung
being unfinished.

**Filed:** 2026-08-20 by Lane B.
**Action needed from A:** one new self-test function in
`kernel/src/proc/spawn.rs`, beside `self_test_bash_on_slateos_libc` and
`self_test_pkgconf_on_slateos_libc`. Everything on Lane B's side — the binary,
the staging, the staleness gate and the makefile fixtures — is done and merged
to `main`.

**Not urgent, nothing is blocked on it.** The image is green without it;
`/bin/make` simply sits there unverified. Pick it up whenever a `spawn.rs` task
is already open.

## What landed

Upstream **GNU make 4.4.1**, unmodified, cross-compiled and statically linked
against `toolchain/sysroot/lib/libc.a` — our own POSIX layer, not glibc and not
zig's bundled musl. Zero undefined symbols, zero duplicates.

- Built by `scripts/make-spike/run.sh` → `build/spike/make-slateos.elf`
- Staged by `scripts/create-ext4-rootfs.sh` as **`/bin/make`**
- Static `ET_EXEC`, x86-64, 3,114,912 bytes, entry `0x10488c0`
- Same staleness gate as bash and pkgconf: older than `libc.a` ⇒ fatal,
  `ALLOW_STALE_FIXTURES=1` downgrades

Getting there required raising `libc.a` to `codegen-units=4096`
(design-decisions.md §339) — worth a glance because it changed every binary on
the image, not just make's, and shrank pkgconf by 12.8% and bash by 6.0%
without a source change.

## Why `--version` is not enough here, even more than it wasn't for pkgconf

`/bin/make --version` proves the binary loads, relocates, runs `main` and exits
0. It proves nothing about make, because what make *does* is parse a makefile,
build a dependency graph, compare mtimes, and run recipes through a shell.

make is also a far heavier OS client than bash or pkgconf. The spike recorded
what it will actually reach for, from its own `src/config.h`:

| configure decided | consequence for us |
|---|---|
| `HAVE_POSIX_SPAWN 1`, `HAVE_POSIX_SPAWNATTR_SETSIGMASK 1` | recipes launch via **`posix_spawn`, not `fork`+`exec`** |
| `MAKE_JOBSERVER 1`, `HAVE_MKFIFO 1`, no `HAVE_NAMED_SEMAPHORES` | a `-j` build coordinates over a **FIFO** |
| `HAVE_WAITPID 1`, `HAVE_WAIT3 1` | make's own exit code depends on decoding a raw wait status |

The `posix_spawn` line is the one to internalise: whatever is true of our
`fork` is not the question. All twelve symbols are present in `libc.a` (checked
with `nm`, not by reading the source), but presence is not behaviour — that is
exactly what the rung is for.

**Suggestion: keep `-j` out of the first rung.** The FIFO jobserver is a second,
harder question, and a serial rung that passes is a much more useful result than
a parallel one that fails ambiguously. Once serial is green, `-j2` on
`02-order.mk` is the natural follow-up — it should still produce the same
sequence, since the prerequisites are strictly chained.

## The fixtures

`scripts/create-ext4-rootfs.sh` writes five makefiles to
**`/usr/share/make-selftest`**. Drive them with `make -f`, from a **writable**
cwd — every recipe writes into the *current* directory, and `/usr/share` may be
read-only. `/tmp` is the intended home. Use a **fresh empty directory per
fixture**; `03-mtime.mk` in particular is stateful by design.

Every behaviour asserted below was **run against real GNU make 4.3 before this
request was filed**, not inferred. That caught two errors in my own first draft,
both noted inline.

| fixture | proves | assert |
|---|---|---|
| `01-recipe.mk` | `posix_spawn` → `/bin/sh` → recipe → reap status 0 | exit 0; `recipe-ran.txt` contains exactly `slateos-make-ok\n` |
| `02-order.mk` | the graph is traversed depth-first, bottom-up | exit 0; `order.txt` is exactly `first\nmiddle\nfinal\n` |
| `03-mtime.mk` | mtime comparison — the heart of make, and the only VFS assertion here | see below |
| `04-vars.mk` | variable expansion + `$(shell …)`, a *second* spawn path (pipe capture) | exit 0; `vars.txt` is exactly `slateos-make captured-ok\n` |
| `05-failure.mk` | failure propagation + error cleanup | see below |

Each fixture carries its own PASS/FAIL note in a comment at the top, saying what
a failure means, so the log can be read without coming back to this file.

### `03-mtime.mk` — three runs, and the second and third fail differently

It is self-contained (`stale-in.txt` has its own rule), so an empty directory is
all it needs. The recipe **appends**, so a spurious rebuild is visible as an
extra line rather than being idempotent and invisible.

| step | expect |
|---|---|
| run 1 | exit 0, `stale-out.txt` = **1** line (`rebuilt`) |
| run 2, nothing changed | exit 0, `stale-out.txt` = **still 1** line |
| rewrite `stale-in.txt`, run 3 | exit 0, `stale-out.txt` = **2** lines |

**Assert on the line count, not on make's message.** Run 2 prints `Nothing to be
done for 'all'.` — *not* "up to date", because `all` itself has no recipe. My
first draft asserted the wrong string; running it is how I found out. Message
wording is not something make promises. The file is.

The two directions are separate facts:

- **Run 2 rebuilding anyway** ⇒ mtimes are non-monotonic, or the output landed
  older than the input.
- **Run 3 *not* rebuilding** ⇒ timestamps do not advance. A VFS that returns a
  constant `st_mtime` for every file passes run 2 and fails only here, which is
  why run 3 is worth the extra step.

Run 3 is the one plausible flake: if our `st_mtime` granularity is coarse (say
1 s) and the rewrite lands in the same tick as run 1's write, make will
correctly conclude nothing changed. **If that happens, please report it rather
than inserting a delay to paper over it** — coarse mtime is a real property of
our VFS with real consequences for every incremental build we will ever run, and
this is the first thing on the image that would notice. If it needs a delay to
be deterministic, that finding belongs in `known-issues.md` first.

### `05-failure.mk` — two assertions, do not collapse them

The recipe creates the target and *then* exits 1, which is how a real
interrupted build leaves a half-written object behind. The fixture sets
`.DELETE_ON_ERROR:` so make unlinks it.

| assert | fails when |
|---|---|
| make exits **non-zero** (GNU make uses 2) | wait status is being decoded as success |
| `should-not-exist.txt` **does not exist** | error-cleanup path never reached `unlink()`, or `unlink()` failed |

Check exit non-zero rather than `== 2`; the value is make's convention, not a
guarantee.

The first assertion is the `slateos-badver` row of the pkgconf request wearing a
different hat, and it is the one I would most encourage keeping. Without a case
that must *fail*, all four other fixtures still pass against a make that treats
every child as successful — and "every broken build silently succeeds" is worse
than make not running at all.

(Second correction from testing: without `.DELETE_ON_ERROR` upstream make
**keeps** the partial file. My first draft asserted the target would be absent
by default. It would not have been.)

## Two things from the pkgconf rung that will save you a boot cycle

1. **Capabilities.** `A-PKGCONF-A-MISSING-CAPABILITY-PRESENTED-AS-A-MISSING-FILE`
   — pkgconf `lstat`ed its search dirs and silently dropped the ones it could
   not stat, so a missing `(File, METADATA)` right produced a message
   word-for-word identical to a genuinely missing `.pc` file. make does far more
   `stat`ing than pkgconf does (that is its whole job), and it also needs to
   **create, write, unlink and re-stat** files in its cwd, plus spawn `/bin/sh`.
   A narrow capability list will present as "nothing to be done" or a spurious
   rebuild, not as a permission error. Grant generously first, narrow after it
   passes.

2. **`pathz_missing(...)` self-skip applies unchanged.** A checkout that has
   never run `scripts/make-spike/run.sh` has no `/mnt/bin/make`, and should
   still boot green with the harness printing `PATH-Z COVERAGE INCOMPLETE`.

`MAX_YIELDS`: make's startup is heavier than pkgconf's (it reads and parses a
makefile and spawns a shell) but far lighter than bash's. bash's budget
(`1_048_576`) is a safe starting point; pkgconf's `262_144` may be tight for the
three-run `03-mtime.mk` sequence.

## What a green rung would and would not establish

It would make make the **third** real program on the image exercised through our
libc, and the first that exercises `posix_spawn`, wait-status decoding and
mtime-based decisions end to end. That is a genuinely new slice — bash uses
`fork`, and pkgconf spawns nothing at all.

It would **not** establish that make can build anything real. No `-j`, no
recursive make, no `include`, no pattern rules, no archive members. Those come
later, and each is a separate fixture I am happy to stage — say the word.
