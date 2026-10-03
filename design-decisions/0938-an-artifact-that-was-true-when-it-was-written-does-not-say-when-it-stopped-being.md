## 938. An artifact that was true when it was written does not say when it stopped being true

**Date:** 2026-09-14 · **Decided by:** Claude (autonomous) · **Lane:** A

Co-named with lane C, who supplied the framing: *true when measured, false when
read*. Distinct from 937, which is about a check that cannot see a defect. This
is about a record that **could** see it, did, and has since gone out of date
without changing its appearance.

**In short:** files and status codes describe the moment they were produced.
Nothing in them says how long ago that was, so a stale one reads exactly like a
current one. Four separate wrong conclusions on 2026-09-14 came from this, and
in every case the reasoning applied to the artifact was correct -- only its age
was wrong.

| artifact | the rule applied to it | why the rule did not help |
|---|---|---|
| a kept per-gate log | `run_checker` deletes its log on success, so a surviving log means a finding | true, and the finding was 39 minutes old and already fixed |
| `build/serial-test.txt` | grep the serial log for the self-test's line | the file was 40 hours old; the run being judged never reached QEMU |
| `git fetch` then reading the tree | the ref is current | the *ref* was current; **HEAD** was 265 commits behind |
| a task notification saying `exit code 0` | the command reported success | it describes the **wrapper**; the child had exited `FAIL (exit 1)` |

**Why it is worse than being wrong.** A false rule can be corrected once and
stays corrected. Here the rule is sound and stays sound, so re-deriving it
confirms the mistake. There is no moment at which re-checking the *reasoning*
helps; only re-checking the *provenance* does, and provenance is a question you
have to already be asking. The artifact volunteers nothing: a 40-hour-old log
and a fresh one are the same bytes in the same place under the same name.

**The documentation case, and it is the most common one.** Prose about a past
state is indistinguishable, to a search, from a description of the present.
Three instances on 2026-09-14, and in all three the text was **accurate when
written**:

| text | read as | actually |
|---|---|---|
| `main.rs`: "appregistry and startmenu **were** reachable only from `kshell`" | a current fact, and it priced a whole open question wrongly | a fixed condition, past tense |
| five `scripts/*.py` matching `os-lane-[abc]` | four gates scanning a foreign worktree | four were a comment, a paragraph about a past bug, and a usage example |
| `//!` dependency arrows: five modules say "-> appregistry" | a hub with five dependents | documentation; the code has one caller |

This is worse than a stale artifact, because a document does not even carry an
mtime a reader would think to check. A log at least *has* a timestamp that a
careful reader might consult; a sentence in a module doc has nothing but tense,
and `grep` does not read tense.

**The remedy is not more careful reading.** It is to separate the two
mechanically wherever the answer matters: filter comment lines, parse rather
than match, and count call sites rather than name occurrences. Lane A read five
grep hits as four gates and later read five doc arrows as five dependents, on
the same day, having written the first lesson down in between.

**The rule adopted.** Before reading an artifact as evidence about a run, assert
it belongs to that run. In practice:

- **mtime before contents.** Print the age next to the path, every time, in the
  same command that reads it. It costs one line and it is the only defence that
  fired reliably.
- **Prefer an artifact the work must produce over one it merely writes.** Lane
  B's form of this: a log line saying a thing happened is testimony; a file that
  only exists if it happened is evidence. Where they disagree the log is the one
  that can be forged, and where they agree the log was not needed.
- **A wrapper's exit status describes the wrapper.** Write the real status into
  the log yourself (`BOOT_REAL_EXIT=$?`) and read that, never the notification.

**What this does not get.** No gate can carry this, and that is not an
oversight: a gate reads what is in front of it too, and would need the same
provenance question asked about its own inputs. This is a habit with a
mechanical trigger -- reading a file to decide something -- rather than a check
that can be wired into the boot.
