### TD-OILS-TESTS-RUN-IN-THE-CRATE-DIRECTORY. The oils test harness starts external commands in the crate's own directory, so a misbehaving spawn litters the source tree and silently breaks glob-sensitive tests — 2026-08-02 — ✅ **RESOLVED 2026-08-02**

**Resolution.** Every shell the tests build now stands in a scratch directory of
its own. `tests::new_shell()` replaces the bare `Shell::new()` at all 130 sites,
binding `cwd` — and `$PWD` alongside it, so the two agree — to `tests::test_cwd()`.

That directory is one per *thread*, which is what makes it per test without a
lifetime to thread through the helpers: the harness runs each test on a thread of
its own, so a `thread_local!` `ScratchDir` is created on the test's first shell
and removed when the test's thread ends — including the unwinding a failed
assertion does. Per-thread rather than per-shell is also the behaviour the tests
want, since the several shells one test builds have to see each other's files.

Two follow-on cleanups fell out of it:

* the five glob tests that needed a *relative* pattern (a different resolution
  path from a rooted one) were the last things writing into the source tree.
  `ScratchDir::relative` now creates its directory inside `test_cwd()` and the
  tests name it by its new `base()`, passing `test_cwd()` as the glob's cwd — so
  the pattern is still relative and resolves nowhere near the working copy.
* `cwd_guard` and the `let orig = current_dir()` / `set_current_dir(&orig)` pairs
  in eleven tests were **vestigial** and are gone. A working directory belongs to
  a `Shell` (`change_dir` moves `self.cwd` and nothing else); nothing in the crate
  calls `set_current_dir`, so the process's directory never moved, the restores
  were no-ops and the lock guarded nothing. The comments claiming otherwise —
  "mutates the process-global cwd, so serialize …" — were the misleading kind, and
  are replaced by one note saying why no lock is needed.

Covered by `a_test_shell_stands_in_a_scratch_directory_of_its_own`, which asserts
the contract directly: `echo Cargo.tom*` finds nothing to match (the crate
directory has one), `$PWD` and `pwd` agree and are not `CARGO_MANIFEST_DIR`, and a
file written by one `run()` is visible to the next. Full suite green
(1156 + 4 + 39 + 7), clippy clean, and `userspace/oils/` holds nothing but its
tracked files after a run.

**What it was, below.**

**Where:** `userspace/oils/src/interp.rs` — the `run()` test helper builds a
`Shell` whose `cwd` is whatever `cargo test` inherited, which is the crate root.
Every test that runs an external command therefore runs it *there*.

**What:** a spawn that goes wrong can create files or directories in
`userspace/oils/`, and because git does not track empty directories, `git status`
shows nothing. Any later test whose expectation depends on what globbing finds in
the cwd then fails for an invisible reason.

**How it was found.** A bug in this session's `$PATH` work briefly spelled
`cmd.exe`'s own path with `/`; `cmd.exe` re-parsed that as switches and created
four stray empty directories — `b`, `hi`, `E`, `.exe`. Three unrelated tests then
failed (`length_specials_take_only_an_operator`,
`an_unclosed_bracket_is_a_literal_word_not_a_pattern`,
`compound_while_read_from_heredoc`) because `[${1^}]` = `[Ab]` is a glob and it
matched the stray `b`. The failures reproduced at a clean `HEAD`, which is what
made the cause hard to see. `rmdir`-ing the four fixed all three.

**Proper fix.** `run()` should give each shell a scratch working directory of its
own — a `tempfile::TempDir` per test, removed on drop — so a test can neither
observe nor create anything in the source tree. That also makes the glob tests
say what they mean instead of depending on the crate directory happening to be
tidy. The differential harness already does exactly this for corpus cases
(`scripts/osh-bash-diff.py` runs each in a fresh temp dir); the unit tests should
match.

**Impact.** Not a shipping bug — it is a test-isolation hazard that turns an
unrelated failure into a mystery. Medium priority: it has already cost one
bisect.
