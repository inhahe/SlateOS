### TD-ALL-THE-OS-INTEGRATION-TREE-HAD-CORE-BARE-SET-TO-TRUE — 2026-09-04 — FIXED (watch for recurrence)

**In short.** `D:\visual studio projects\os` — the integration checkout every
lane merges through — had `core.bare = true` in its `.git/config`, while having
a complete working tree on disk. Git then refused every command that needs one:

    $ git status
    fatal: this operation must be run in a work tree
    $ git merge lane-c
    fatal: this operation must be run in a work tree

Which blocks **all three lanes**, because that directory is where lane branches
are merged up to `main`.

**Fixed by** `git config core.bare false` in that directory. Nothing else was
wrong: history was intact (the previous lane-C merge was still on `origin/main`)
and the working tree was fully present, which is itself the proof that the flag
did not describe reality.

**How it got set is not established, and that is the reason for this entry.**
What was ruled out:

- Not a history rewrite. `.git/filter-repo/` exists but is dated 2026-07-11 and
  its `already_ran` marker is from then; nothing in the reflog is recent.
- Not latent since July. Six lane-C merges went through this tree earlier the
  same day, the last at `162953e8f`; the flag appeared between that merge and
  the next attempt maybe twenty minutes later.
- Not this session's doing: nothing lane C ran in that directory was more than
  `fetch`, `merge`, `push`, and none of those writes `core.bare`.

That leaves something else touching the shared tree — another lane, a tool, or a
script. **If you see this again, look at what else ran against `os` in the
window before it**, and consider whether the integration tree should be a
worktree of its own like the three lane trees are, rather than the original
clone with a `filter-repo` history in it.

**Symptom to recognise:** every git command in `os` failing with "must be run in
a work tree", while the same commands work fine in `os-lane-a/b/c`. The check is
`git -C "…/os" config --get core.bare`; the answer should be `false` or absent.
