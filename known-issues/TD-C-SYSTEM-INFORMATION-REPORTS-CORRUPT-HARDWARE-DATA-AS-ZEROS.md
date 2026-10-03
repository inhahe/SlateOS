## TD-C-SYSTEM-INFORMATION-REPORTS-CORRUPT-HARDWARE-DATA-AS-ZEROS

**Date:** 2026-09-14. **Lane:** C.

**In short:** the System Information program reads the kernel's hardware files
and, when a number in one of them is malformed, quietly shows **0** instead of
saying anything. A CPU whose `family` field is garbage is displayed as "family
0, model 0, 0 cores" -- which looks exactly like real data and is the one thing
a system-information tool must never do. There are 34 places this happens.

**The fix is already written and has never been called.** `SyscallProvider` has
three helpers -- `parse_u64`, `parse_u32`, `parse_f32` -- that do the parse and
return `HwQueryError::ParseError` with the offending text. They have four tests.
Their only callers are those tests. Meanwhile the live code, three lines below
them, reads:

```rust
family: kv.get("family").and_then(|v| v.parse().ok()).unwrap_or(0),
```

and all **nine** enclosing functions already return
`Result<_, HwQueryError>`. The error path exists, is typed, is tested, and is
unreachable. This is the `apps/automator` mutation-table shape again, and the
`apps/fileassoc` export/import shape from earlier today: work done carefully,
with tests, on something nothing calls.

**What the fix has to preserve**, and the reason it is not a blind
search-and-replace: *absent* and *malformed* are different. A field the kernel
did not report is ordinary and should keep defaulting; a field that is present
and will not parse means the file is corrupt. So each site becomes "no key ->
the default; a key that will not parse -> `?`", which is what the three helpers
already express.

| enclosing function | sites |
|---|---|
| `query_cpu_from_cpuid` | 11 |
| `query_memory` | 6 |
| `query_storage` | 4 |
| `query_processes`, `query_pci`, `query_network` | 3 each |
| `query_display` | 2 |
| `query_irqs`, `query_dma` | 1 each |

**A second, smaller thing in the same file.** `SYSFS_BASE` is
`"/sys/hardware"` and is unused, while the twelve constants under it each spell
`/sys/hardware/...` out in full. The base is declared once and then written
again twelve times, which is why the declaration could fall out of use without
anyone noticing. `concat!` takes literals rather than constants, so the fix is a
small macro holding the one literal.

**How this was found, which is the part worth keeping.** Not by reading the
file. `cargo clippy -p sysinfo-app` reported the dead helpers -- and nobody had
run that, because the habit is `-p sysinfo`, which reaches a **different crate**
(`userspace/sysinfo`). The directory is `apps/sysinfo`; the package is
`sysinfo-app`.

**That hazard is already filed and already gated** --
`TD-B-FIVE-CRATES-CANNOT-BE-REACHED-BY-THEIR-DIRECTORY-NAME` (lane B,
2026-09-10), with `scripts/check-crate-names.py` as Gate 18 of the boot test
refusing any *new* mismatch. Nothing about it is new here and this entry does
not restate it; `backup`, `indexer` and `sysinfo` are the three that resolve to
somebody else's crate rather than erroring, confirmed twice on 2026-09-14 by
two lanes using different methods.

What *is* worth recording is the consequence for this crate: `apps/sysinfo` has
been outside a check everyone assumed covered it, which is why dead code and
34 swallowed parse errors sat here undisturbed. The gate stops the *set* of
mismatches growing; it cannot make anyone type the right name, and for these
three a wrong name does not fail -- it succeeds about something else.

That is a third mode of the failure this file keeps recording, and it deserves
its own word: **substitution**. A check blind to a population, or to a
property, produces an *absence* -- something not looked at, an assertion too
weak -- which a careful reader can go hunting for. Substitution produces a
*presence*: a green line about real work that really happened, just not the
work anyone asked about. It also defeats the usual diagnostic. "Would this go
red if the thing were broken?" is satisfied -- it would, for the other crate.
The question that catches it is narrower: **did the check examine the thing I
named?**
