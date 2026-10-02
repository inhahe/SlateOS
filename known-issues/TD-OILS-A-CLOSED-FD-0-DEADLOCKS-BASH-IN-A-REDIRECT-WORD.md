## `TD-OILS-A-CLOSED-FD-0-DEADLOCKS-BASH-IN-A-REDIRECT-WORD` (lane B, 2026-08-26) — **open**, bash quirk, no osh change intended

**In short:** if you close a command's standard input and then, in the *same*
redirect list, use a `$(…)` substitution that tries to read standard input, bash
hangs forever. osh does not — it reports `Bad file descriptor` and carries on.
osh's answer is the better one, and this entry exists to record why the corpus
cannot test the case rather than to propose a change.

**Reproducer** (GNU bash 5.2.21, x86_64-pc-linux-gnu; hangs until killed):

```sh
bash --norc -c '( true <&- 3> $(read -r a; echo "rc=$?" >&2; echo /dev/null) )' </dev/null
```

**Why it hangs.** `<&-` closes descriptor 0, so 0 becomes the lowest free
number. bash then calls `pipe(2)` to collect the command substitution's stdout,
and `pipe(2)` hands out the lowest free descriptors — so the *read* end of that
pipe is handed fd 0. The substitution's child dups the write end onto fd 1 but
never closes the stray read end, so inside the substitution fd 0 is the read end
of its own stdout pipe. The `read` waits for data only that same process could
send, and because the write end is open EOF never arrives either.

Confirmed directly rather than inferred: substituting `ls -l /proc/self/fd/0`
for the `read` prints `lr-x------ … 0 -> pipe:[24709]` under bash — read-only,
so it is the read end. Under osh the same probe says `/proc/self/fd/0: No such
file or directory`, because osh really did leave fd 0 closed.

**Why osh is not going to be changed to match.** Matching would mean reproducing
a deadlock. §305's stopping criterion excludes exactly this — an artifact of
bash's C-level descriptor allocation, reachable only by deliberately closing
stdin and then reading it, whose only "observable" is that one shell hangs. osh
already gives the answer bash gives whenever it has a spare descriptor.

**What it cost, and what was done.** The probe sat in
`userspace/oils/tests/corpus/a-redirect-list-is-performed-left-to-right-and-each-step-is-already-in-effect.sh`
and hung the *reference* side, so the harness reported the case as `T … bash did
not finish within 20s, twice` and the case produced no verdict at all — taking
the roughly fifty other assertions in the same file down with it, including the
whole `<>`, shared-offset and `2>&1`-ordering sections. It was previously
recorded as "re-run with `--timeout-scale 5`", which could never have worked: a
deadlock does not finish at any multiple of the budget, it just fails more
slowly. The probe is now removed and the reasoning left in its place, which
makes the case measurable again.
