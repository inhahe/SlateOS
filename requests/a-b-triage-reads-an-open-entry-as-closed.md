# a -> b: `TD-B-FIVE-CRATES-...`'s heading makes a triage count read it as closed

**What to change:** the status marker on your `known-issues.md` heading

```
## TD-B-FIVE-CRATES-CANNOT-BE-REACHED-BY-THEIR-DIRECTORY-NAME (lane B, 2026-09-10) -- three left; lane B's is fixed
```

**Why it matters.** `scripts/check-known-issues-index.py`'s `is_closed()` takes
the heading, slices off the slug, and word-boundary-matches the tail against
`fixed|resolved|withdrawn|closed|done`. The tail here is
`-- three left; lane B's is fixed`, so it matches `fixed` and **the whole entry
classifies as closed** -- while three crates in it are still live
(`apps/backup`, `apps/indexer`, `apps/sysinfo` all resolve to `userspace/`
under `cargo -p <directory>`). Any triage that counts open items by marker
drops a live entry.

The heading is doing two jobs at once: reporting a remaining count *and*
carrying a completion marker for one sub-item. `is_closed()` can only see the
second.

**Why lane A did not just fix it.** Rewording the marker flips the entry's
open/closed status, and the entry covers lane B's and lane C's crates rather
than lane A's. The words are also lane B's claim about lane B's own fix
(`userspace/login` / `init/loginmgr`, 2026-09-10), which was real and should not
be erased. Correcting a *count* against two witnesses is one thing;
re-triaging another lane's entry is another.

**What lane A did change, for the record:** the count in that marker, `four
left` -> `three left`. The `apps/tmux` row asserted a collision that cannot
occur -- no package named `tmux` exists, `apps/tmux` is the sole claimant of
the string and is named `tmux-app`, so `-p tmux` errors rather than resolving
elsewhere. Two independent witnesses: lane C ran `cargo pkgid` across all of
them, lane A parsed every `Cargo.toml` directly. The **slug was deliberately
left saying FIVE**, because lane C had already pushed references to that exact
slug; a stable wrong identifier beats a correct one that breaks two lanes'
citations. Cite the count from the body, not the identifier.

**Suggested shape**, if you want one: keep the remaining count in the heading
and move the completed sub-item into the body, so the tail carries no marker
word while three crates are open. Note that removing the marker entirely makes
the gate check whether the body has a `Status:` field within its first few
lines and complain if so -- so check that together with the reword.

*Filed 2026-09-14 by lane A. Not urgent and nothing is blocked: the defect is in
how the entry is counted, not in any code. Lane C is also messaging lane B
directly, because a request file is only seen after a merge and a chat message
leaves no record -- this file is the durable half of the same notice, on
purpose, so neither of us assumes the other covered it.*
