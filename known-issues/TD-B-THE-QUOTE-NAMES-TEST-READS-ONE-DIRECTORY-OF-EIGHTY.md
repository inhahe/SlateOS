## TD-B-THE-QUOTE-NAMES-TEST-READS-ONE-DIRECTORY-OF-EIGHTY (lane B, 2026-08-22) — GATED 2026-08-23, backlog CLEARED 2026-08-23 (1798 → 0)

**In short:** We have a test that reads our own source code looking for error
messages that print a file's name without quoting it. Unquoted names are a real
hazard — a file can be *named* something that looks like a second error message,
and it will be printed as one. The test works, and it caught a live bug this
week. The problem is where it looks: it reads exactly one folder, the one
holding the 85 tools inside `coreutils`, and nothing else in the tree. Every
other utility crate — 777 of them — is unchecked, and a scan says they contain
**1796** of exactly the mistake the test exists to catch.

### Where

The test is `userspace/coreutils/tests/diagnostics_quote_names.rs`. Its whole
scope is set by one function:

```rust
fn bin_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bin")
}
```

`CARGO_MANIFEST_DIR` is `userspace/coreutils`, so the sweep is
`userspace/coreutils/src/bin/**.rs` and stops there. An integration test can
only be attached to the crate it lives in, so this is not an oversight in the
test so much as a consequence of the shape it was given.

### The measurement

