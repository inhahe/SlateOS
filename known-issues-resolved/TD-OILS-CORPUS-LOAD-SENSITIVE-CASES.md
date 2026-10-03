### TD-OILS-CORPUS-LOAD-SENSITIVE-CASES. Three corpus cases fail a full sweep and pass in isolation, because their timing margins are smaller than the load a sweep puts on the machine — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/tests/corpus/{dynamic-var-assign,jobs-disown,kill-dispositions}.sh`.

**What:** each of the three failed at least one of the last three full sweeps
and passed every time it was re-run on its own. None of them was an osh bug —
in all three the *reference bash* is the shell that gave the wrong answer, and
in two of them osh's answer was the intended one. The failure reports are
`target/dvscratch/corpus-failures/2026080{4-185306,4-193007}/`.

The common cause is that a sweep is 369 cases run back to back, and every
`sleep` in these cases is a *process spawn* through the MSYS runtime. A nominal
delay is not a wall-clock delay: under sweep load a nominal 0.2 s has been
observed to cost over a second. Any case whose correctness depends on one
interval being shorter than another therefore needs a margin measured against
that, not against the nominal figures — and a case that merely takes a long
time needs a budget with room in it.

**The three, and what each was measured doing:**

- **`dynamic-var-assign`** — not a race at all, a budget. `SECONDS` counts whole
  seconds, so the case's sleeps come to 8 s and it runs ~15 s a shell on an idle
  machine, against the 20 s default. Twice, bash ran past 20 s and the harness
  recorded `bash: <timed out after 20s>` against a complete, correct osh run.
  **Fixed** with `# TIMEOUT: 120` — no behaviour change and no added sweep time,
  since the budget only bites under load.

- **`jobs-disown`** — the one line that turns on timing:
  `sleep 0.05 & sleep 0.7 & sleep 0.4; disown -r; jobs`. `-r` forgets only the
  *running* jobs, so the line needs job 1 finished and job 2 not, at the same
  moment. One direction is safe by construction (job 1 is forked first and
  sleeps less, so it cannot outlast a wait forked after it); the other was a
  0.3 s margin, and the sweep spent it — bash found *both* jobs finished,
  disowned neither, and listed `[2] Done sleep 0.7` that osh correctly omitted.
  **Fixed** by raising the surviving job to `sleep 5`, a ~12× margin. The
  printed row is unchanged: the survivor is disowned and never listed.

- **`kill-dispositions`** — twelve lines of the shape
  `sleep 1 & sleep 0.1; kill -SIG %1; …; sleep 0.1; jobs; wait`, each asserting
  that a job *survives* its signal. The header already claimed the 1 s job
  "sleeps several times as long as the two settling delays put together" — 5×,
  and the sweep went through it: bash listed `[1]+ Done sleep 1` where osh
  listed `Running`. **Fixed** two ways at once. The surviving job now sleeps
  3 s, and the second settling delay is dropped from these lines entirely: it
  exists only so a job that *dies* is reaped before the listing looks, which is
  the killing loop's business, so a surviving job was paying for a delay it had
  no use for. Margin goes from 5× to ~30×. The case now runs ~40 s a shell and
  carries `# TIMEOUT: 180`.

**Standing lesson for the corpus**, alongside the one in
TD-OILS-CORPUS-BG-DEBUG-TRACE-RACE: *a nominal delay is not a wall-clock
delay.* If a case's answer depends on interval A finishing before interval B,
either make the ordering true by construction (fork the shorter one first) or
give it a margin of at least ~10×, and never count a settling delay as part of
the margin when it is itself a process spawn. If a case is simply slow, declare
`# TIMEOUT:` rather than letting it sit just inside the default.
