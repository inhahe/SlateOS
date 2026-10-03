## §89 — `jobs -x` substitutes a job's *pid*, where bash substitutes its process group

**Date:** 2026-07-27
**Decided by:** Claude (autonomous)

**Decision.** osh's `jobs -x cmd …` replaces each whole-word job spec among the
operands with the job's **process id**. Reference bash replaces it with the
job's **process group id** (`job->pgrp`).

**Why they differ.** bash's substitution is a pgid because bash puts each job in
its own process group when job control is on — so the pgid *is* the job, and
`jobs -x kill -9 %1` reaches every process in the pipeline. osh has no terminal
job control and no process groups at all (TD-OILS13); a job is one process (or
one in-process thread), so there is no group to name.

* **Alternative A — answer with the pid (chosen).** `%1` names something that
  really is the job, so `jobs -x kill %1` does what it says. Cost: on a
  multi-stage background pipeline it names the stage osh records as the job,
  not the whole pipeline — which is the same limitation osh's `kill %1` already
  has, so `-x` adds no new gap.
* **Alternative B — mirror bash literally by answering with the shell's own
  process id** (bash without job control puts every child in the shell's group,
  so that is the number it prints). This would match the reference shell's
  output byte-for-byte under the differential harness, at the price of making
  the feature useless and actively dangerous: `jobs -x kill %1` would signal the
  shell itself.

Chosen A. Fidelity to what the operand *means* beats fidelity to a number that
reference bash only produces because it is not doing job control in the
harness. The differential corpus (`tests/corpus/jobs-execute.sh`) therefore
covers everything except the substituted value — which words are replaced at
all, the diagnostics, the option rules and the exit status — and the value
itself is pinned by a unit test instead.
