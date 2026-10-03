## TD-C-SYSINFO-PARSES-HARDWARE-FIELDS-BY-DEFAULTING-TO-ZERO

**In short:** the code that reads hardware facts out of the files the kernel
publishes -- how much memory you have, how fast the processor is -- turns a
value it cannot parse into zero without saying so. Three helper functions that
would report the problem properly were written, are tested, and are called by
nothing.

**Corrected 2026-09-13, same day, and the correction is the useful part.**
When this was written it said "a machine with a garbled memory file and a
machine with no memory look identical *on screen*". That is false. `hwquery`
is one of the five modules in
`TD-C-FIVE-APP-MODULES-ARE-COMPILED-TESTED-AND-UNREACHABLE`: `sysinfo/main.rs`
declares `pub mod hwquery;` and never calls into it, so nothing this module
computes reaches a screen at all. The defect is real and the fix below is
still the right one, but its *urgency* was overstated by an entry written
from the code rather than from the call graph -- and an entry that overstates
gets worked before things that matter more. Whether to do it at all is
downstream of C-Q17, which asks whether these five modules are wired up or
deleted.

**Where it lives:** `apps/sysinfo/src/hwquery.rs`. About thirty sites of the
form

```rust
total_mb: kv.get("total_mb").and_then(|v| v.parse().ok()).unwrap_or(0),
```

and, unused since they were written, `SyscallProvider::parse_u64` /
`parse_u32` / `parse_f32`, each of which returns
`HwQueryError::ParseError { detail }` naming the field and the text that
failed. `cargo clippy -p sysinfo-app` reports them as never used, and that
warning is deliberately left standing: it is the only thing in the tree
currently pointing at this.

**Why it is not simply a bug fix.** Swapping the thirty sites over to the
helpers makes each field fallible, and then `read_cpu_info` has to decide what
to do with one bad field out of twenty. Failing the whole read means one
garbled line blanks the entire hardware panel, which is worse than what
happens today. So the fix is not mechanical.

**What the proper fix looks like.** Keep the struct total, add the report
beside it: each `read_*` returns its struct plus a `Vec<FieldError>`, built by
routing every site through the existing helpers and collecting the `Err`s
rather than discarding them with `.ok()`. A field that failed keeps its
default *and* is named, so the panel can draw it greyed with the raw text in a
tooltip instead of printing a confident 0. That satisfies the project rule
this violates -- never discard a `Result` with `.ok()` without a comment
saying why the failure is safe to ignore -- without making one bad byte fatal.

**How it was found:** clippy dead-code warnings surfaced while making the
twelve application palette conversions warning-clean. The helpers looked like
ordinary dead code; they are the intended implementation of a path that was
never wired up.
