## B-POSIX-FOUR-MORE-PROCESS-GLOBALS-ARE-RACED-BY-THEIR-OWN-TESTS (lane B, 2026-08-22) — FIXED 2026-08-22

**In short:** A C library has a handful of functions that remember something
between calls — `strtok` remembers where it stopped, `dlerror` remembers the
last error, `umask` remembers the mask. That memory is *one slot for the whole
program*, on purpose: that is how POSIX defines these functions. But `cargo
test` runs tests on many threads at once, so several tests were writing to the
same slot simultaneously and reading back each other's values. One of them
started failing; the rest had not yet, but had the same defect.

**Why this entry exists separately from the two above it:** those two were found
because they *failed*. These were found by then going and reading every
process-global in the crate, which is what should have happened the first time.

### How it surfaced

`cargo test -p coreutils -p posix` — the two crates together, so the machine is
busier than `posix` alone — failed once on:

```
---- string::tests::test_strtok_basic stdout ----
thread 'string::tests::test_strtok_basic' panicked at posix\src\string.rs:4114:9:
assertion failed: !tok2.is_null()
```

Ten consecutive runs of `-p posix --lib` on its own had passed immediately
before. That is the load-sensitivity signature: the same code, a busier
machine, a different answer.

### What was actually wrong, and why `strtok` was the serious one

| Global | Where | Tests racing it | Worst case |
|---|---|---|---|
| `SAVED` (strtok) | `posix/src/string.rs:573` | 3 | **cross-thread memory corruption** |
| `DL_ERROR` | `posix/src/dlfcn.rs:35` | 15 | wrong/absent error message |
| `UMASK_VALUE` | `posix/src/file.rs:3030` | 3 | wrong previous-mask assertion |

The failed assertion is the least of what `strtok` can do. Each test's buffer is
a local on *its own thread's stack*. `SAVED` points into whichever buffer was
tokenised last, so under interleaving it points into **another live thread's
stack frame** — and `strtok` writes a NUL through that pointer to terminate the
token it returns. The observed symptom was a null return; the available symptom
was one thread silently overwriting a byte in another thread's frame.

`DL_ERROR` and `UMASK_VALUE` cannot corrupt anything — `DL_ERROR` only ever
holds pointers to `'static` strings, and `UMASK_VALUE` is a plain integer. They
are ordinary flaky-assertion races, and both would have failed the workspace
gate eventually.

`dlerror`'s test count is high (15, not 6) because `dlerror` is a *destructive*
read: it returns the message and clears the slot. So a test that merely calls
`dlopen` is a writer that can refill a slot another test just asserted was
empty. Being a writer is enough to break a reader, so every test that calls any
of `dlopen`/`dlsym`/`dlclose`/`dlerror` had to take the lock, not just the ones
with `dlerror` in the assertion.

### Fixed 2026-08-22 (lane B)

A `Mutex` per global in the test module — `STRTOK_TEST_LOCK`,
`DL_ERROR_TEST_LOCK`, `UMASK_TEST_LOCK` — taken as the **first statement** of
each affected test, because in all three cases the indivisible unit is the whole
"set a known state, provoke, read it back" body and not any single call. Poison
is recovered with `unwrap_or_else(PoisonError::into_inner)` so one genuine
failure reports once instead of poisoning up to fourteen siblings and burying
the cause. This is the idiom `getopt.rs`, `crypt.rs`, `libintl.rs` and
`error.rs` already use in this crate; these three modules were simply the ones
that had not adopted it.

Thread-locals — the fix used for `search.rs`'s counters — are **not** applicable
here. That fix works when state is only *incidentally* shared; these three are
shared *by specification*. `strtok` with a per-thread save pointer would be
`strtok_r`, which already exists next to it.

**The production `// SAFETY: Single-threaded access` comments were the root, and
all four are rewritten.** Each asserted a fact — "this is single-threaded" —
that nothing in the crate established and that the crate's own test suite
falsified. What is true is an *obligation*: POSIX specifies these interfaces
around process-global state and does not make them thread-safe, so serialising
is the caller's job. The new comments say that, name who discharges it, and
record that the previous wording was false. A SAFETY comment stating an
unchecked fact is worse than no comment, because it tells the next reader the
question has already been considered.

### What was checked and left alone

`ctype.rs`'s `CACHED` locale-table pointers (`:336`, `:394`, `:408`) are a
memoisation: every writer stores the *same* address of the *same* static table,
so a race stores the value that was already there. Left as is.

