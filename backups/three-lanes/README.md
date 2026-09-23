# Three-lane rules: backup taken before the six-lane split (2026-09-22)

**These files are a backup. They are not the rules in force. Do not follow them.**
The live rules are `CLAUDE.md` ("Six Sessions — Find Out Which One You Are,
First"), `roadmap.md` ("Six-Agent Parallel Execution") and
`scripts/which-lane.py`.

On 2026-09-22 the operator moved the project from three parallel Claude
sessions (lanes A–C, one per Claude account) to six (lanes A–F, two per
account), to see whether two sessions per account works. This directory keeps
the three-lane versions of the files that defined the lanes, byte for byte, so
that going back is a matter of copying files rather than reconstructing them
from history.

| Backup | Was | What it defined |
|---|---|---|
| `CLAUDE.three-lane.md` | `CLAUDE.md` | The "Three Sessions" section: the lane table, the worktree table, the account → lane mapping. |
| `roadmap.three-lane.md` | `roadmap.md` | "Three-Agent Parallel Execution" (ownership map, rules of engagement), "Joint tasks", "Current backlog by lane". |
| `which-lane.three-lane.py` | `scripts/which-lane.py` | Lane detection by `CLAUDE_CONFIG_DIR`, and the ownership globs every other script mirrored. |

**Why this is not named `CLAUDE.md`.** Claude Code loads a `CLAUDE.md` it finds
in any subdirectory it reads from. A backup with that name would inject the
superseded three-lane rules into whichever agent next looked in this folder.

## The exact point in history

- **Last three-lane commit on `main`:** `36354a7b3` ("Variant lists are checked by
  type, and by name of variant not count"). Every file in the repository as it
  stood under the three-lane rules is `git show 36354a7b3:<path>`.
- **The six-lane change** is the run of commits on `main` whose subjects start
  with `six lanes:` — list them with
  `git log --oneline --grep='^six lanes:' main`.

## Going back to three lanes

Do it with the six-lane commits as the map, not by copying these files blindly:

1. **`CLAUDE.md` and `scripts/which-lane.py`** can be copied back from here as
   they are — but only together with the other scripts the split changed,
   because several of them now import the ownership table from
   `which-lane.py` (`owner_of`, `OWNERSHIP`, `LANES`), which the old file does not
   provide. `git revert` of the `six lanes:` commits restores all of them at
   once and is the safer route.
2. **`roadmap.md` must NOT be copied back wholesale.** It is the live status
   file; `roadmap.three-lane.md` froze on 2026-09-22 and copying it over would
   silently discard every status change made since. Instead restore only the
   lane sections (everything from "Six-Agent Parallel Execution" down to "If you
   are running as a single agent again") from this backup, then fold the
   lane D/E/F backlog items that exist by then back into lanes B and C.
3. Lanes D–F have their own branches (`lane-d`, `lane-e`, `lane-f`) and
   worktrees (`os-lane-d`, `-e`, `-f`). Merge each into `main` first, so no work
   is stranded; only then remove the worktrees.
4. Entries filed while six lanes ran keep their D/E/F letters (`### [E] …`,
   `D-Q<n>`, `**Lane:** F`, bands §1100–§1399). Leave them: the regexes the gates
   use accept A–F, and history is not renumbered in this repository.

The single-agent version of the roadmap, from before the three-lane split, is
still at `roadmap.single-agent.md` in the repository root.
