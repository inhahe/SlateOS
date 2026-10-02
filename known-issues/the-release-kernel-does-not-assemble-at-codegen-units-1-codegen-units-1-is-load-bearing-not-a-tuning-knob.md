## The release kernel does not assemble at `codegen-units` > 1 — `codegen-units = 1` is load-bearing, not a tuning knob

**Lane A · found 2026-08-21 · Status: OPEN (does not affect any shipped or
tested build; blocks one option that would otherwise be attractive)**

**In short:** the kernel is built with a setting that tells the compiler
"produce one big chunk of machine code rather than sixteen small ones". That
setting was there for speed of the *finished* kernel. It turns out the kernel
also fails to build without it — so it cannot be relaxed to make builds
faster, which is a thing we would otherwise want to do, because the one-chunk
setting is what makes a release rebuild take 594 s instead of 42 s.

**Reproduction** (from `os-lane-a`, ~174 s to fail):

```bash
touch kernel/src/main.rs
cargo build --release --config 'profile.release.package.kernel.codegen-units=16'
```

```
error: <inline asm>:142:5: expected absolute expression
.if (664b-663b) > (662b-661b)
    ^
error: could not compile `kernel` (bin "kernel")
```

The variable is cleanly isolated: the identical tree, toolchain and command at
`codegen-units = 1` builds in 594 s and boots to `BOOT_OK` (run 4 of the Q46
series, same commit `8b481b0f2`). Only the codegen-unit count differs.

**Where it lives.** `kernel/src/alternatives.rs:262` — the assembly-time guard
inside `alternative_site!`, which refuses to build a replacement sequence
longer than the site it patches over. The macro is expanded from inside
`global_asm!` in the `isr_stub_no_error!` / `isr_stub_with_error!` macros
(`kernel/src/idt.rs:681,735,825`), once per exception vector, so a crate build
contains many expansions. The numeric local labels (`661:`…`664:`) are the
Linux `ALTERNATIVE` convention and are deliberately reused across expansions;
`661`/`662` bracket the site in `.text`, `663`/`664` bracket the replacement in
a `.pushsection .altinstr_replacement`.

**What is not yet known** — and this is the honest gap in this entry: *why*
splitting the crate into several codegen units makes one of those label
differences non-absolute. Each expansion is a single `concat!` string, so a
site and its record cannot be separated from each other, and both label pairs
are within one section each. The error names `<inline asm>:142`, which is
several expansions deep into a concatenated blob, so the assembler is seeing
multiple stubs in one unit and failing on one of them rather than on the first.

**The proper fix** is to find that out before changing anything: build with
`--emit=asm` at `codegen-units=16`, locate line 142 of the offending blob, and
establish which expansion breaks and what precedes it. Only then decide between
(a) making the macro robust across codegen units — e.g. deriving the lengths
without a cross-section subtraction, or emitting the guard into the same
section as the labels it measures — and (b) recording `codegen-units = 1` in
`Cargo.toml` as a *correctness* requirement with a comment pointing here, so
the next person to treat it as a performance knob is stopped at the comment
rather than at a 174 s build failure. Do not "fix" it by deleting the `.if`
guard: an oversized replacement silently shreds the instruction after the patch
site, which is exactly the failure the guard exists to make loud.

**Why it is filed rather than fixed now.** Nothing built or tested today is
affected — every kernel build in the tree, debug and release, is
`codegen-units = 1` (debug via `[profile.dev]`, which never sets it and so gets
the incremental default; release via `[profile.release.package.kernel]`). The
practical cost is that it removes an option from `open-questions.md` Q46: the
14× release-rebuild penalty measured today (42 s → 594 s for a kernel-only
edit) is largely the single codegen unit, and "build release but with 16 units"
would have been the obvious way to buy release-only bug coverage without the
penalty. That option is unavailable until this is understood, and Q46's write-up
now says so.
