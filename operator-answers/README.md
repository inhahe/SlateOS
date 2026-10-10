# Operator answers

**In short:** the operator answers the questions in `open-questions/` by
writing a plain text file in the integration tree
(`E:/visual studio projects/os`): `open-questions-answers.txt`, then
`open-questions-answers.2.txt` at its root, then `open-questions/answers.txt`
beside the questions, one paragraph per question, keyed by its id or its
title. That file is untracked -- on no branch, in no lane's worktree -- so
this directory is where it becomes part of the project, and where anyone can
see which answers went where.

| File | Holds |
|---|---|
| `<date>-<name>` | each answers file, **copied in verbatim** the day a lane finds it. The operator's words, unedited; nobody changes a copy afterwards. A later version of the same file (the operator adds answers to it) is copied again under its new date. |
| `LEDGER.md` | for each answers file: its content hash, and a row per answer -- where it was recorded, and whether anything from it is still waiting on the operator. |

## How a new answers file is noticed

`python scripts/check-lane-signals.py` -- which every lane runs at the start
of a task and on every wakeup -- hashes each `open-questions-answers*.txt` at
the root of the integration tree, any suffix, and each `*answers*.txt` in its
`open-questions/`, and reports a file whose hash is
in `LEDGER.md` on no lane's branch: a new file, or a new version of an old one.
It names the question ids in it, the reader's own lane's first. What to do
then is in `CLAUDE.md`, under "When the operator answers".

The hash is SHA-256 of the file with CRLF read as LF and the blank lines at
either end ignored (`check-lane-signals.py`, `answers_key`), so a copy in this
directory -- stored with LF -- has the same hash as the original.

## Why this exists

On 2026-09-27 the operator answered 36 questions in
`open-questions-answers.2.txt`. Every one was recorded that day -- a
`Decided by: Operator` entry in `design-decisions/`, a line in
`open-questions-resolved/` -- relayed to each lane by lane F's session, which
was the one given the file. But nothing told the operator so. The integration
tree, which the operator reads and every session is launched in, had not been
updated since 2026-09-26: by 2026-10-09 it was 3,389 commits behind `main`. It
still showed the old single `open-questions.md` with every answered question in
it, and not one of the 33 questions that reached `main` after it and were
waiting on the operator. Two answers that asked for a change to `CLAUDE.md`
(B-Q19, C-Q20) were held for a confirmation the operator could not see being
asked for.

`check-lane-signals.py` now keeps the integration tree fast-forwarded to
`main`, and the operator can read here what became of every answer.
