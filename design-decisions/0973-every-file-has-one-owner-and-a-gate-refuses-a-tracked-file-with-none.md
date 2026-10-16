## 973. Every file has one owner, and a gate refuses a tracked file with none

**Date:** 2026-09-27 · **Decided by:** Operator (the operator left the choice of lanes to Claude; the assignments below are Claude's and Claude's to revisit) · **Lane:** A

Answering A-Q11. Relayed by lane F from the operator's answers in lane F's
session, 2026-09-27; verbatim:

> Assign it to whatever lane you want, if another lane wants to change it too
> they can request the original lane to do it. And pick a lane for each of the
> remaining files without lanes, and make a rule somewhere that we don't leave
> files with ambiguous ownership.

**In short:** the tool that says which agent may edit which file left about a
thousand files to nobody:
- the scripts;
- the request notes;
- a few dozen small shared libraries;
- the files at the top of the tree.

Two agents once edited one of them, the push hook, on the same night. The
operator decided that every file gets exactly one owning lane, and that another
lane wanting a change asks the owner. A rule stops files from being left
without an owner again. Which lane gets which file was left to Claude.

**What it obliges.**
1. **`scripts/hooks/pre-push` is lane A's**, and so are `scripts/run-checker.sh`
   and `scripts/which-lane.py`. The hook and `scripts/boot-test.sh` run the same
   gates through one `run_checker`, and A-Q13's answer (§974) moves gates from
   the boot into the hook, which is lane A's work. One owner for the gate
   machinery keeps its two call sites consistent. Lane B's claim to the hook
   dates from the three-lane table; lane B's scope has been userland since
   2026-09-22.
2. **The rest of `scripts/`** follows three rules:
   - a script belongs to the lane whose code it judges or serves;
   - a test suite goes with the script it tests;
   - a gate that judges every lane's files belongs to lane A, as part of the
     gate machinery. Examples are line endings, gates that can refuse, and
     gates that are wired.

   The result is written into `which-lane.py`'s table, script by script where
   the directory does not decide it.
3. **`requests/`: a request belongs to its sender**, the first letter of its
   name (`a-bc-...` is lane A's). The addressee's status stamp stays allowed,
   as roadmap.md rule 3's exception already says.
4. **A root leaf crate goes to the lane whose code depends on it most**,
   counted from the manifests rather than guessed. This is how `randrange`
   went to lane E.
5. **The files at the top of the tree.**
   - The shared documents stay shared, each under the per-lane rule it already
     has: `roadmap.md`, `known-issues.md`, `known-issues-resolved.md`,
     `design-decisions.md`, `open-questions.md`, `deferred-questions.md` and
     `todo.txt`. A rule that says who writes which part is an owner, not an
     ambiguity.
   - The operator's own files are the operator's: `CLAUDE.md`, the design
     texts, `backups/` and the personal notes.
   - Everything else gets a lane.
6. **The rule.** `which-lane.py` answers every tracked path with one of:
   - a lane;
   - `operator`;
   - a named shared-document rule.

   It never answers "nobody". A gate in both the boot test and the push hook
   refuses a tracked file with no owner, so a new file or directory at the top
   of the tree gets one in the commit that creates it. `roadmap.md`'s
   six-agent section states the rule.
