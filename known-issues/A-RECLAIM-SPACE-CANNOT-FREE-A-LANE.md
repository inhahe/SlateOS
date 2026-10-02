### A-RECLAIM-SPACE-CANNOT-FREE-A-LANE'S-OWN-TARGET — the rename probe is vetoed by an unattributed handle

**Where:** `scripts/reclaim-space.py` step 3 ("*our own* `target/`"), via
`reclaim_dir`'s rename probe.

**Symptom.** On 2026-08-18, with no build running anywhere and the boot lock
free, the probe reported:

```
Step 2: target/ directories, integration checkout first
  SKIP (in use)  D:\visual studio projects\os-lane-a\target  [Access is denied]
```

Reproducible with a bare `os.rename` outside the script, so it is a property of
the tree and not of the script's logic (WinError 5).

**Narrowed, but not resolved.** Renaming the *children* of `target/` one at a
time isolates it to exactly one:

| child | rename |
|---|---|
| `.rustc_info.json`, `CACHEDIR.TAG`, `debug`, `release`, `x86_64-unknown-none` | OK |
| **`x86_64-pc-windows-gnu`** | **Access is denied** |

> **RETRACTED — the two paragraphs below are wrong.** They are kept because
> they were acted on, and a reader who finds only the correction would not know
> why the wrong tool was reached for. See **"Correction"** at the end of this
> entry. Do not use the conclusion they reach.

~~That `debug` renames while `x86_64-pc-windows-gnu` does not is the informative
part. An open file *inside* a directory blocks renaming every ancestor, so if
the handle were on some file below `x86_64-pc-windows-gnu/debug/` then renaming
`debug` would have failed too — it did not. The handle is therefore on the
`x86_64-pc-windows-gnu` directory node itself, which is the signature of a
process holding it as its **current working directory** rather than having a
file open in it.~~

