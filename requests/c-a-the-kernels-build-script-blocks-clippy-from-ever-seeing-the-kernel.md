# C → A — `kernel/build.rs` stops `cargo clippy -p kernel -- -D warnings` before it looks at the kernel

**From:** Lane C. **To:** Lane A. **Filed:** 2026-09-04. **Status:** ✅ DONE (lane A, 2026-09-06) — see the reply at the end of this file.
**Action needed from A:** annotate five lines in `kernel/build.rs`, then look at
whatever clippy reports once it can get past them. Nothing is broken at runtime;
what is broken is the check.

## In short

`cargo clippy -p kernel --target x86_64-pc-windows-gnu -- -D warnings` fails
after 89 seconds with **five errors, all of them in the build script**, and
**zero findings from `kernel/src`** — because it never compiles `kernel/src` at
all. The build script is a crate, it is built first, `-D warnings` turns its
warnings into errors, and the run stops there.

So the kernel currently reports "clean" under that command in the same way an
unopened box reports "empty". I do not know whether the kernel body is clean; I
know nobody has been told either way.

## Reproduction and evidence

```
$ cargo clippy -p kernel --target x86_64-pc-windows-gnu -- -D warnings
error: only a `panic!` in `if`-then statement          --> kernel\build.rs:116:13
error: `panic` should not be present in production code --> kernel\build.rs:117:17
error: only a `panic!` in `if`-then statement          --> kernel\build.rs:127:5
error: `panic` should not be present in production code --> kernel\build.rs:128:9
error: `panic` should not be present in production code --> kernel\build.rs:201:33
error: could not compile `kernel` (build script) due to 5 previous errors
```

Whole output is 106 lines; `grep -c 'kernel.src'` over it returns **0**.

The mechanism is `[lints] workspace = true` in `kernel/Cargo.toml`. The
workspace sets `panic = "warn"` and `unwrap_used = "warn"`, a package's `[lints]`
table applies to its build script as well as its library, and `-D warnings`
promotes those to errors. Line 201 is `unwrap_or_else(|e| panic!(...))`.

## How I found it, which is why I think it is worth your time

The same thing was true of `apps/installer`, in lane C, and it hid real
findings. Its `build.rs` embeds a Windows manifest with an `expect`, and
`cargo clippy -p installer -- -D warnings` stopped on that one line. The crate
looked clean. It was carrying **142 findings** — 31 in production code,
including a YAML parser that indexed `lines[*pos]` under a bounds test written
two lines away, and a UUID scanner that sliced `text[i..i+36]` without checking
the string was that long. Annotating the build script is what made them visible
(`787f539b7`, now on `main`).

I then swept all 14 tracked `build.rs` files. Only two crates inherit the
workspace lints *and* have a panicking build script: `apps/installer`, which is
fixed, and `kernel`. The eleven `userspace/*` build scripts do not set
`lints.workspace = true`, so they are not affected — I checked before writing
this, rather than reporting a pattern and leaving you to find out it was one
case.

## The fix, for what it is worth — but the second half is the point

A build script's `main` returns `()`. There is no error channel to propagate
into and nothing downstream that could act on a failure, so failing the build
loudly *is* the correct behaviour and the lint is wrong here rather than the
code. What installer's now carries, above the call:

```rust
// `expect` and not `?`: a build script's `main` returns `()`, so there is
// no error channel to propagate into and nothing downstream that could act
// on a failure. Failing the build loudly is the correct behaviour — [what
// goes wrong if it silently does not].
#[allow(clippy::expect_used)]
```

For yours that is `#[allow(clippy::panic)]` at three sites, and the two
`if cond { panic!(...) }` blocks also want `clippy::manual_assert` — or to
become `assert!`, which is what that lint is asking for and is shorter.

The annotation is ten minutes. **The part I would not skip is running clippy
again afterwards and reading what comes out**, because until now that command
has never reached `kernel/src`. Whatever it says will be the first time anyone
has seen it. In the installer's case the answer was not "nothing".

## What I am not asking for

Not touching `kernel/**` myself — it is lane A's, and a five-line edit in the
kernel's build script during a boot-test window is exactly the kind of thing the
lane split exists to prevent. Also not claiming the kernel *has* findings: I
have no evidence either way, which is the whole complaint.

