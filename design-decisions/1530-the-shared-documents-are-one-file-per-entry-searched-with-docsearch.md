## 1530. The shared documents are one file per entry, searched with `docsearch.py`, and their open/done rules are a gate

**Date:** 2026-10-02 (cutover published as `7046d5e04`)
**Lane:** A -- recorded for the operator by the operator's docs session (os-81); the docs tooling is lane A's since §973
**Decided by:** Operator (Claude proposed the five steps; the operator approved them as proposed)

**In short:** The documents every lane reads to learn what is broken, what was decided and what is left -- `known-issues.md`, `design-decisions.md`, the question queues and `roadmap.md` -- had outgrown reading: the issue file alone was 180,000 lines, and agents searched it with grep, which matches wording rather than entries and cannot tell a fixed issue from an open one. Now each issue, decision and question is its own file; closed issues and finished roadmap items live apart from open ones; a search command finds entries by their words, and by their meaning once the local model has indexed them; and a checker run by the boot test and the push hook keeps the open/done split honest.

### The problem

- **Size.** `known-issues.md` was 180,043 lines (10.6 MB), `design-decisions.md` 90,413 lines (5.3 MB), `roadmap.md` 9,778 lines.
- **Cost, measured** (`agent-knowledge/scripts/doc_usage.py` over the session transcripts): from June to October agents spent about 44,000 tool calls grepping and reading these files and pulled about 17 M tokens of them into context.
- **grep finds lines, not entries, and not status.** Status was written several ways (`— FIXED`, `-- FIXED`, a bold opening line, lower-case `fixed`); `ki_split.py` alone counted 185 `-- FIXED` entries as open.
- **Single files needed gates of their own:** entries torn in half by code fences, questions filed below `# Resolved`, two lanes inserting at one line offset, archived entries resurrected by merges.

### The decision -- five steps

1. **One file per entry.** `known-issues/<ID>.md` (open) and `known-issues-resolved/<ID>.md` (closed); `design-decisions/NNNN-<slug>.md`; `open-questions/<ID>.md`, with answered questions as one-line records in `open-questions-resolved/lane-<x>.md`; `deferred-questions/`. The old files are signposts, and a citation's id or number is the file name. Finished roadmap blocks live in `roadmap-done.md`. The conversion (`docs-migrate.py`) is deterministic and verified itself: every line accounted for, every entry byte-identical and filed by its status.
2. **Search.** `scripts/docsearch.py`: SQLite FTS5 (BM25, Porter stemming) over every entry kind -- issues, decisions, questions, roadmap items, `todo.txt` paragraphs, requests, spec passages -- with `--kind`, `--lane`, `--status` and `--since` filters, ids and section numbers looked up exactly, and local embeddings (Qwen3-Embedding) fused in once at least 90% of the searched entries have one.
3. **Summary cards.** A local model (Qwen3.8-27B) writes each entry a one-line summary, its status, tags and the questions it answers, into a cache in the git common directory. Cards are searched as an extra field and shown labelled as summaries; they are never written into a document. `agent-knowledge` refreshes them nightly at 04:30.
4. **The rules are a gate.** `check-docs.py` (S1-S4, I1-I3, D1-D3, Q1-Q2, F1, T1, R1-R2) replaces `check-known-issues-index.py`, `check-open-questions.py` and `check-design-decisions-bands.py`, and runs in the boot test and in pre-push gates 13, 29 and 36. Only the running lane's own entries can fail its build (§903). What the cutover inherited is grandfathered in `scripts/docs-baseline.json`.
5. **Carry-forward.** Each lane carried its in-flight edits across with `docs-carry-forward.py` (a three-way merge per entry). A dry run on all six lane tips before publishing lost no entry.

### Rejected

- **A self-writing memory layer** (Hindsight and similar): beliefs written by a model, read by every agent, reviewed by no one, invisible to git diff and revert, and blind to branches -- a second source of truth competing with these documents. The cards are summaries of the documents, labelled as such, never instructions.
- **Whoosh or another search library:** FTS5 ships with Python, needs no dependency, and its index lives in the git directory.
- **Keeping the single files and adding an index:** keeps the size and the merge failures, and an index over a 180,000-line file still returns lines.
- **Directories by subject:** entries straddle subjects; one directory per kind, with filters, covers it without a taxonomy.
- **One file per roadmap item:** items nest and are reordered constantly; finished blocks move to `roadmap-done.md` instead.

### What it costs

- Every lane merged the cutover once (lane A with one roadmap conflict, resolved to main's side).
- Inherited debt is grandfathered, not fixed: 2,088 issues without a `**Status:**` line and 108 lower-case markers. The baseline should only shrink.
- R2: 290 roadmap items marked `[x]` still hold 2,257 open sub-items, 287 of them lane B's old batch items in §2.7. They are reported as a warning, not fixed.
- Search by meaning and the cards depend on the local models in `D:\ai`; without them search is keyword-only and says so.