`crt.rs`, `fdtable.rs`, `aio.rs`, `dirent.rs`, `pthread.rs` and `perthread.rs`
reach their globals through the `perprocess!`/`perthread` macros, which is a
separate mechanism with its own story and was not part of this pass.

### The check that now covers this

All five instances were found by a flake or by someone reading, so the sixth
would have been too. `scripts/raced-globals.py` closes that: it flags a
*resettable* `static mut` / `static Atomic*` reachable from two or more
`#[test]` functions with no lock and no thread-local between them, and
`scripts/hooks/pre-push` gate 3 refuses a push that introduces one.

Three things make it a check that survives contact rather than one that gets
deleted:

- **It is a ratchet.** `scripts/raced-globals-baseline.txt` records the 40
  pre-existing instances, and `--check` fails only on a global that is *not* in
  it, so the backlog can be worked down without the gate being red on the day it
  lands. The file only ever shrinks. A false positive belongs in the script's
  `IGNORE` table, which records *why*; the baseline records only *that*.
- **A resetting write is required.** A `static COUNTER: AtomicU64` that is only
  ever `fetch_add`ed is the pattern that *prevents* this bug — every caller gets
  a distinct value and a race cannot lie. Demanding a `store`/`swap`/
  `compare_exchange`/`addr_of_mut!`/assignment cut the report from 84 globals
  to 48; the three largest entries it removed (106, 88 and 35 tests) were all
  unique-id counters.
- **The lock hint propagates one hop.** `getopt` and `environ` serialise via
  `reset_getopt_state()` and `lock_env_for_test()`, whose *call sites* contain
  no word resembling "lock". Following the same call graph used for
  reachability is what stops the tool flagging the code that already did it
  right — the failure mode that gets a linter switched off.
- **`#[cfg]` is evaluated, not ignored.** A global the host build never compiles
  cannot be raced by a host test. This was not a theoretical concern: the first
  entry examined from the backlog, `unistd.rs`'s `NO_NEW_PRIVS`, is a
  `thread_local!` on host and a `static AtomicBool` *only* under
  `#[cfg(target_os = "none")]` — it is already the fix this tool exists to ask
  for, and 20 host tests were being credited with racing the bare-metal half
  they cannot link against. Eight of the original 48 were this, so the tool now
  parses the predicate under "`target_os = "none"` is false, `test` is true,
  everything else unknown-and-assumed-true". It has to genuinely parse: the tree
  writes `#[cfg(any(target_os = "none", test))]` twenty times, and *that* item
  is compiled on host via the `test` arm, so a rule that merely grepped for the
  string would have excused all twenty. Unknown predicates resolving to "true"
  is the safe direction — the tool can fail toward noise, never toward silence.

That left 41, of which one more was a genuine false positive with a reason worth
recording rather than baselining: `perthread.rs`'s `HOST_FALLBACK` is reached
only from `current()`'s `try_with(…).unwrap_or(&raw mut HOST_FALLBACK)` arm,
which is taken only once a thread's TLS has *already been destroyed*. A `#[test]`
body always runs with live TLS. It is in `IGNORE` with that sentence attached.
**Backlog: 40.**

### First burn-down: 40 → 32, and the fix is not always a lock

Eight entries closed in the first pass, by two different fixes — which is the
point, because picking the wrong one produces a comment that lies.

*Locked* (7): `stdlib.rs`'s `RAND_STATE`, `RAND48_STATE`, `OLD_SEED` and
`L64A_BUF`, and `syslog.rs`'s ident/options/mask. These are shared **by
specification** — POSIX mandates one `rand` sequence, one `drand48` sequence,
one `l64a` return buffer and one syslog configuration per process — so they
cannot stop being shared, and the fix is a test-only `Mutex` taken as the first
statement of every touching test. Three separate locks in `stdlib.rs` rather
than one, because the three pieces of state are unrelated and a single lock
would serialise 21 tests that mostly do not contend; `OLD_SEED` rides the
`drand48` lock because `seed48` writes it and `RAND48_STATE` in one call.

*Converted* (1): `unistd.rs`'s `HOSTID`. **I started to write a lock for this
one and the lock would have been wrong** — worth recording, because the error
is exactly the defect class this whole entry is about. I had drafted a
`HOSTNAME_TEST_LOCK`, 26 guards, and a `sethostname` SAFETY comment asserting
that "two concurrent writers can leave the buffer holding one name and the
length belonging to another." Then I read `posix/src/perprocess.rs` and found
the hostname is not a `static` at all: it is `process_global!`, which expands
to a `thread_local!` on the host. The hostname was already per-thread. The
claim was false, and it was false in the specific way the self-review rule
warns about — *a SAFETY comment stating an unchecked fact is worse than none,
because it tells the next reader the question has been considered.*

