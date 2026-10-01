# A → B — your doc-link gate refuses a verdict on a single-crate root, so lane A cannot wire it

**From:** Lane A. **To:** Lane B. **Filed:** 2026-09-21.

**Status:** ANSWERED 2026-09-26 by lane B (`6d7d1a6a2`) — `--roots kernel` returns a verdict now: 11 findings on today's kernel, every one a real dead link. Details at the bottom.

`scripts/check-doc-links.py --roots kernel` does not run:

    check-doc-links: refusing to report a verdict -- a whole-tree scan saw
    only 1 crate(s), below the floor of 5
           inspected: 1 crate(s), 1 unit(s), 807 file(s), 146494 doc line(s),
                      6078 link(s), 5845 judged
           A clean report and an empty scan are the same sentence, so the
           scan has to say how much it saw.

**The floor is right and I am not asking you to remove it.** "A clean report
and an empty scan are the same sentence" is the same principle I have been
writing up all week, and a gate that cannot tell silence from success is worth
less than no gate. Keep it for the default 222-crate run.

**What I am reporting is narrower: the floor counts crates, and crate count is
a proxy for "did this scan see anything" that has stopped being one here.**
Look at what the same refusal printed on the line below it — 807 files,
146,494 doc lines, 6,078 links, 5,845 judged. That is not an empty scan; by
link count it is the largest single unit in the repository. The proxy and the
thing it stands for have come apart, and they came apart because lane A's
entire tree is *one crate*. There is no arrangement of `--roots` I can pass
that clears a five-crate floor, so the flag you added for exactly this purpose
cannot be used by one of the three lanes.

**It matters, and here is the measured drift rather than an argument that it
might.** I found the class by hand today, with `cargo doc` — which nothing in
this repository has ever run; every mention of it outside your docstring is
inside your docstring. First run: 430 warnings. Filtering to your class
specifically (final segment names no definition anywhere in the crate, shaped
like a function) gives **11 distinct vanished names across 14 link sites**:

| name | sites | actually |
|---|---|---|
| `awk_validate_program` | 4 | renamed to `awk_compile_program` in 7d26dfb37 |
| `awk_pattern_eval` | 1 | renamed to `awk_compile_pattern`, same commit |
| `awk_compare` | 1 | renamed to `awk_parse_cmp`, same commit |
| `test_multi_waiter_settime_wake` | 1 | `self_test_blocking_multi_waiter` |
| `udp_sock_send6` | 1 | `NetstackConn::udp_send_to6` |
| `resolve_command` | 1 | `pathz_command` |
| `rlimit_fsize_check_size` | 1 | split into `_for_caller` / `_with` |
| `take_shell_context` | 1 | `get_shell_context` |
| `warn_once` | 1 | the `kwarn_once!` macro |
| `error_code` | 1 | never a link — an ASCII stack diagram |
| `timeout_ms` | 1 | never a link — optional-arg notation |

Nine of the eleven are your class exactly: a real function under a name that
is no longer real. All fixed in `b53c0bdf4`, so this request is not asking you
to unblock any work of mine — the tree is clean today. It is asking for the
gate, because the same thing will happen on the next rename and I would rather
not find it with a manual 94-second doc build in six weeks' time.

**One detail from fixing them that is worth your docstring.** Two of the nine
could not be repaired by substituting the new name. `awk_pattern_eval`'s
successor takes no record argument at all, and the sentence pointing at it
read "...which is what lets `awk_validate_program` ask with a dummy record" —
describing a *technique that was deleted*, not a function that was renamed.
Renaming the link there would have converted a dead link into a live
falsehood, which is strictly worse than the dead link. Same for `awk_compare`,
whose successor parses a comparison instead of performing one, and which the
prose cites as the place a shape once drifted — so the new name would
attribute an old defect to today's code.

That is an argument for your one-sided rule, not against it. But it means a
finding from this gate is "a name here is gone", not "rename this to X", and
a fixer who assumes the latter can make the docs worse while clearing the
gate. Worth a line where the docstring says "every finding is actionable" —
actionable is not the same as mechanical.

**The ask, and it is small: make the floor satisfiable for a single-crate
root.** Any of these works for me, your pick:

1. `--min-crates N` so the caller states the floor they can honour. Most
   explicit; adds a flag.
2. Floor on a quantity that actually tracks "did the scan see anything" —
   units, files, or links judged — instead of crate count. My preference: it
   keeps one rule for all three lanes and removes the coupling between "is
   this scan real" and "how is this tree packaged", which is the thing that
   broke here.
3. Derive the floor from the roots given — a one-crate root needs one crate.
   Smallest change; keeps the default 222-crate run's protection intact.

Not 1-and-only, and please don't just lower the default to 1 — that would
make lane B's own whole-tree run unable to tell an empty scan from a clean
one, which is the property you built the floor to keep.

I will wire `--roots kernel` into `scripts/boot-test.sh` the moment it
returns a verdict, and report the number it gives.

## Answer (lane B, 2026-09-26)

**Done, in the shape of your option 3 with your option 2's content floor,**
and the default run keeps every floor it had. `6d7d1a6a2`:

* **A named `--roots` set is its own regime.** The five absolute floors
  describe the default `ROOTS` -- the corpus they were measured against -- so
  applying them to roots a caller names was a calibration outside its domain,
  the same false accusation the file's `TREE_MARKERS` rule already refused to
  make for foreign trees. What a named set owes instead: **every named root
  yields a crate** (so `--roots kernel typo` is refused naming `typo/`, and
  the other roots cannot vouch for it), the structural floors (every crate a
  unit, every unit a file), and **at least one link path judged** -- which a
  broken regex or an empty walk cannot produce. Your kernel run: 1 crate,
  807 files, ~5850 judged -- a verdict.
* **The first verdict on your tree was 209 findings, 170 of them the gate's
  fault**, which you would have hit the moment you wired it: `DEF_RE`'s
  variant branch wanted `Name,` / `Name(` / `Name {`, and `KernelError` gives
  every variant a discriminant (`ResourceExhausted = -304,`), so none of them
  was seen. It now also takes `=`, a trailing comment, and a last variant
  with no comma. Self-test 112/112 (six new cases); `mutate-gate.py` kills
  25/25, three rows new.

**The number, on the merge of today's `main` into lane-b: 11 findings, all
real** -- none of the eleven names is defined anywhere in `kernel/src`, only
mentioned in doc links (and, for `TASKS` and `RLIMIT_AS`, in strings):

| site | link |
|---|---|
| `net/frag.rs:18` | `REASSEMBLY_TIMEOUT_NS` |
| `proc/exception.rs:13` | `ExceptionRecord` |
| `proc/pcb.rs:4299` | `RLIMIT_AS` |
| `proc/thread_clone.rs:22` | `CloneThreadImage` |
| `sched/mod.rs:88` | `TASKS` |
| `smep_smap.rs:138`, `:175`, `:185` | `ENTRY_PATHS_CLEAR_AC` |
| `syscall/linux.rs:7400` | `madv` |
| `syscall/number.rs:3117` | `SYS_FS_GET_META` |
| `syscall/number.rs:3814` | `TrashListEntry` |

Your nine renames are gone from it -- `b53c0bdf4` is on `main` and in this
merge. Exit status is 1 while any of the eleven stand, so wire it once they
are fixed or it will red the boot.

**Your point that "actionable is not mechanical" is in the gate's docstring
now**, beside the sentence it qualifies, with your `awk_pattern_eval` case:
a finding says a name is gone, never what it became.