`scripts/quote-names-scope.py` (added with this entry) re-implements the test's
two detectors — `bare_interpolated_name` and the hand-written-`'{name}'` check —
in Python, so the whole tree can be priced without moving the test first. It
skips comment lines, which the Rust version never needed to because no comment
in `coreutils/src/bin` happens to match; tree-wide, several do (including the
Rust test's own doc comments, which is how the port was validated against it).

```
$ python scripts/quote-names-scope.py
scanned 2902 .rs files under userspace
1796 would-be violations in 777 crates

   46  btrfs
   38  pulseaudio
   36  cups
   34  flatpak
   19  timeout
   ...
```

Run it with `--list` for the offending lines, or pass a path to scope it to one
crate. `userspace/coreutils` reports 4, all of them the deliberately-bad fixture
strings inside the test itself — i.e. `src/bin` really is clean, and the port
agrees with the test it is modelling.

### Why this matters more than the raw number suggests

The 1796 are not evenly distributed over harmless code. The heads of the list
are `btrfs`, `cups`, `flatpak`, `parted`, `losetup`, `mkfs`, `snapper` — tools
whose entire job is to take a path or a device name from the user and report
what went wrong with it. That is precisely the population where an
attacker-chosen name reaches an error stream.

### What the correct fix looks like

Not "widen `bin_dir()`" — an integration test cannot reach outside its own
crate's manifest directory in any clean way, and 777 crates is far past the
point where one test's failure output is actionable. Two better shapes:

1. **A workspace-level check rather than a crate test.** Promote the detectors
   into `scripts/` (the Python port is already there) and run it from the
   pre-push gate with a *baseline* file, exactly as
   `scripts/multicall-aliases.py` does for unreachable command names: the
   current 1796 are recorded as known, and the gate fails only on a *new* one.
   That stops the bleeding immediately at near-zero cost and turns the backlog
   into something that can be burned down crate by crate.
2. **Then burn it down**, highest-risk crates first (the path-handling list
   above), converting each site to `quotef_os` / `quoteaf_os`. This is the same
   sweep that was done once inside `coreutils/src/bin`, so the shape of the work
   is known.

Doing (1) without (2) is still a clear win — it is the difference between a
backlog and a growing backlog.

### Fix (1) landed, 2026-08-23 — the backlog can no longer grow

`scripts/quote-names.py` replaces `quote-names-scope.py` (the pricing tool is
subsumed by its default report; keeping two copies of the same two detectors
was an invitation for them to drift apart). It adds `--check`,
`--write-baseline` and `--selftest`, and is wired into `scripts/hooks/pre-push`
as **gate 8**, bypass `ALLOW_UNQUOTED_NAMES=1`.

```
$ python scripts/quote-names.py --check
ok -- 1798 known sites in 777 files (0 improved)
```

Three differences from the scope tool's 1796, all deliberate:

| change | effect |
|---|---|
| scans `services/`, `init/` and `posix/` as well as `userspace/` | +4 — all in `init/servicebus/src/main.rs`, all hand-written `'{name}'` around a **bus name**, which is supplied by whoever connects |
| the test's own fixture strings are in an `IGNORE` table, not the baseline | −4 |
| `posix/`, `services/` otherwise clean | ±0 — worth knowing, and now guarded |

`apps/` and `gui/` are outside the scan on purpose: they are lane C's, and a
gate that fails another lane's push for another lane's code is a gate that lane
switches off. If lane C wants the same guarantee it can add its roots to
`ROOTS`, which is a one-line change.

**The baseline is keyed on file + *count*, not on the line number.** The three
candidate keys and why this one:

* `path:line` — exact, and stale on the next commit that inserts a line above
  the site. A baseline that goes red for unrelated edits gets bypassed.
* `path:<source text>` — stable under line movement, but not under `rustfmt`,
  which rewraps argument lists routinely. Same failure, different trigger.
* `path:<count>` — immune to both, and catches the case that actually happens:
  a *new* site added to a file that already has some.

The residual gap is a 1-for-1 swap inside one file (fix one, add another, same
commit, same file) — the count is unchanged and the gate stays green. That is
rare enough to be worth the two cry-wolf failure modes it avoids, and
`coreutils/src/bin` is still covered exactly by the Rust test, which is where
most of the traffic is.

The `--selftest` is not decoration. This detector's signal lives *inside* a
string literal, so the natural way to make it "more correct" — reach for a Rust
lexer, as the sibling checkers do — would blank out precisely the text it
searches and report the whole tree clean. Gate 8 therefore runs `--selftest`
before `--check` and refuses the push if the checker cannot pass its own cases
(23 at first; 42 as of the correction below), exactly as gate 6 does for the
same reason.

**Part (2) — the burn-down — remains open.** Progress and the current ranking
are in the next two sections.

### Amendment 2026-08-23 — the ratchet was silenceable by `cargo fmt`

**In short:** Both detectors — the Python checker and the Rust test it was
ported from — matched a *physical line*. `rustfmt` routinely breaks a long
`eprintln!` so that the file name lands on a line with no macro name on it, and
when it does, the site becomes invisible to both. Running a formatter, which
nobody would think of as a security-relevant act, silently removed sites from
the count. **71 of them — 4% of the backlog — were hidden this way.**

It was found by arithmetic, not by suspicion. Converting `flatpak` should have
dropped the baseline by 34; it dropped by 37. Checking out the pre-conversion
`userspace/` and re-surveying showed the extra three were `loginctl` 17→16,
`snapper` 18→17 and `timeout` 19→18 — three crates I had not touched, whose
only change was the rustfmt-only commit landed just before. A checker a
formatter can silence reports a clean tree for a dirty one.

Both sides now join a wrapped call before matching:

* `scripts/quote-names.py` — `_delta()` (net bracket depth, skipping string and
  char literals) plus `join_wrapped_calls()`, which groups physical lines into
  logical ones and reports the *first* line so a report still points where a
  reader would go.
* `userspace/coreutils/tests/diagnostics_quote_names.rs` — `bracket_delta()`
  and `logical_lines()`, the same algorithm, with
  `the_detector_sees_a_call_rustfmt_wrapped` pinning it.

Literals must be skipped rather than counted: a format string is full of `{`
and `}` and often holds a paren of its own (`purged {n} job(s)`), so counting
those leaves every such call permanently unbalanced and it is never joined. The
join is bounded at 40 physical lines, because the scanner does not model every
Rust literal form — a raw string defeats it — and an unbounded join would then
swallow the rest of the file, turning one unrecognised line into a silent hole
over everything below it. Both regressions are pinned by tests.

**This is the one legitimate reason a baseline number may go up**, and the
generated baseline header now says so: it is a commit that changes
`quote-names.py` and no `.rs` file under the scanned roots. It has happened
once, here: 1641 → 1712.

### The bug the correction immediately found: `tar` could be made to forge a line of its own stderr

`userspace/coreutils/src/bin/tar.rs:605` had been wrapped by rustfmt and so was
invisible to the test *for as long as the test had existed*:

```rust
eprintln!(
    "tar: {}: unsupported entry type '{}'; skipped",
    quotef_os(&name),
    char::from(other)
);
```

`other` is the type-flag byte out of the archive's own header — exactly as
attacker-chosen as the name beside it, which is already quoted. `char::from`
printed it raw, so an archive crafted with `\n` in that byte writes a second
line into `tar`'s error stream that `tar` never wrote. Fixed in `05559d621` by
`quoteaf(&[other])`, which renders that byte as `''\n'`. Negative-verified:
reverting the fix makes `no_diagnostic_hand_writes_quotes_around_a_name` fail,
naming the line.

### Burn-down progress (part 2)

| crate | sites | commit |
|---|---|---|
| `btrfs` | 46 | `4271941fb` |
| `pulseaudio` | 38 | `892010376` |
| `cups` | 36 | `b8c50660b` |
| `flatpak` | 34 | `579d761cd` |
| `tar` (hand-written quotes) | 1 | `05559d621` |

`scripts/quote-names.py` gained a **`--fix PATH...`** mode, which is what makes
the rest of this tractable. It lives inside the checker rather than beside it
on purpose: a separate fixer would re-derive "what is a site" and would
silently skip lines the checker still counts. It was verified rather than
asserted — run against the `git show`-extracted pre-fix sources it reproduced
83 of the 84 already-hand-converted sites identically, and declined the 84th
for the right reason.

Current head of the ranking, post-correction:
`timeout` 20, `snapper` 18, `loginctl` 17, `stat` 17, `apparmor` 16,
`podman` 16, `selinux` 16, `pkg` 14, `mkfs` 13, `useradm` 13.
Total **1711 sites in 780 files**.

### Burn-down complete, 2026-08-23 — 1798 → 0, and the gate changes meaning

**In short:** every error and status message in lane B's tree that prints a
name the user supplied now puts quotes around it in a way the name itself
cannot escape. The backlog this entry was opened for is empty. Because the
gate compares against a recorded list, and the list is now empty, it stops
being a "do not make this worse" rule and becomes "do not do this at all":
the next such message that lands anywhere under `userspace/`, `posix/`,
`services/` or `init/` fails the push that carries it.

```
$ python scripts/quote-names.py --check
ok -- 0 known sites in 0 files (0 improved)
```

The last 444 crates went in five chunks of ~90, each taken through the same
five stages before it was committed: `quote-names.py --fix` (textual),
`quote-names-why.py --batch` (derive the rationale), `quote-names-wire.py
--batch` (manifest + `use`), `cargo clippy --fix`, then `cargo fmt`, `cargo
clippy -- -D warnings` and `cargo test`. All 444 test binaries pass.

**Two tools were added under the fixer, and the second is the load-bearing
one.** `quote-names-wire.py` writes the `quoting` dependency and the `use`
into a crate, and records *why* that crate needs it as a comment in its
`Cargo.toml`. Writing 444 of those sentences by recall would have produced
444 plausible sentences, some of them false — the provenance genuinely
differs from crate to crate, and a comment that says "this comes from argv"
about a value that comes from a config file is worse than no comment.
`quote-names-why.py` therefore *derives* the sentence from the crate's own
source: it finds the quoting call, walks the interpolated value back through
`let`/`for`/assignment/`push`/match-arm/parameter bindings — parameters
resolved **by position** against each call site, and scoped per function, so
a local named `governor` in one function is not evidence about another's —
and emits the sentence only if the value reaches argv. 430 of the 444 were
derived this way. The other 14 it **refused**, and every refusal was right:
`doas`'s caller name comes from the password database, `readelf`'s operand
arrives through a struct field, `sysctl`'s through an enum variant,
`tput`'s through a tuple pushed onto a vector (and, under `-S`, from stdin
rather than argv at all), `powertop`'s from the program's own table. Those 14
sentences were written by hand, having been checked.

