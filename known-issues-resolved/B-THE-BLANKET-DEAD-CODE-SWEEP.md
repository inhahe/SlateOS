## B-THE-BLANKET-DEAD-CODE-SWEEP — CLOSED 2026-09-13 (lane B)

**Lane:** B. **Status:** the line of work is finished. Every blanket
`#![allow(dead_code)]` in `userspace/`, `services/`, `init/` and `posix/`
is gone, `scripts/check-dead-code-allows.py` holds the line at zero, and
the eight remaining allows are narrowly conditional (`cfg(test)` or
`cfg(not(unix))`) and pass on the shipping target.

**What it started as, and what it turned into.** Lint hygiene. It became
the most productive defect-finding technique of the session, and the
reasoning is mechanical enough to reuse: **a model nothing constructs
means nothing populates it, and something is usually pretending it did.**

| crate | what came out of it |
|---|---|
| `gdb` | `MAX_BREAKPOINTS` declared, documented, never enforced; a 105-line duplicate tokeniser resolving `$rax` to its *index* |
| `wpa` | the supplicant model, orphaned when two fabricating personalities were deleted |
| `logind` | the entire write side implemented and reachable from nothing — filed separately |
| `systemctl` | **six commands printing work they never did**, including `enable` naming an exact symlink path and `poweroff` announcing a shutdown |
| `objdump` | nothing. A clean crate, and worth recording as such |
| `jq` | exit **0** on a runtime error; `$x` **hung forever**; `"\(.a)"` returned a **wrong answer** |
| `finger` | six helpers orphaned by the deleted `w`, five of them the *fixed* versions of a fabrication |
| `irqbalance` | an unrecognised option was ignored, so the program **daemonised** instead of failing |
| the tail | protocol headers parsed whole and rendered in part — bookkeeping, as expected once counts fell to single digits |

Nine crates produced a real defect; one produced a clean bill. Against
the grep for the word "stub", which produced a 45-comment reading list
containing two real findings, that is a far better filter — and it is
worth knowing it is now **spent**, because it only fires where a blanket
allow was hiding the evidence.

**The best of the findings were not visible in the code.** `jq -n '$x'`
produced no output to read; it had to be typed into the binary. The dead
enum variant was the index that said which feature to type. Reading the
source would not have found it, and neither would any test that existed.

**Four crate-level justifications, four that covered less than they
suppressed.** This is the durable lesson:

| crate | the comment above the allow |
|---|---|
| `gdb` | eight careful lines justifying its two neighbours, silent on `dead_code` |
| `wpa` | enumerated what it covered — and described personalities deleted three days earlier |
| `logind` | claimed the daemon event loop was unimplemented (it exists), citing a todo.txt note that never existed |
| `finger` | right about three fields, wrong about a fourth, and predated the orphans entirely |

**A list in a comment goes stale; an attribute on the item cannot.** That
is a stronger argument for the per-item form than "the comment did not
mention the lint", and it is why the gate permits per-item allows without
limit and refuses the blanket form outright.

**Method, for whoever repeats this.** `RUSTFLAGS="--force-warn dead_code"
cargo check -p <crate> --target <triple> --message-format=json`,
deduplicated by (file, line, message).

* `--force-warn` overrides a crate-level `#![allow]`; `-W` does not.
* `check` rather than `build`: no linking, so a linux target can be
  measured from a Windows host without a cross-linker.
* **Measure bin-only; verify deletions with `--all-targets`.** The two
  answer different questions, and I got this wrong twice — once deleting
  something the tests used, once reporting `main` as dead because the
  test harness supplies its own.
* A warning naming several fields points at **one** of them. Annotating
  the reported line leaves the rest warning; I fixed that by hand three
  times before automating it.

**Numbers.** 180 findings on `x86_64-unknown-linux-gnu` at the start
across 22 crates; 175 of those appear on every target. The count moved
three times before it was right — 208 measured on a non-unix host, then
180, and the crate total was an undercount until the detector learned to
read `cfg_attr`. Each move came from widening the instrument, not from
the tree changing, which is worth stating because a reader cannot tell
those apart from the number alone.
