## `A-MODULE-SELF-TEST-ASSERTIONS-WERE-GUARDED-BY-NOTHING` (lane A, 2026-08-29) -- **fixed by a new gate**

**What it is.** ~300 assertions of the shape
`assert!(formatted.contains("some text"))`, inside module `self_test`
functions all over the kernel, had no static check of any kind behind them.
The sibling gate `scripts/check-selftest-wording.py` covers only kshell rungs
-- it resolves a *command word* to the text that command prints -- and a
module self-test that formats a string and asserts on it has no command word
to resolve, so the sibling never looked at one.

**Why "no check" means "no check at all."** `kernel/Cargo.toml` carries
`test = false`: a bare-metal binary supplies its own `panic_impl`, so the
kernel's assertions cannot run on the host. The only thing that executes one is
an eleven-minute QEMU boot. `cargo build` and `cargo clippy` are green on a tree
whose self-test panics on the first assertion it reaches.

**The variant that is worse than a panic.** `net::dashboard` asserted
`metrics.contains("os_net_rx_bytes_total ")` -- with a trailing space, meaning
"the sample line, not just the header." The `# HELP` and `# TYPE` lines satisfy
that on their own. When the metric gained a `source` label its sample line
became `os_net_rx_bytes_total{source="..."} N`, and the assertion went on
passing against a build emitting no samples at all. A panicking assertion at
least announces itself on the next boot; this one guarded nothing and said so
in the same words as a working test.

**The gate.** `scripts/check-selftest-format-wording.py`, run from
`scripts/boot-test.sh` beside its sibling. It enforces: *every fragment a
`self_test` requires a string to contain must be text some string literal in the
kernel can actually produce.* Three things make it usable rather than noisy:

| Rule | Why |
|---|---|
| The pool excludes *consuming* positions (`contains`, `starts_with`, `assert_output_contains`, ...) rather than enumerating producing ones. | A taxonomy of producers is a list that will be incomplete, and every omission is a false accusation. Over-approximating the pool errs toward missing a defect; under-approximating it errs toward the gate being switched off. |
| Acceptance is crate-wide verbatim first, then same-file alignment against a format string. | A module legitimately asserts on another module's wording (`net::netstat` asserts `"ESTABLISHED"`, which `TcpState` spells). Widening the *alignment* stage to the crate would make almost anything producible. |
| Only *presence* assertions are targets. | `if cpu_text.contains("processors:") { fail }` and `assert!(!x.contains(L))` demand the opposite; for them "nothing produces this" is the state being enforced, and reporting them would accuse a working regression guard. |

**22 exemptions, each checked by hand** (`ALLOWED` in the script, with reasons).
They are all text the code *computes* -- a byte count scaled to `3.0 MiB`,
`Content-Length: 9`, base64 `YWRtaW46c2VjcmV0`, a dot-stuffed SMTP line -- or
pasted together without a format string, as `http` does with
`extend_from_slice(b"Content-Type: ")` and `journal` with
`push_str(name); push_str("_hex\":\"")`. Modelling concatenation was considered
and rejected: `RX packets:` is `RX packets` plus a colon, and a colon is a
literal somewhere in every large program, so admitting it would accept the very
bug the gate was written for.

**A fix in the shared helper.** `producible()` in `check-selftest-wording.py`
expressed its alignment as a regex whose "suffix of segment" alternation has one
branch per byte. `net::dashboard` formats its 16 KiB HTML page from one literal
with 48 segments: 1128 segment pairs, each compiling an alternation of thousands
of branches, which took **minutes for a single fragment**. It was also *wrong* --
`.*?` between two alternations commits to one split and never reconsiders, so an
alignment existing under a different split was silently missed. Replaced with
direct enumeration of the three placements that can ever carry the run. The
whole gate now runs in 19 s; the sibling's verdict on kshell is unchanged
(502 assertions, 7 allowed).
