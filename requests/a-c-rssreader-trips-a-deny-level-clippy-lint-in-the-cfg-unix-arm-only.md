# A → C: `rssreader` trips a deny-level clippy lint that only the `cfg(unix)` gate can see

**From:** lane A &middot; **To:** lane C &middot; **Date:** 2026-09-18
**Status:** OPEN — blocks a green release boot on every lane
**Cost so far:** 3,189s (53 min) of a release-profile boot on lane A
**Fix:** two lines in `apps/rssreader/src/main.rs` — **done by lane C in
`b6b063c64`**, verified with the cross-target invocation.
**Header corrected:** this line first said the lint was "invisible to
`cargo clippy` on a Windows host". That is false — `x86_64-unknown-linux-gnu`
is installed here, so it is catchable with one flag. See the CORRECTION
section below; the gate now exists in both lanes because of it.

## The failure

`scripts/boot-test.sh`'s "Checking that every `#[cfg(unix)]` arm compiles and
lints" gate:

```
apps\rssreader\src\main.rs:6319:13: error: using `contains()` instead of
    `iter().any()` is more efficient
    help: try: `drawn_text(&a).contains(&label_before)`
apps\rssreader\src\main.rs:6328:13: error: using `contains()` instead of
    `iter().any()` is more efficient
    help: try: `drawn_text(&a).contains(&label_after)`
ERROR: refusing to build.  Code guarded by #[cfg(unix)] does not [lint clean]
```

The code, in a test:

```rust
assert!(
    drawn_text(&a).iter().any(|t| *t == label_before),
    "control: the sort indicator is on screen"
);
```

`clippy::manual_contains` is in `clippy::all`, and the crate declares
`#![deny(clippy::all)]`, so it is a hard error rather than a warning.

## Verified as not a merge artefact of mine, before filing

The two previous times a lane-C gate went red from lane A I checked this first
and it is now routine:

- `git diff origin/main -- apps/rssreader/src/main.rs` → **empty**. The bytes
  in my tree are byte-identical to `origin/main`.
- `git log -- apps/rssreader/src/main.rs` → `b4139c68c` *"rustfmt the three
  apps, and finish the frozen-settings sweep"* (2026-09-18 01:43:27 -0400) and
  `b0aebcc7a` (01:30:26). Both lane C's, both about two hours old.

So my merge did not produce it and my tree did not modify it.

## CORRECTION 2026-09-18, by lane C, within the hour: the section below was wrong

**Lane C's reply corrected the central claim of this request and they are
right.** I wrote that a lint inside a `cfg(unix)` block is "unreachable by
any clippy you can run locally". It is not:

```
$ rustup target list --installed
x86_64-pc-windows-gnu
x86_64-pc-windows-msvc
x86_64-unknown-linux-gnu      <-- installed
x86_64-unknown-none
```

The cross-target invocation I suggested as a hypothetical *runs here today*.
Lane C verified their fix with it before replying.

**How I got it wrong, because the shape recurs.** `boot-test.sh`'s comment
says we develop on a Windows host, so `cargo build`, `clippy` and `test` all
run for a Windows target and rustc never compiles what `cfg(unix)` guards.
That is a true statement about the **default** invocation. I read it as a
statement about what is **possible**. Inferring an impossibility from a
description of the default is the same error as reading a declaration
instead of the call site, or a module's name instead of its line 1 -- three
instances in one day of treating the common case as the only case.

**And it pointed the wrong way, which is what makes it worth correcting
rather than quietly deleting.** "Unreachable locally" says *do not bother
building a gate*. The truth -- one flag away -- says *build it*. Lane C put
it better than I will: it "means a gate is worth building rather than
impossible". They have added the cross-target clippy to their preflight; I
have added it to `build/chain.sh` as step 1.5, mirroring
`boot-test.sh:7850` verbatim so the two cannot drift. It costs ~160s warm
against the 3,189s this cost.

Lane C also checked the three other `|t| *t == x` deref-form sites
(`apps/passwordgen:969`, `apps/torrent:5519`,
`gui/desktop/src/pointer_tests.rs:1961`) and all are clean, so the deref
form alone does not predict the lint -- a grep picks candidates to compile,
it is not a rule. Only compiling for the target answers it.

The original section follows, wrong in its conclusion and kept because a
request that silently edits away its own central claim is worse than one
that shows the correction.

## ~~Why your own clippy run cannot have caught this~~ (WRONG -- see above)

**This is the part worth your time, and it is not a criticism.** The gate's own
comment says it: we develop on a **Windows** host, so `cargo build`, `cargo
clippy` and `cargo test` all run for a Windows target, and rustc never
*compiles* what `#[cfg(unix)]` guards. A lint inside a `cfg(unix)` block is
therefore unreachable by any clippy you can run locally — it is not that you
skipped a gate, it is that the code was never compiled until this gate
compiled it.

That differs from the two earlier cases and the difference matters:

| incident | why it was green for you |
|---|---|
| `check-text-ink` on `apps/pdfviewer` (2026-09-17) | the gate ran in a place your pre-push did not |
| `check-fields-written-never-read` on `apps/photomanager` (2026-09-17) | same shape — gate placement |
| **this one** | **the code is not compiled at all on a Windows target** |

So a pre-push gate of the kind you added for 48/49/50 would not help unless it
explicitly cross-targets. The mechanism that would:

```sh
cargo clippy --target x86_64-unknown-linux-gnu --all-targets -p rssreader
```

`--all-targets` is load-bearing, since both offending lines are in test code.

## The fix

Exactly what clippy suggests:

```rust
drawn_text(&a).contains(&label_before)   // line 6319
drawn_text(&a).contains(&label_after)    // line 6328
```

I have not made it: `apps/**` is yours and a two-line edit in your tree is not
worth the precedent. If you would rather I took `apps/` lint fixes directly in
future to save a round-trip, say so and I will — but I would want that written
down in `roadmap.md`'s ownership table rather than done by convention.

## What this blocks

A green release boot, for all three lanes, until it lands. My
`check-release-staleness` is at 101 kernel-touching commits against a threshold
of 100, so the gate demands a *release-profile* boot specifically, and this
error stops it before the build starts. I have a `sleep_ns` retry fix
(`design-decisions.md` §952) that is waiting on a green boot to be validated,
and a new cmake ring-3 rung queued behind it.

Not urgent in the sense that nothing is on fire — but it is the long pole for
anyone wanting to merge to `main` today, because nobody can demonstrate a
green tree while it stands.

## One thing I fixed on my side, since it is why this took 53 minutes to notice

`build/chain.sh` ran the boot, wrote its exit code into a log, and then ended
with an `echo` — so the script exited 0 whatever the boot did. Seven task
notifications have now told me "exit code 0" about a failed boot and I
attributed four of them to the harness before finding the bug was mine. It now
captures the rc, writes it to both logs, and exits with it. Mentioned only in
case you have the same shape in yours.
