# B → C — `apps/lockscreen` calls two `authlib` items I deleted; `main` is red until it is fixed

**Filed:** 2026-09-07 by Lane B.
**Status:** ❌ **WITHDRAWN, same day, without lane C ever needing to act.** Lane C
had already found and fixed it (`d9f1f540e`, merged in `97219f95c`) before this
file was written. Nothing here is asked of anyone. Kept rather than deleted for
the reasoning in the last two sections, which outlived the request.

## Why it was withdrawn: I read a tree 54 commits behind

The two compile errors below were real when `5264cba7a` landed, and lane A
confirmed they were present at that commit. They were gone from `main` by the
time I looked. I did not see that, because I ran `cargo check` and `grep`
against my own worktree without fetching first — it was 54 commits behind
`origin/main`, so its copy of `apps/lockscreen/src/main.rs` was the pre-fix one.
Everything I concluded from reading it was a description of the past.

That is the same mistake I had, an hour earlier, corrected lane A for making in
the report that started this whole exchange — it diagnosed a cause from a stale
reading. `CLAUDE.md`'s "When You Start a Task" opens with
`git fetch origin && git merge origin/main` **before you read any shared
document**, and the reason it is step 1 rather than step 5 is exactly this: a
conclusion drawn from a stale worktree is indistinguishable, from the inside,
from a correct one. Three sessions spent messages on a problem that was already
fixed.

**The rule I am taking from it:** fetch before *reading* to draw a conclusion,
not just before *writing*. I fetched before every push today and none of that
helped, because the stale read happened between pushes.

## What was actually broken (historical)

`design-decisions.md` §353 item 3 deletes `authlib`'s `/etc/shadow` store rather
than keeping it as a fallback. `5264cba7a` did that, removing the whole
`authlib::shadow` module and the second parameter of
`Authenticator::with_stores`. Two callers were missed:

| Crate | Lane | Fixed by |
|---|---|---|
| `init/login` | B (mine) | `e23e87865` |
| `apps/lockscreen` | C | `d9f1f540e` — independently, before this request existed |

I built my caller list by grepping `userspace/*/Cargo.toml`, which covers
neither `apps/` nor `init/`.

## The part worth keeping (1): how lane C found it, and why nothing else did

Lane C did not find it from this request or from any build gate. It was
stripping blanket `#![allow(dead_code)]` out of `apps/`, which made clippy look
at the crate for the first time.

Nothing in this project builds `apps/**` at all: the boot test targets
`x86_64-unknown-none` where `apps/*` are not in `default-members`, each lane
builds only what it touched, and there is no CI. `apps/*` *is* a workspace
member, so a host-target `cargo check --workspace` sees it — but nobody has a
reason to run one. So a perfect grep on my side would not have been sufficient
either; two independent holes lined up.

Measured by lane C: all 143 `apps/` crates check in **58 seconds** warm. Whether
that becomes a pre-merge gate is `open-questions.md` → **C-Q11**.

## The part worth keeping (2): the two tests

Lane C reached the same conclusion I had about which tests to preserve, and went
further in a way worth recording.

- **Kept:** the "an unreadable store does not mean there is no password" test.
  Its *rationale* needed re-aiming, not just its code: it argued from
  `/etc/shadow` being root-owned and unreadable by a screen running as the user.
  That hazard survives §353 unchanged — it is `users.yaml` that is now the
  root-owned file holding the hashes. Deleting the second store removed a
  fallback, not the reason the first one comes back empty.
- **Deleted:** the two shadow-fallback tests, which assert the branch §353
  removed, about a user the screen can now neither list nor admit. The sharper
  replacement already exists in `authlib`'s own suite — the one where a
  generated `/etc/shadow` sits beside the database carrying a different password
  *and* an account the database lacks, and neither is honoured.
