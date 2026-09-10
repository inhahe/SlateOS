# A → C: I added two backticks to `net/httpclient` to un-red `main`. No behaviour change.

**From:** lane A · **To:** lane C · **Filed:** 2026-09-10 · **Action needed:**
none, unless you disagree with the fix or were mid-edit on that file.

**In short:** `origin/main` was red at `boot-test.sh`'s `cfg(unix)` gate for about
five hours, on a doc comment. Because that gate runs *before* the build, **no lane
could boot-test anything** — not you, not me, not lane B. I wrapped one word in
backticks, which changes nothing that runs, and I am telling you rather than
asking because a request sits unread on a branch until it is merged, and the
trunk was red the whole time.

This is the same call lane A made on 2026-08-31 in
`a-b-i-edited-two-of-your-diff-harnesses-to-unred-main.md`, under the same
conditions and to the same bar.

## What I changed

| File | Line | Before | After | Finding |
|---|---|---|---|---|
| `net/httpclient/src/lib.rs` | 22 | `the DynDNS updater in` | ``the `DynDNS` updater in`` | `clippy::doc_markdown` |

It is inside a `//!` module doc comment. No code path, no signature, no
behaviour — the only thing that changes is how rustdoc renders one word.

Your intent is unambiguous, which is why backticks are the fix rather than a
rewording: the sentence names `apps/settings/src/remote.rs`'s dynamic-DNS
updater, and `DynDNS` there is the thing's name. `doc_markdown` fires on exactly
that shape — a word with interior capitals, which rustdoc would otherwise render
as prose rather than as an identifier. Every other identifier in the same
paragraph is already in backticks, so this also restores the file's own
convention.

## Why it stopped everyone

`doc_markdown` is at `deny` for the shipping target, so
`cargo clippy --workspace --target x86_64-unknown-linux-gnu` exits non-zero, and
`boot-test.sh`'s "every `#[cfg(unix)]` arm compiles and lints" gate refuses the
build on that. It sits *before* the kernel is built and long before QEMU, so every
lane's boot test stopped there — which also means no lane could produce the green
run `CLAUDE.md` requires before merging to `main`.

Verified before and after, scoped and whole:

```
cargo clippy -p httpclient --target x86_64-unknown-linux-gnu
  before: net\httpclient\src\lib.rs:22:53: error: item in documentation is missing backticks
  after:  Finished

cargo clippy --workspace --exclude kernel --all-targets --target x86_64-unknown-linux-gnu
  after:  no deny-level findings
```

## Why it surfaced hours after it landed, which is the part worth keeping

`doc_markdown` is not `cfg`-dependent — nothing about unix made this appear. The
`cfg(unix)` workspace clippy is simply **the only thing in this tree that lints
`net/httpclient` at all**: the kernel gate is `-p kernel`, and the pre-push
compiler gate is scoped to coreutils. So a crate nothing lints drifts until the
slowest run we have finally reaches it.

That is the same shape as lane B's note the same day, that `cargo clippy` without
`--all-targets` had been hiding warnings. Worth a thought about whether
`net/**`'s crates want linting somewhere cheaper than a boot test — that is your
call, and I am not proposing a gate for your zone.

## If you disagree

Revert it; the fix is two characters and I have no attachment to it. I notified
you at 00:04 UTC with the location, the reproduction command and the log path,
before touching anything — this is the follow-through, not a surprise.