~~That also explains why it could not be attributed: a cwd is not an image path,
so `Get-Process | Where Path -like` finds nothing (checked — no process runs
from under `target/`), and `D:\utils\handle.exe` is v3.2 and refuses to
enumerate without administrator rights ("Initialization error: Make sure that
you are an administrator"), which this session does not have.~~

The half that survives: `handle.exe` here really is v3.2 and really does refuse
to enumerate without administrator rights ("Initialization error: Make sure
that you are an administrator"), which this session does not have. That is what
left the handle unattributed. The inference about *what kind* of handle it was
does not follow — see below.

**Why it matters.** Step 3 is the step that spends *this lane's* build output
before any other lane's — the script's stated fairness rule. If it is
permanently vetoed, a reclaim run can only finish orphaned staging directories
and clear old `build/` scratch. On the day this was found that was the
difference between freeing ~27 GiB and freeing 1.6 GiB, and the boot test's
`--reclaim-space` retry therefore fails to clear the floor in exactly the
situation it was added for.

**Not a defect in the probe.** The rename test is behaving correctly: it is
refusing to delete a directory the OS says is in use, which is the whole reason
it renames instead of consulting timestamps. The bug, if any, is whatever holds
the cwd.

**Next step when it recurs:** run

```
python scripts/who-holds-dir.py target/x86_64-pc-windows-gnu
```

and record what it names. If it is a long-lived tool (rust-analyzer, a shell,
an editor) the fix is on that side; if it is an orphaned test process, it is
`scripts/run-timeout.py`'s job-object teardown that let one escape and that is
the real bug. Read the exit status, not just the output: **2 means
inconclusive** — no holder found, but some processes could not be inspected, so
the directory has *not* been shown to be free. Only **1** is "really free".

Until it is attributed, do not "fix" it by weakening the probe — a reclaim that
deletes a directory something is using is far worse than one that frees less
than it hoped.

**Discovered:** 2026-08-18, while testing `boot-test.sh --reclaim-space`
end-to-end.

### Correction (2026-08-18, lane A) — the cwd conclusion was drawn from a misread table

**In short:** the reasoning above concluded "a process is sitting in that
directory" from the fact that a *different* directory called `debug` renamed
fine. But the `debug` in that table is `target/debug` — a **sibling** of
`target/x86_64-pc-windows-gnu`, not the `debug` *inside* it. A sibling
renaming successfully says nothing whatsoever about what is held inside its
neighbour, so the conclusion has no support. Worse, the ruled-out possibility
is the *likely* one: a build directory's most probable blocker is a loaded
`.dll` under `x86_64-pc-windows-gnu/debug/deps/`, and that is precisely the
case the entry's own attribution attempt could not have seen.

The table lists **children of `target/`**. Read it again with that in mind:
every row is `target/<child>`. So the successful rename was `target/debug`,
and the argument "an open file below `x86_64-pc-windows-gnu/debug/` would have
blocked `debug` too" compares a directory to one that is not its ancestor. It
would only hold if the table had listed the children of
`target/x86_64-pc-windows-gnu`, which was never probed.

**Four things make Windows refuse a directory rename, and all four raise the
same `Access is denied`:**

| cause | seen by `Get-Process ... Path -like`? |
|---|---|
| **an ordinary open file** inside it | **no** — the commonest case, and not a path property of the process object |
| a running `.exe` whose image is inside it | yes — this is the only one it can see |
| a **loaded `.dll`** inside it | **no** — a DLL is not the process's image path |
| a process whose **cwd** is inside it | **no** — a cwd is not a path property of the process object at all |

The entry checked with `Get-Process | Where Path -like`, saw nothing, and
concluded the cause must be a cwd. In fact that check is blind to *three of the
four* causes, so "it found nothing" was never evidence for any one of them over
the others.

Two candidates outrank the cwd it settled on. The first is an open file, which
is simply the ordinary way a directory becomes un-renamable. The second is a
loaded DLL: `target/x86_64-pc-windows-gnu/debug/deps/` is exactly where cargo
puts proc-macro output and test-harness DLLs — loaded, mapped and
un-renamable. Neither was excluded by anything that was actually run.

There is a second, quieter defect in that check: `Get-Process` reports nothing
for a process it cannot open, and it does so **silently**, so its empty result
conflates "no process matches" with "I could not look at 174 of them".

**What replaces it.** `scripts/who-holds-dir.py` (added 2026-08-18) tests all
four causes, needs no elevation, and refuses to conflate those two answers:

* Open files come from `NtQuerySystemInformation(SystemExtendedHandleInformation)`,
  which lists every handle on the system for any caller. A handle is only a
  number until it is named, and naming it means duplicating it here, which
  needs `PROCESS_DUP_HANDLE` on the owner — granted for same-user processes.
* Images and loaded modules come from `EnumProcessModulesEx(LIST_MODULES_ALL)`.
* The cwd is read out of the target's PEB via `NtQueryInformationProcess` +
  `ReadProcessMemory`, needing only
  `PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ`.
* Processes it cannot open are **counted and printed**, and exit status 2
  ("inconclusive") is distinct from 1 ("really free"). Passing `--no-handles`
  or `--no-modules` also forces 2, because a check that did not run cannot have
  cleared anything.

Every right it needs is granted for same-user processes, which is what makes
`handle.exe`'s demand for administrator rights avoidable rather than merely
inconvenient.

**The open-file case was found by the tool failing to find it.** The first
version tested only the three causes above. Given a two-line Python script
holding one file open in a directory, the rename was refused — and the scan
printed "no holder visible". That is the same false negative as the original
investigation, reproduced in the replacement for it, and it is why the fourth
cause is listed first in the table.

Two things had to be got right to make that scan usable, both found by
measurement rather than reasoning:

* **`GetFileType`, not a granted-access heuristic, is what makes it wedge-proof.**
  `NtQueryObject`'s name query blocks forever on a synchronous pipe with I/O
  outstanding. Filtering on the classic hang-prone access mask let enough
  through to stall a 16-thread pool at 349 of 558 processes. `GetFileType`
  answers from the file object without touching the device, so it cannot block,
  and only `FILE_TYPE_DISK` has a path on a volume — pipes and consoles are not
  merely safe to skip, they are incapable of blocking a rename. Full scan
  after the change: ~15s, no timeout.
* **Kernel object addresses are zeroed for an unprivileged caller.** The
  obvious optimisation — name each kernel object once and share the name among
  every process holding it — is unavailable: measured here, 333 342 handles,
  22 267 of them files, and exactly **one** distinct `Object` value across all
  of them. Each handle must be named on its own.

Verified on this machine: it names the exact file and pid for a deliberately
held directory; it finds cwd holders in this worktree; and it finds image *and*
module holders under `C:\Program Files\Git\usr\bin` — including `msys-2.0.dll`,
a mapped DLL that no image-path filter could ever have surfaced, which is the
blind spot that produced this correction.

**`reclaim-space.py` now attributes its own veto.** `reclaim_dir` calls the
scan on the rename-failure path, so `SKIP (in use)` no longer states a
conclusion and discards the evidence:

```
  SKIP (in use)  target/veto-test  [Access is denied]
    HELD BY pid 56736  D:\python314\python.exe
        handle  D:\visual studio projects\os-lane-a\target\veto-test\inner\held.bin
```

That is the whole reason this entry went unresolved: the one moment the answer
is obtainable is the moment the rename fails, and by the time anyone reads the
log the holder has exited. It runs only on the veto path — the scan costs
seconds and a veto is rare — and its failures are printed, never raised, since
diagnostics attached to an already-decided veto must not be able to turn a
partial reclaim into no reclaim at all.

**The underlying question is still open.** This corrects the reasoning and
supplies the tool; it does not say what was holding the directory that day.
The condition has not recurred since, and nothing was diagnosed
retrospectively — when it next appears, run the script above before forming a
hypothesis.

**Lesson worth carrying:** the original argument was a single deduction from a
single observation, with no control. The observation was even real — `debug`
did rename. What made it wrong was reading the row as a child when it was a
sibling, and there was nothing in the argument's structure to catch that. The
replacement tool then reproduced the same class of error one level up, by
enumerating the causes it had thought of and calling the list complete; what
caught *that* was a deliberate test with a known answer, not more thinking.
Where a cheap discriminating check exists — here, probing the children of the
*failing* directory rather than of its parent — run it instead of reasoning
from the check you happen to have already run.
