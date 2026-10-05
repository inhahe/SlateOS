## B-A-DIFF-HARNESS-WHOSE-FIXTURE-DIES-REPORTS-TOTAL-AGREEMENT (lane B, 2026-09-12)

**In short:** `scripts/ps-diff.sh` runs the same command through our binary and
through procps and compares. On 2026-09-12 an edit of mine left the shared
inner script malformed, so **neither** side ever reached `exec ps`. Both
produced nothing. Nothing equals nothing, so the harness reported

```text
59 passed, 0 differed, 0 differ on purpose, 6 NO LONGER differ
```

-- a perfect score, from a run that measured nothing at all.

**What caught it was the impossible XPASS.** Two of the six cases that suddenly
agreed were `-X` and bare `-u`, which this build **does not implement**. They
cannot legitimately match procps. A result that is too good is data.

**So the declared divergences are the harness's liveness proof**, and that is
worth stating because it was not designed in. A dead fixture makes everything
agree; the only cases that can contradict an all-agree result are the ones
expected to *differ*. `xfail_case` was introduced to stop a known difference
being re-investigated -- it turns out to be the thing that distinguishes "both
correct" from "both dead", and a harness whose xfail list ever empties loses
that check with it.

**`broken` did not fire**, and the reason is instructive: `compare()` refuses
exit codes 127 and 125, because those are "command not found" and "cannot
execute". A shell whose script has a syntax error exits **2**, having run
nothing -- a code no rule was watching. The same shape as every other case this
week where a check answered a narrower question than the one being asked.

**How the malformation happened**, since it is the ordinary way and not an
exotic one. A first edit was rejected (an apostrophe inside a single-quoted
`sh -c` string ended the quote). The repair patched from the `if` to the first
`     fi\n` -- which matched a `fi` belonging to the *rejected* edit still
sitting in the file, leaving its tail behind. Two `done`/`fi` pairs, one `if`.
The syntax check `bash -n` passed the OUTER script, because the damage was
inside a quoted string that `bash -n` does not parse.

### The fixture bug underneath, which was real

`run_shared` spawns `sleep 5 &` and immediately `exec ps`. Between `fork` and
`exec` the child is still **runnable** and still carrying the **shell's**
memory map -- so a `ps` arriving in that window reports state `R` and a virtual
size that is not the sleep's. One cause, two symptoms, and the reason
`ps -l -p 2` flapped between runs rather than failing honestly: any change to
either side's timing moved the race. Adding row buffering for `--sort` moved it.

It now polls `/proc/2/stat` until the state is `S`, using the `read` builtin so
it forks **nothing** -- a forked `awk` would take PID 3 and could itself be
caught in the listing it is preparing.
