## TD-B-COREUTILS-PRINT-THE-HOSTS-ERROR-TEXT (lane B, 2026-08-22) — FIXED 2026-09-16

**Closed: the ratchet is empty.** `scripts/host-errmsg.py --check` reports
`0 file(s) affected; 0 not in the baseline; 0 baseline line(s) now stale`, and
`scripts/host-errmsg-baseline.txt` has 0 entries.

**The last two bins were `diff` and `patch`**, 5 sites each. `patch` was
half-converted -- one site already called `coreutils::errmsg::strerror` while
five interpolated the host's `Display` -- which is exactly the state this entry
warns about: a half-converted bin prints two wordings for the same failure, so
the reader cannot tell which one the next error will use.

**The counts in the section below are a month stale and were never corrected.**
It says 92 sites across 29 bins. When measured today the answer was 2 files.
The work was done incrementally and nothing updated the prose, so a reader
picking this up would have budgeted for 29 bins and found two. The live figure
is whatever `host-errmsg.py` prints; this paragraph is the only part of the
entry that should be trusted about scale.

**One conversion I made and reverted, because the distinction matters.**
`patch.rs:1489` is `diag!("patch: {e}")` where `e` is a `String`, not an
`io::Error`. It was never a finding and the scanner never reported it. I
converted it anyway, because I worked from the `{e}` SPELLING rather than from
the scanner's list, and `strerror(&e)` does not compile against a `String`.
The compiler caught it in five seconds -- but the same mistake against a type
that happened to satisfy the signature would have silently replaced a
domain-specific message with a POSIX errno sentence. Convert the sites the
tool names, not the ones that look similar.

**And an argument list that needed reordering**, which the entry predicts: the
multi-line `diag!("patch: cannot create backup {}: {e}", ...)` became two `{}`
with one argument. That is why the guidance says to bind `let why = strerror(&e);`
rather than inline -- advice I did not follow at the multi-line sites and
should have.

39 + 70 tests pass across the two bins; clippy has no deny-level findings and
no new warnings at any converted line.

The description below is kept in the tense it was written in.

**The original entry, 2026-08-22, follows.**

**In short:** When a tool like `cp` or `tar` fails to open a file, it prints a
sentence explaining why — "No such file or directory". That sentence does not
come from us; it comes from whatever operating system the program was *compiled
on*. Compiled for SlateOS it reads the way every Unix reads. Compiled on the
Windows machine we develop on, the very same failure prints "The system cannot
find the file specified. (os error 2)" instead. So 29 of our 85 tools say one
thing in production and a different thing in every test we run, and a test that
checks the wording is either wrong on the real OS or wrong on the dev machine —
it cannot be right on both. We already have the one-line fix; it has been
applied to 5 tools and not to the other 29.

### Why the wording is not cosmetic

`userspace/coreutils/src/errmsg.rs` states the principle in its own module doc,
and it is the reason that file exists at all:

> the message is an interface. A shell script that does
> `2>&1 | grep 'No such file'`, a test that asserts a diagnostic, and a person
> reading a log all read it.

POSIX names these strings (`strerror(3)` — the C function that turns an error
number into a sentence), every Unix uses the same ones, and scripts written
against any Unix expect them. A tool that substitutes Windows' phrasing is not
merely differently worded; it is unmatchable by anything written for a Unix.

### Where

The defect is a *format string*: an `io::Error` interpolated directly, so Rust
calls the platform's own `Display` for it.

```rust
// wrong -- prints whatever the host said
eprintln!("cp: cannot stat {}: {e}", quoteaf_os(src));
```

Measured 2026-08-22 by `scripts/host-errmsg.py`: **92 sites across 29 bins**
under `userspace/coreutils/src/bin/`. The head of the distribution:

```
   17  tar.rs        4  patch.rs      2  nohup.rs
   11  fetch.rs      4  cmp.rs        2  diff.rs
    8  dd.rs         3  xargs.rs
    7  chmod.rs      3  stat.rs      1 each:  bc chown df ed find ls md5sum
    6  tee.rs        2  renice.rs            more nice realpath sh sha256sum
    6  du.rs         2  readlink.rs          strings time_cmd touch
```

Five bins — `rm`, `mv`, `cp`, `ln`, `mkdir` — were converted in commit
`f516898f7` (18 sites) and are clean. They are the worked example.

### What the correct fix looks like

`coreutils::errmsg::strerror` already exists and already does the whole job: it
maps `std::io::ErrorKind` to the POSIX sentence, falling back to the host's text
only for the kinds Rust does not name. Per site:

```rust
let why = strerror(&e);
let _ = writeln!(err, "cp: cannot stat {}: {why}", quoteaf_os(src));
```

Bind it to a `let` rather than inlining `strerror(&e)` into the format string.
Most of these are multi-line `writeln!` calls, and inlining forces the argument
list to be reordered — a much larger diff for no gain.

Convert a **whole bin at a time**. A half-converted one prints two different
wordings for the same failure depending on which line reported it, which is
worse than one wrong wording consistently.

Watch for one thing while converting: `rm.rs` had a hand-written `NotFound`
arm that existed to make `-f` quiet. That is matched on `ErrorKind`, not on the
message, so it survives the change untouched — the branch decides whether to
speak, `strerror` decides the words. Don't fold the two together.

### Why a gate as well as a burn-down

The defect is invisible where we work. The dev host prints the *wrong* text, so
a test asserting the *right* text fails on the machine that runs it, and the
natural response is to assert nothing — which is why none of the 29 bins has a
test that would have caught this. Fixing 29 bins does not stop the thirtieth.

`scripts/host-errmsg.py` is gate 6 in `scripts/hooks/pre-push`
(`ALLOW_HOST_ERRMSG=1` to bypass). It records the 29 in
`scripts/host-errmsg-baseline.txt` and fails only on a bin that is *not* there,
so the backlog is frozen and can only shrink. A finding is keyed on the file
rather than the site, matching the unit of repair above; `--list` prints the
individual sites for whoever is doing the work.

Three shapes are deliberately not reported, and each is pinned by
`--selftest` so the exclusion cannot silently widen:

| Not reported | Why |
|---|---|
| `assert!(ok, "{err}")` and friends | Test scaffolding, read by a developer looking at a failure. There is no POSIX wording to get wrong. Seven bins' *only* hit was one of these — three of them bins that had just been converted, and a gate that reports its own fixes is a gate nobody believes. |
| `write!(f, "{e}")` in a `Display` impl | A wrapper error forwarding its source. Nothing there for `strerror` to translate. Recognised by the sink being named `f`, the formatter's conventional name. |
| `eprintln!("tar: {e}")` — the *whole* message | The top-level print in `main`, where the error already is the entire message. In every bin here it carries a `getopt::Error` or a `String`, never an `io::Error`. This exemption is what lets the target be zero rather than "one per bin". |

One genuine false positive is in the script's `IGNORE` table with its reason:
`awk/fmt.rs` formats `%e` output as `format!("{s}{e}{sign}{:02}", ..)`, where
`e` is the exponent character. A name collision, and the only one in 85 bins.

### Interaction with the argv conversion

This travels with `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`. Both are
per-bin rewrites of the same 30-odd files, both touch the diagnostics, and doing
them separately means reading and re-reading each bin twice. The five converted
so far had both done in one pass (argv first, error text immediately after), and
that is the order to keep: the argv conversion is what introduces the
`writeln!(err, ..)` sink in the first place.
