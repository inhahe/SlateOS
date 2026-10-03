## ~~TD-B-FIVE-PROGRAMS-STILL-TAKE-THE-CALLERS-IDENTITY-FROM-THE-ENVIRONMENT~~ (lane B, 2026-09-10) -- CLOSED the same day, and it was six

**In short:** several programs work out who is running them by reading an
environment variable. The environment is set by whoever starts the program, so
this asks the caller who they are and believes the answer. Three of them fall
back to *root* when the variable is missing -- and the variable is normally
missing, so they were not merely spoofable, they were unconditionally root.
`passwd` was fixed on 2026-09-10; these five were found by the same look.

| Program | Code | Falls back to |
|---|---|---|
| `userspace/chage` | `current_uid()` = `env::var("UID")` | **0 (root)**, and there is no other source |
| `userspace/newgrp` | `get_current_user()` = `env::var("UID")`/`GID` | **0 (root)**, and `$USER` for the name |
| `userspace/polkit` | `/proc/self/status` first, then `env::var("UID")` | **"0" (root)** if both fail |
| `userspace/crontab` | `effective_uid()` = `env::var("EUID")` | 1000 -- conservative, but `EUID=0` is still believed |
| `userspace/doas` | `/proc/self/status` first, then `env::var("UID")` | `u32::MAX` -- the safe direction |

**Why the fallback is the normal path, not a corner case.** `UID` and `EUID`
are *shell* variables, not exported ones. This tree's own shell is explicit
about it: "an inherited `UID=...` in the environment neither wins nor becomes
exported" (`userspace/oils/src/interp.rs`). So a program launched from `osh`
sees no `UID` at all, the `unwrap_or(0)` fires, and the answer is root every
single time. **No spoofing was required to get root; spoofing was required to
get anything else.**

**What it cost in `passwd`, which is why this is filed rather than noted.**
`is_root()` was `current_uid() == 0`, and it guarded two checks: "only root may
change another user's password" and "only root may use this option". Both were
therefore unreachable for the entire life of the program. A check that is
always skipped is indistinguishable from a check that always passes, and no
test could tell them apart because the decision was made in `main` from process
state.

**The fix, done once.** `authlib::identity::caller_uid()` returns
`Option<u32>` from `getuid(2)` -- the credential the kernel recorded, which the
process's parent cannot set. `None` means "this build cannot tell" and must
never be read as root. The caller's *name* is then resolved from that uid
against the account database, because `$USER` is the same spoofable input
wearing different clothes: in `passwd`, `USER=root passwd root` satisfied the
"changing your own password" exemption without being root.

**All converted, 2026-09-10.** `chage`, `newgrp`, `polkit`, `crontab`, `doas`
and `sudo` now take the caller's uid from `authlib::identity::caller_uid()`,
and their *names* from that uid resolved against the account database. `su`
was on the list too once its own fallback was read properly. Nothing in the
tree derives an identity from `$UID`, `$EUID`, `$GID` or `$USER` any more --
`grep -rn 'env::var("UID")'` returns only the doc comments describing what was
removed.

**Three things the conversion turned up that the survey above had not.**

- **`sudo` was a sixth.** `current_id_from_proc("Uid:", "UID")` read
  `/proc/self/status` and fell back to `$UID`, then to 1000. The non-zero
  default was reasoned and is kept (as `UNKNOWN_CALLER_ID`, with the reason
  attached), but the environment lookup between them was not: this value
  "decides whether a password is demanded", so `UID=0 sudo <command>` skipped
  the prompt wherever procfs was unreadable.

- **`doas` was worse than the table said.** Its `current_username` returned
  `$USER` *outright* when set, before any uid lookup -- and that name is what
  it matches `/etc/doas.conf` rules against. `USER=<anyone with a permit rule>
  doas <command>` inherited their rules. The uid fallback listed above was
  never reached in the case that mattered.

- **`crontab`'s fallback identity was the process id, not a uid.** Under a
  comment reading "use the numeric UID as the 'username' ... On a real Slate OS
  system this would call `getuid()`", the code was
  `format!("uid{}", std::process::id())`. A pid is unique per *invocation*, so
  `crontab -e` wrote `uid4123` and the `crontab -l` after it read `uid4127` and
  found an empty crontab. Not a security bug -- a plain one, sitting inside a
  security one, with a comment naming the fix it was waiting for and a premise
  that had already arrived.

**`/proc/self/status` is gone from this path too**, not just the environment
fallbacks. `getuid(2)` is strictly better than parsing it: no file to be
missing, no line to be absent, no parse to fail -- and every one of those
failure modes was what dropped control into the environment fallback in the
first place.

**`userspace/oils` is not on this list and is the reason the fix was easy.** It
had already reasoned the whole thing out for its own `$UID`, and reached the
opposite conclusion: consulting an environment variable "on a system that
*can* [supply the credential] would let any parent process redefine `$UID` by
exporting a variable, which is precisely the spoofing bash refuses when it
ignores an inherited `UID=`". The shell refused what six privileged programs
accepted. **A correct answer already in the tree does not propagate by
existing.**
