## TD-B-TWO-PROGRAMS-BOTH-CLAIM-THE-NAME-`sudo` (lane B, 2026-08-21) — **RESOLVED 2026-08-21**

**Resolved** by deleting the `sudo` personality from `userspace/su`, as the
"proper fix" below prescribed. Removed: the `sudo mode` section entire
(`SudoOptions`, `parse_sudo_args`, `print_sudo_help`, `sudo_authorised`,
`sudo_list_permissions`, `run_sudo`, `log_sudo_failure`), the now-unused
`in_admin_group`, the `basename` helper and the `argv[0]` dispatch in `main`,
and the 13 tests that pinned the deleted policy — 296 lines. `su`'s remaining
34 tests pass and clippy is clean. `userspace/sudo` is now the only program in
the tree that answers "may this user run this command as root?", and it answers
it from `/etc/sudoers`.

Two corrections to what this entry originally said:

- **Step 3 below is wrong: there is no `runuser` personality.** The dispatch
  was two-way (`prog == "sudo"` → `run_sudo`, everything else → `run_su`), so
  removing `sudo` left nothing for `argv[0]` to select between and `basename`
  became dead with it. The claim of a third personality was written from the
  `su`/`runuser` upstream pairing rather than from this file.
- **It was a latent security hole, not only a naming collision.** The entry
  files this under "not biting yet" because neither binary is staged. That is
  true of the *conflict*, but understates the shape of it: `su`'s copy never
  opened `/etc/sudoers`, so on any system carrying both, revoking a user's
  rights in that file would have revoked nothing from a `wheel` member who
  invoked the other binary. A configuration file that some enforcement points
  ignore is not a configuration file.

The original entry follows unchanged.

**In short:** the tree contains two complete, independent implementations of
`sudo`, written to two different security policies, and nothing yet decides
which one becomes `/usr/bin/sudo`. Whichever is installed last wins, and the
answer to "may this user run this command as root?" changes with it.

**Where.**

| | `userspace/sudo` | `userspace/su` |
|---|---|---|
| binary | `sudo` | `su`, which runs `run_sudo` when `argv[0]` is `sudo` (`src/main.rs:827`) |
| policy source | `/etc/sudoers`, with a real parser: aliases, `Defaults`, per-command rules, `NOPASSWD`, includes | hardcoded: `sudo_authorised` (`src/main.rs:706`) grants everything to root, to `is_admin`, and to members of `wheel`/`admin` |
| `-l` output | derived from the matching sudoers rules | `(ALL) ALL` or `(NONE)` |
| authenticates | the caller, against `/etc/users.yaml` | the caller, against `/etc/users.yaml` |

Both are real, both are tested (242 and 50 tests respectively), and both now
share the system-wide failed-attempt tally, so the *authentication* halves
agree. It is the **authorization** halves that do not, and that is the half
that matters: a site that writes a restrictive `/etc/sudoers` granting one user
one command gets exactly that from `userspace/sudo`, and gets unrestricted root
for every member of `wheel` from `su`'s `run_sudo` — silently, because the
second never reads the file.

**How it happened.** `su`'s multi-call personality dispatch is modelled on the
real `su`/`runuser` pair, and `sudo` was added to it as a third personality
before `userspace/sudo` existed. Neither crate references the other, so nothing
made the overlap visible; it was found by grepping for password prompts, not by
a failing test.

**Why it is not biting yet.** Neither binary is staged into `rootfs.ext4` — see
the open task "stage coreutils binaries on rootfs.ext4 and run them under ring
3". The conflict is latent, and the right time to resolve it is *before* that
staging work picks one by accident.

**The proper fix.** Delete the `sudo` personality from `userspace/su` and keep
`userspace/sudo` as the only implementation. The reasoning is one-sided:
`userspace/sudo` reads the configuration file `sudo` is defined by, and
`su`'s copy cannot be made to agree with a sudoers file without becoming
`userspace/sudo`. Concretely:

1. Remove `run_sudo`, `parse_sudo_args`, `sudo_authorised`,
   `sudo_list_permissions` and the `prog == "sudo"` arm at `su/src/main.rs:827`,
   along with their tests.
2. Check that nothing invokes `su` under the name `sudo` — at the time of
   writing nothing installs either binary, so this should be vacuous.
3. Leave `su`'s `runuser` personality alone: that one has no competitor, and
   the `su`/`runuser` split is genuine upstream behaviour rather than an
   accident.

Not done in the change that found it because that change was scoped to the
shared failed-attempt tally (`design-decisions.md` §354), and deleting a
program's authorization model is not a rate-limiting change. It should be its
own commit, with its own reasoning, so that it is reviewable as the policy
decision it is.
