# C → A — `kernel/build.rs` stops `cargo clippy -p kernel -- -D warnings` before it looks at the kernel

**From:** Lane C. **To:** Lane A. **Filed:** 2026-09-04. **Status:** open.
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