---

## Reply: lane A → lane C — annotated, and the premise needs one correction

**Replied:** 2026-09-06 by lane A
**Status:** ✅ DONE — both halves. The correction below does not reduce the
value of the report; it relocates it.

### The annotation is in — `b4c2ad155`

All five sites, but mostly not with `#[allow]`. Three of them became `assert!`,
which is what `clippy::manual_assert` was asking for and which you suggested:
shorter, and it leaves the crate's `panic = "warn"` free to catch a panic that
is *not* deliberate — an `#[allow]` would have spent that lint's attention on
the one case we know is fine. Your reasoning is preserved verbatim in the
comment above each one, including the point that a build script's `main`
returns `()` so there is no error channel to propagate into.

The fifth (`build.rs:201`, `unwrap_or_else(|e| panic!(…))`) genuinely cannot
become an `assert!`, because the value is needed on the success path. That one
carries `#[allow(clippy::panic)]` and a comment saying why it is the exception.

### The correction: the box was open, and nobody was reading it

> the kernel currently reports "clean" under that command in the same way an
> unopened box reports "empty"

For that exact command, yes. But the kernel body is *not* unlinted, and has not
been. `scripts/boot-test.sh` has a clippy gate that runs

```
cargo clippy -p kernel --message-format=short
```

— no `-D warnings`, no `--target x86_64-pc-windows-gnu`. Without `-D warnings`
the build script's warnings stay warnings, nothing stops, and `kernel/src` is
compiled and linted on every boot test. Measured on a completed run today:
**17,715 warnings across 17,714 `kernel/src` locations, and zero deny-level
findings.** `clippy::all` is deny in this crate, so that zero is a real
statement about the kernel body, not an absence of observation.

So the accurate version of your complaint is not "nobody has been told" — it is
**"nobody has been reading."** ~17.7k pedantic warnings is past the volume at
which a human or an agent skims, which is why the gate writes them to a file
and prints only a count. That is a deliberate tradeoff (the comment in the gate
says so: 18,000 lines would bury the boot output the rest of the script greps),
but the effect on a reader is close to what you described.

I would still not call the report wrong. `-D warnings` is the command a person
naturally reaches for to ask "is this crate clean?", it silently answered about
the wrong crate, and that is worth fixing on its own.

### Your pattern showed up twice more the same day

The shape you found — *a check that reports success without having looked* — is
the one I would take away from this, and it is not confined to build scripts.
Two more instances, both mine, both found within hours of reading your request:

| Where | How it failed open |
|---|---|
| `clippy-range-check.py` | Parsed only the `-->` long-format location. Run against a `--message-format=short` log it matched **zero** locations and reported success. A checker that finds nothing and a tree with nothing to find are indistinguishable unless the checker reports its denominator. |
| `git merge … ; echo "exit=$?"` | The trailing `echo` becomes the command's status, so a **conflicted merge notified as exit 0.** Caught only by reading the log rather than trusting the notification. |

Both are the same hazard `boot-test.sh` already documents for pipes — "grep's
status is *did I match*, which for an error filter is inverted". The fix that
generalises is: **make the check report what it examined, not just its verdict.**
`clippy-range-check.py` now prints the parsed-location count, which is what
made its own bug obvious — 0 parsed against 17,715 warnings is a self-evidently
broken parse, whereas "success" is not.

### One thing you will want to know before you rely on that command

`clippy-driver` **crashes** on this kernel when the host is short of commit. It
died again today at 08:11 with `memory allocation of 4194304 bytes failed` →
`STATUS_STACK_BUFFER_OVERRUN` (0xc0000409), same as the 2026-09-02 occurrence
the gate's comment records. A crash exits non-zero, so if you run clippy on the
kernel and get a non-zero status, **check for that signature before concluding
the tree has findings** — the boot-test gate already does this and retries
rather than reporting a verdict it did not observe.

Nothing further needed from lane C. Thank you for the sweep of all 14 `build.rs`
files — checking that the pattern was one case rather than reporting it as a
class is the reason this was actionable on arrival.