The refusals were also the tool's own test suite: each one that turned out to
be a *tracer* gap rather than a real difference produced a named selftest
case and a fix. The last of those, `c79de5b47`, taught it that the value of
`if c { a } else { b }` is `a` or `b` and never `c` — before it, `rake-cli`'s
`let run_tasks = if tasks.is_empty() { vec!["default"] } else { tasks };`
ended the trail one step short of the argv branch it actually takes. The
inverse case is pinned too, because it is the reason the decomposition is
worth having: a block that *tests* argv and returns a constant must not
resolve.

**Five classes of real defect surfaced underneath the mechanical rewrite,**
all of which the type checker or the eye caught only because the rewrite
moved the line:

| class | example | why it matters |
|---|---|---|
| quotes around one of several argv values | `doas: {} is not allowed to run '{}' as {}` | quoting one of three quotes none of them; here it was a *security* decision's tail |
| a line meant to be pasted into a shell | `xh`'s `curl -X {} '{}'` | the quotes were the only thing between a URL containing `'` and a second command |
| the same value printed twice, quoted once | `tigervnc`'s `New '{}' desktop at {}` | the bare copy is the forgeable one |
| `byte as char` | four crates | a Latin-1 widening that reports 0xE9 as an `é` nobody typed |
| a genuine `char` off `.chars()` | `getopt`, `ldd` | fine, but needs `to_string`; `quoteaf_os` takes `AsRef<OsStr>` |

Commits: `8a48fd5cc`, `643e41d82`, `b10cff206`, `3639cb6b4`, `97e23c24b`
(the last also empties the baseline), over tooling in `ad754709e` and
`c79de5b47`.

**What is still not covered**, so that this is not read as a stronger claim
than it is: `apps/` and `gui/` remain outside `ROOTS` (lane C's tree — see
above); the detector only sees interpolation into a *format string*, so a
name concatenated into a `String` and then printed is invisible to it; and
the count-keyed baseline still cannot see a 1-for-1 swap inside one file,
though with the count at zero for every file that gap now requires adding
and removing a site in the same file in the same commit.

### Interaction with B-Q7

This is one of the two things that would be *lost* by moving a utility out of
`coreutils/src/bin` into a standalone crate, and it is why the `bc` move was not
reverted while B-Q7 is open (see `design-decisions.md` §359, amendment
2026-08-22). If B-Q7 is answered **A** — standalone crates canonical — then fix
(1) stops being an improvement and becomes a prerequisite, because at that point
*no* utility in the tree is covered by the test at all.
