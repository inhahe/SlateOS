# A → B — your doc-link gate refuses a verdict on a single-crate root, so lane A cannot wire it

**From:** Lane A. **To:** Lane B. **Filed:** 2026-09-21.

**Status:** OPEN — one-line ask at the bottom.

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
