## 1074. A program's remembered settings include the processors it may use, a cap on its processor use, and a profile while it runs

**Date:** 2026-10-11
**Lane:** B
**Decided by:** Operator (answering B-Q26: option A, which Claude
recommended), in `open-questions/answers.3.txt` in the integration tree.

**In short:** the operator asked whether SlateOS should have controls like
Process Lasso's (a Windows tool that remembers settings for each program).
Most of them were already planned -- a priority remembered for each program,
processor time, memory and disk speed kept back for the desktop, workload
profiles, disk priorities and per-program limits. Three were not, and are now:
a program can be given the processors it may use, a cap on how much processor
time it takes, and a profile SlateOS switches to while it runs. They are kept,
shown and applied with the remembered priority.

**The operator's answer, verbatim:** `B-Q26: A`.

### The three, and where each lives

| control | what the user sets | applied by |
|---|---|---|
| **Processors** ("CPU affinity") | the processors a program may run on; on a chip with fast and power-saving cores, "the fast ones" or "the power-saving ones" | the processor mask the kernel already keeps per process (`SYS_SCHED_SET_AFFINITY`), set when the program starts |
| **A processor cap** | "never more than 25%", or "no more than 2 cores" | the launch-time limits `design.txt` calls for ("set resource limits at process launch, let the kernel enforce them") |
| **A profile while it runs** | a workload profile -- Gaming, say -- switched to when the program starts and back when it ends | the workload profiles of `roadmap-detailed.md` §1.3 |

All three join the item "A default priority per program, remembered"
(`roadmap-detailed.md` §1.3): the same places to set them (Settings, and the
task manager beside the running program), the same precedence from launch
parameter down to the program's own declared default, and the same rule that
lowering is free while raising needs the grant -- a cap or a narrower processor
set is always allowed; a profile that raises priority for everything else
needs the right a priority raise needs.

### Left out on purpose

Process Lasso also blocks programs by rule (the permission system's job),
restarts a program that exits (the service manager's, for services), and
trims programs' memory in the background, a Windows workaround that usually
makes things slower. None of those is added.

### What was weighed

- **B, the first two without the profile switch**, leaves profiles to be
  changed by hand; the switch costs only the remembered setting, since the
  profiles exist.
- **C, none of them**, leaves `taskset` to be run every time a program starts.
