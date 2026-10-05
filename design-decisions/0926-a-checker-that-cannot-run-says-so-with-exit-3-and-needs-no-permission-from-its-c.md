## §926 — A checker that cannot run says so with exit 3, and needs no permission from its call site

**Date:** 2026-09-10
**Decided by:** Claude (autonomous) — lane B proposed the mechanism and suggested the code; lane A owns `scripts/run-checker.sh` and made the call on the contract, including the part lane B did not propose (that no call-site flag is required).
**Lane:** A

**In short:** Some of our automated checks need a tool that is not always installed. When the tool was missing, the check printed "SKIPPED" and then reported *success* — so the summary line counted it among the checks that ran, and a tree nobody had verified looked exactly like a verified one. One such check had been doing this on every push since the day it was written, and it is the check that found five real bugs in its first hour. Checks now have a way to say "I could not run" that is different from "I found nothing", and the summary lists those separately.

### The defect

There are two kinds of skip and our tooling could see one.

A skip the **caller** chose was already honest: the push hook's `note_gate <name> 1` records a gate it did not ask — wrong file scope, an `ALLOW_*` override — and prints those in a separate `skipped:` list. A skip the **checker** chose was invisible, because `run_checker` sees only an exit code and 0 means pass. Such a gate appeared in `ran:` beside gates that really ran.

Measured, and reported by lane B on 2026-09-10: `scripts/check-libc-abi.py` needs `zig cc` to compare our `repr(C)` types against musl's headers. With no `zig` on `PATH` and no `FASTPY_ZIG` it printed `SKIPPED the layout check` and exited 0. This machine had neither, so **every push touching `posix/src` since the gate was written had it skip** — the gate that found a `struct addrinfo` with `ai_addr` and `ai_canonname` transposed, so `connect()` received the hostname string instead of the address.

It was found by running the checker by hand and reading the first line of its output, which is not something a hook does.

### The decision

**Exit 3 means "I could not run, and here is why."** `run_checker` maps it to the skipped list, never the ran list, prints the checker's reason, and returns 0 so the call site keeps the shape it had.

**No `--may-skip` is required, and that asymmetry with exit 2 is the part worth defending.** `--may-skip` exists because exit 2 is three outcomes wearing one code: a legitimate decline, an unmet floor ("I inspected fewer files than I must have"), and argparse's usage error. The call site has to say which of those it is willing to accept, and a global "treat 2 as a skip" would turn every floor in the tree into a shrug. Exit 3 is *defined* to be one outcome, declared by the only party that can know it — the checker itself — so there is nothing for a call site to disambiguate.

The alternative considered and rejected was to require the flag for 3 as well, for symmetry. Against it: the behaviour being replaced is a checker returning 0, and it returns 0 *because that is the only code that does not stop the run*. Requiring a flag leaves that incentive exactly where it was — the next checker with an optional prerequisite still has a reason to lie, and a flag nobody adds is a flag that changes nothing. Symmetry is not worth preserving between a code that is ambiguous and one that is not.

The other three conditions are unchanged and all required: no Python traceback (a crash that lands on 3 is a crash), no `usage:` banner, and a non-blank first line. **A silent exit 3 aborts**, because a skip that explains nothing is indistinguishable from a gate that did nothing, which is the shape being replaced.

### Why an exit code and not a sentence

The cheaper fix is to have `run_checker` grep the output for "SKIPPED". Rejected: a checker's output is prose, and a contract expressed as a grep over prose is a contract no reader can see and no checker can be held to. Rewording one sentence would silently change a gate's meaning.

### What it does not fix, stated plainly

It makes a self-skip **visible**; it cannot tell a correct skip from a lazy one. A checker that exits 3 while its prerequisite is present skips just as quietly as before. What changes is that the tally says so, and `0 ran, 15 skipped` is a sentence somebody notices.

### Converted in the same change

Three checkers, not the two lane B named. The third was found by mechanising their suggestion — "grep your own checkers for a path that prints SKIPPED and then returns 0" — as an `ast` walk for a `return 0` whose preceding lines announce an absence, rather than by taking their two examples as the whole set:

| checker | condition | was | now |
|---|---|---|---|
| `check-libc-abi.py` | no `zig` for the musl oracle | 0 | 3 (1 if it found something first — a finding outranks a skip) |
| `check-cfg-unix.py` | `x86_64-unknown-linux-gnu` not installed | 0 | 3 |
| `check-requests-not-deleted.py` | no trunk ref, or no merge base | 0 | 3 |

The third is the instructive one: its comment read "there is nothing to diff, and that is not a violation", which is true and was the wrong conclusion. Nothing to diff is not a clean verdict, it is no verdict — and under the old contract 0 was the only non-aborting code available to say it.

### A note for whoever wires `kasan-check-preshadow.py`

It already exits 3, for "no `llvm-objdump` found", "objdump failed", and "no such kernel binary". The first is exactly this meaning; the other two are closer to no-verdict. It is not invoked through `run_checker` today, so nothing changes — but if it is wired, split those codes first.

### Cost of getting the code wrong

`run_checker`'s own suite failed five assertions the moment 3 gained a meaning, because group 4 used 3 as its representative "code nothing reads" — having moved there from 2 when 2 gained a meaning. That failure is the suite working. The group now uses 17 and says in a comment that 17 is *reserved*, so the next claimant finds the reason rather than repeating the move.
