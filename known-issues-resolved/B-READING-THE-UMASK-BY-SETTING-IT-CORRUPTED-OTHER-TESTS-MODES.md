## B-READING-THE-UMASK-BY-SETTING-IT-CORRUPTED-OTHER-TESTS-MODES (lane B, 2026-08-30) — **FIXED 2026-08-30**

**In short:** Two of `cp`'s tests failed at random, a few percent of runs each,
for as long as they have existed. The cause was in neither of them. POSIX offers
no way to *read* the file-creation mask without *setting* it, so the code that
needed the value set it, read the old one, and set it back — and in the moment
between those two calls, every file created anywhere else in the process came
out at the wrong mode. Under `cargo test`, "anywhere else in the process" is
every other test. Fixed by adding `coreutils::umask`, which reads the value from
`/proc/self/status` instead.

**The symptoms.** Over 25 repeat runs of `cargo test --bin cp`:

```text
thread 'tests::dereference_fails_on_a_dangling_link_in_the_tree' panicked at
  cp.rs:3557: called `Result::unwrap()` on an `Err` value:
  Os { code: 13, kind: PermissionDenied, message: "Permission denied" }

thread 'tests::copies_a_file_into_a_directory' panicked at cp.rs:2928:
  assert!(ok, "{err}")
```

Both point at the code under test. Neither has anything to do with it: a
`PermissionDenied` on a file the test itself had just written, and a copy that
failed for a reason the test then printed as an empty string.

**The mechanism.** `cp.rs` carried GNU's `cached_umask` (`copy.c`) verbatim:

```rust
let old = umask(0o777);   // deny everything
umask(old);               // put it back
```

That is correct in GNU, and this is worth being precise about — it is not a bug
upstream. `cp` there is one thread that creates nothing between the two calls,
so the window is provably empty. Here it is not: `cargo test` runs a binary's
tests on threads of one process, the mask is per-process, and a file another
thread creates inside the window arrives at mode `0000` — unreadable and
unwritable by its own owner. Worse, `cp.rs` deliberately *disabled* its
`OnceLock` cache under `#[cfg(test)]` (so the mode tests could vary the mask),
which turned one window per process into one window per copy.

**Why no lock would have fixed it.** The obvious repair is to serialise the
umask readers. It does not work, and the reason is worth stating: the thread
being harmed never touches the umask at all. It is calling `File::create`. A
mutex orders the participants that agree to take it, and the victim here is
every non-participant in the binary. `cp.rs`'s own `with_umask` helper had
already conceded this in a doc comment — *"It does not protect against an
unrelated test creating a file while a mask is installed — nothing can, short of
running single-threaded"* — which was true of the tool available at the time.

**The fix.** Linux publishes the mask in `/proc/self/status` as a `Umask:` line
(since 4.7), and SlateOS's own procfs writes the same field
(`kernel/src/fs/procfs.rs`, `Umask:\t{mask:04o}`). Reading it there is a read:
there is no window, and the answer is live rather than cached. The new
`userspace/coreutils/src/umask.rs` does that, with the POSIX probe kept as the
fallback for a Unix with no `/proc` — and the fallback probes with `umask(0)`
rather than `umask(0777)`, because between two bad windows the one that
*permits* too much loses a race quietly while the one that *denies* everything
loses it loudly.

**Who was migrated, and who was deliberately not.** Five binaries had each
reproduced the idiom privately:

| where | was | now |
|---|---|---|
| `cp.rs` | `umask(0777)` probe, uncached under `test` | `coreutils::umask::current`, cached outside `test` |
| `tar.rs` | `umask(0)` probe, `OnceLock` | `coreutils::umask::current`, `OnceLock` |
| `chmod.rs` | `umask(0)`, **never restored** | `coreutils::umask::current` |
| `mkdir.rs`, `mkfifo.rs` | `umask(0)`, kept | **unchanged** — see below |
| `userspace/chown` | `umask(0)` probe, `OnceLock` | **unchanged** — separate crate |

`mkdir` and `mkfifo` are not copies of this and must not be folded into it: they
*clear* the mask on purpose, because `-m` names an exact mode that the mask must
not narrow. `chown` is its own crate and cannot depend on `coreutils` (the same
constraint that moved `quote` out to `userspace/quoting`; see
`design-decisions.md` §370). Its probe is the permissive direction and is
cached, so it opens one short window per process; if a third crate ever needs
this, the module should move out beside `quoting` rather than be copied a
seventh time.

**The general lesson.** An intermittent failure whose stack points at the code
under test is evidence about the *instrument* at least as often as about the
code. Both symptoms here were mode-shaped — a denied read, a failed create — and
the only thing in the process that touches modes globally is the umask. That
is a cheap first question to ask, and it was not asked for weeks because each
failure was individually rare enough to re-run past.
**Update 2026-08-30 (lane A).** The kernel half now exists for *three* of the
calls — see `A-VFS-EIGHT-MORE-AT-CALLS-STILL-RESOLVE-BY-PATH` below and
`requests/a-b-the-at-family-now-has-three-primitives-that-resolve-the-handle.md`.
This entry stays OPEN because it is about `rm`, which lives in lane B's tree and
still walks by path until it is ported onto `SYS_FS_UNLINKAT_PINNED` (662).