The real defect was the opposite of the one I had written up: `HOSTID` was a
plain `static AtomicI64` while the hostname it is *derived from* was
per-thread. Two halves of one piece of state with two different sharing rules
— so a test could set the hostname and read back a hostid derived from another
thread's. The fix is to make the halves agree: `HOSTID` is now a
`process_global!` too, which is design-decisions.md §110's established answer
(§110 deleted a 138-site `CAP_TEST_LOCK` in favour of per-thread state,
rejecting "keep the lock" because it "leaves a live hazard behind an unwritten
convention"). Zero test guards, one baseline entry gone.

The discriminator, stated so the next 32 don't need re-deriving: **ask whether
POSIX requires the state to be shared.** If it does, it cannot stop being
shared and needs a lock. If it is shared only because someone wrote `static`,
`process_global!` it and the problem is deleted rather than guarded.

`reset_hostid_for_test()` was kept even though per-thread storage makes it
unnecessary for isolation, because several tests call `sethostid` twice and
need to return to the unset sentinel *within* one body. Its doc says so.

**Backlog: 32.** Confirmed green: `cargo test -p posix --lib`, 20515 passed.

### Second pass: 32 → 25, and the detector was reading comments

Four more closed by the discriminator above, and one real bug fell out of it.

*Converted* (2): `unistd.rs`'s `KLOG_READ_CURSOR` and `KLOG_CLEAR_FLOOR` — the
`klogctl` read cursor and clear floor, whose own doc comments already said
"per-process". They were plain `static AtomicU64`s, which is right on the
target and wrong under libtest; `SYSLOG_ACTION_READ` *consumes* from the
cursor, so a second reader sharing it sees entries the first already took.
`process_global!`, four one-line wrappers to keep `unsafe` out of the match
body, no test guards.

*Deleted* (1): `linux_seccomp.rs`'s `DUMMY` was a `static mut u8` existing only
to be a non-null pointer for an EFAULT check, carrying a "single-threaded test"
SAFETY note that was untrue. Nothing ever wrote through it, so the `mut` was
excusing a mutability that was never used: it is now `static DUMMY: u8 = 0`
with `(&raw const DUMMY).cast_mut()`, and there is nothing left to race.

*Fixed, and it was a real bug* (1): `crt.rs`'s `AT_RANDOM_BYTES` — see
**B-AT-RANDOM-WAS-RE-ROLLED-UNDER-A-RACE** below.

*Locked* (3 new findings): `pthread.rs`'s four TSD error-path tests bypassed
the `TSD_TEST_LOCK` their happy-path siblings take. Three of them provably
return before `tsd_lock()`, but `tsd_key_delete_returns_zero` does not:
`pthread_key_delete(0)` clears `TSD_DESTRUCTORS[0]` unconditionally, and key 0
is the *first* key `pthread_key_create` hands out — so unlocked it silently
wipes the destructor `tsd_create_set_get` had just registered. All four now
take the lock, because whether the other three touch shared state depends on
where each function's argument checks sit relative to `tsd_lock()`, which is
an implementation detail nobody editing those functions would think to
preserve.

**The detector was matching the global's name inside comments.** This is worth
recording because it inverted the tool's precision exactly where precision
matters. `time.rs`'s `settimeofday` contains

```rust
// as a (deprecated) timezone-only update.  We accept it as a no-op.
```

which made it a "toucher" of the `timezone` global and dragged **all 31**
`settimeofday` tests into the report — not one of which reads the zone. Prose
*about* a global is the single most likely place for its name to appear
without being an access, so this was not background noise; it was noise
concentrated on the entries a reader would most want to trust.

`strip_comments_and_strings()` now blanks Rust line comments, nested block
comments, string/byte/raw-string/char literals — space-for-character, so line
numbers and block matching still work against the result — and body matching
runs on that. Sixteen unit cases cover nesting, escapes, `r#"…"#`, the
lifetime-vs-char-literal ambiguity and an unterminated string.

Effect: `timezone` (31), `stdio.rs:FILE_POOL` (24) and `dirent.rs:GETDENTS_POOL`
(6) all fall to zero unserialised, and `--all` confirms each is still *seen*
and classified as fully serialised rather than having vanished from analysis —
the fix moved the tool toward precision, not toward silence, which is the
direction that matters for a checker nobody re-derives by hand. It also
*exposed* the three pthread TSD entries above, which had been hidden behind a
lock hint that was itself only in a comment.

**Backlog: 25.**

---

### Third pass: 25 → 22, all three by making the detector obey Rust's scope rules

No code changed in this pass. `signal.rs` declares `static RECEIVED`, `static
GOT` and `static CALLED` *inside* three separate `#[test]` bodies, each so a
nested `extern "C"` handler can record what it was passed — which is the
correct shape, not a hazard. The detector reported two tests apiece anyway: its
toucher regex (`\bhandler\s*\(`, `\bh\s*\(`) matched calls to unrelated
same-named helpers elsewhere in a 246-test file.

The fix encodes a language guarantee rather than a better heuristic. A `static`
declared inside a function body is a process-global *value* but a
function-local *name*: Rust does not put it in scope anywhere else, so no code
outside that function — including any other test — can name it, whatever it is
called. `analyse()` now records each global's declaration line, finds the
innermost enclosing function, and skips the global outright if that function is
a `#[test]`; otherwise it restricts the candidate touchers to functions nested
within the owner rather than the whole file. Sharpening the regex would have
been the wrong fix: it would have narrowed a false-positive class that the
language forbids entirely.

**I nearly made the hostname mistake a second time here.** The first
hypothesis was that the signal *disposition table* was racing, which would have
meant a lock. Reading `signal.rs` first showed it is already `process_global!`
(line 158) and therefore per-thread on host — so the fix belonged in the tool,
not the tree. That is now twice in three passes that "read the code before
writing the fix" turned a plausible source change into a no-op plus a detector
correction.

**Writing the scope rule exposed two more ways the tool failed toward
silence,** both fixed in the same pass:

- **The direct-name match ignored scope.** Only the *toucher* search was
  restricted to the declaring function. A test that merely contained the token
  — `let S = 3;` — was still counted as reaching a `static S` it cannot name.
  The two arms now differ deliberately: naming the global directly requires the
  name to be in scope, whereas *calling* a toucher works from anywhere, so only
  the first arm is scope-restricted.
- **Same-named statics collapsed, and ten of eleven declarations vanished.**
  `globals_` was a dict keyed by name, but a name identifies a global only at
  file scope. `userspace/oils/src/interp.rs` declares **eleven** separate
  statics called `COUNTER` and `kernel/src/sched/mod.rs` four called `WARNED`;
  keyed by name, only the last of each was analysed at all and the rest were
  never examined. It is now a list keyed by declaration site. The baseline stays
  keyed by `path:NAME` on purpose — line numbers churn, and a ratchet whose keys
  move on every edit is a ratchet that goes red for no reason.

  Today this changes no result: all eleven `COUNTER`s are `fetch_add`-only with
  no resetting write, so the reset rule drops them correctly, and `--check`
  still reports 22/0. It is a latent-correctness fix, not a new finding — but
  the failure mode it removes is the one that never announces itself.

**The checker now has a self-test, and the pre-push gate runs it first.**
`raced-globals.py --selftest` builds synthetic `.rs` files in a temp directory
and asserts `analyse()`'s classification across eight rules: the base case is
reported; a lock moves it to the serialised column; a comment does not make a
function a toucher; a `static` inside a `#[test]` is not reported;
`#[cfg(target_os = "none")]` is dropped but `#[cfg(any(target_os = "none",
test))]` is kept; a `fetch_add`-only counter is not reported; a static owned by
a non-test function is still reached through a call to its owner but not by a
test that merely reuses the name; and two same-named function-local statics are
analysed separately. Each rule exists because it had already produced a wrong
answer against the real tree, and each is a regex an edit can break silently.
The rule count in the summary line is computed from the registered rules rather
than a literal, so adding a case cannot leave the total lying.

The rules were verified to be capable of failing, not merely observed to pass.
Disabling the comment stripper, the fn-local scope rule, the lock hint, the
direct-match scope restriction and the name-collapse fix in turn each turned
**exactly** the corresponding rule red and no other — so the rules are
independent, and none is passing by accident. That mutation check is the point:
**a broken detector does not report a broken tree, it reports a clean one**, so
`--check` passing is meaningless unless `--selftest` passed first. The gate
therefore runs `--selftest` before `--check` and refuses the push with a
distinct message if the checker fails its own tests.

Also fixed: `_relpath()` raised `ValueError` on any path outside the repo root,
which meant the self-test could not analyse a temp file at all — the checker's
own tests were impossible to write against the function they exist to pin down.
It now falls back to the bare path.

**Backlog: 22.**

### Fourth pass: 22 → 20, by learning that a crate can have no test target at all

**In short:** the checker reported two racing tests in the kernel's network
code. Lane A checked before fixing them and found the tests do not run — the
kernel crate is built in a way that makes `cargo test` produce nothing to run,
so its 54 `#[test]` functions are dead code that is never even compiled. The
checker was right about the code and wrong about the consequence. It now knows
about that shape, and says so loudly rather than quietly dropping the crate.

Lane A's reply is `requests/a-b-raced-globals-flags-tests-that-cannot-run.md`.
`kernel/Cargo.toml` sets `test = false` on its `[[bin]]` and the crate has no
`src/lib.rs`, so `cargo test -p kernel` compiles and runs **no target**. My
`raw.rs` finding described a real interleaving between two `#[test]`s that can
never execute.

**The rule, and why it is skip-*and-report* rather than skip.** Lane A suggested
the tool skip crates whose test target is disabled, and the reason they gave is
the right one: "a checker that reports a race in code that never runs is right
about the code and wrong about the consequence, and the second is what people
act on." But a silent skip is the failure direction this whole tool is built
around — **a broken detector does not report a broken tree, it reports a clean
one**. If `test = false` ever appeared in `posix/Cargo.toml`, a silent skip
would delete my entire crate from the report and the `--check` gate would go
green on the way out. So the dead `#[test]`s are counted and printed in their
own section above the findings:

```
--- 54 `#[test]` fn(s) in 1 crate(s) with no test target: kernel ---
    They look like tests and are not: `cargo test` builds no target for them,
    so they never run and are never even type-checked. Not counted as raced
    below -- tests that cannot execute cannot interleave.
```

That turns a false positive into a true positive about a larger problem, and it
makes a crate falling out of scope an event rather than an absence.

**Every uncertainty in `crate_has_test_target` resolves to "keep the crate".**
The only path to `False` is a manifest that positively demonstrates no target is
left: a `[lib]` with `test = false` (or no `src/lib.rs`), no `tests/*.rs`, an
explicit `[[bin]]` list with `test = false` on every entry, and no autodiscovered
binary — `src/main.rs` unclaimed by an explicit `path`, or any `src/bin/*.rs` —
unaccounted for. A malformed manifest, an unreadable file, a Python without
`tomllib`: all return `True`. Getting this backwards costs silence, and silence
is the one outcome the tool exists to prevent.

**Cross-validated against a number derived independently.** Lane A counted 54
dead tests across 8 files by reading the crate; the new detector reports the same
total *and* the same per-file counts (balloc 3, driver 6, vfs_impl 13, pathutil
10, frag 7, httpd 7, raw 2, tty 6). Two methods that never saw each other
agreeing is stronger evidence than either alone — a regex that merely produced
"some number" would have looked just as plausible.

Self-test **rule 9** covers it, with six expectations built as synthetic crates
in a temp directory: the kernel shape → no test target; `src/lib.rs` present →
has one; an unclaimed `src/main.rs` → has one; a `tests/*.rs` → has one; an
ordinary crate → has one; an unparseable manifest → has one. `--selftest` now
reports 9/9.

The predicate was also run over **every tracked `Cargo.toml` in the repository**
— 2934 manifests, vendored crates included, not just the ones the walker reaches
— and exactly one comes back with no test target: `kernel`. That is the number
that makes the new section meaningful. Had it come back with forty, it would
have been describing a normal shape rather than a defect, and the right response
would have been a quiet skip after all.

**I removed the two `raw.rs` lines from the baseline, against lane A's
suggestion.** They suggested leaving them, on the grounds that "they are
currently the only honest pointer to a real problem, even if the reason they
fire is wrong." That was true when they wrote it and stopped being true with
this change: the new section points at the real problem directly, by name and
with a count. What a stale baseline line would buy now is the ability to excuse
a *genuine* reappearance — if the kernel ever gains a test target and those two
tests start running and racing, a pre-existing baseline entry means `--check`
stays green through it. That is the silence direction again, so the lines came
out. The baseline shrank by exactly two and gained nothing.

**Backlog: 20.**
