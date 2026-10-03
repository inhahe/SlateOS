## §924 — The Linux ABI gets the same file-permission checks as native code, with suspend-and-prompt paying for it

**Date:** 2026-09-09 (answered 2026-09-07; entry written 2026-09-09).
**Decided by:** Operator (Claude recommended C; operator chose A and overruled).
**Lane:** A.

**In short:** a program built for Linux could ask this system about a file —
"how big is it?", "when was it changed?" — without holding the permission token
our own programs are required to hold. Same question, same file, policed or not
depending on which door the program came in by. The operator's call is to
**police both doors**. A Linux program that lacks the token is not simply
refused: it is stopped at the check and the user is asked, or the permission is
granted ahead of time, so the program never learns it happened.

### Why this overruled the recommendation

The recommendation was C — draw the boundary explicitly and leave the Linux
door unpoliced — resting on the claim that Linux binaries *assume ambient
authority by construction* (permission you get by being you, with no token to
hold), so enforcing parity would break software nobody built for us.

The operator's answer defeats that claim rather than accepting the cost:

> "if you simply suspend the linux program long enough to ask the user for
> permission, or the user gives it permission ahead of time, then that's not an
> issue, right? It can be done without making any functional difference for the
> Linux program"

That is correct, and it is the piece the recommendation missed. A Linux binary
cannot *ask* for a capability — but it does not have to, if something else asks
on its behalf while it is suspended. From the program's side a prompt is
indistinguishable from a slow syscall. The objection was never really "Linux
programs can't hold capabilities"; it was "Linux programs can't *request* them",
and a prompt supplies the request from outside. This is the same mechanism the
operator approved in §918 for keyboard/microphone/camera, so it is one facility,
not two.

The honest caveat, which the operator stated first: a program denied a
capability may then misbehave in ways that look like bugs, so the denial must
warn plainly.

### The operator's two questions back

**1. "Do we have a mechanism to define what native programs (and also Linux
programs) under a given account are granted by default?"**

**No.** Nothing in `kernel/`, `posix/`, `services/` or `init/` carries
per-account capability defaults — there is no `default_caps`, no account policy
record, no grant set on the account at all. Capabilities are passed explicitly
per spawn (`SpawnOptions.capabilities`), and every launch site names its own.
So "I think we should" is a **new feature**, not a setting to switch on.

**2. "Should native programs also be granted them by default?"**

**Yes — the mechanism should cover both, but their default contents should
differ, and the reason matters more than the answer.**

The instinct against it is that default-granting to native programs would erode
"no ambient authority" for the half of the system that currently honours it.
That instinct is wrong, and the distinction is worth stating because it will
come up again: **a per-account default grant is not ambient authority.** Ambient
authority means there is no token — permission follows from identity and cannot
be handed over, held, or taken away. A default grant still produces a real
capability, handed to the process at spawn, which can be withheld, inspected and
revoked. What the account default changes is *who does the handing*, not whether
a token exists. The design rule survives intact.

So the split should be by *what the population can do*, not by which ABI it is:

| | default set | why |
|---|---|---|
| **Native** | minimal — ideally empty, or only what the program declares it needs and the account permits | a native program is ours and *can* ask, so a default is a convenience, and a broad one would waste the one advantage we have over the Linux side |
| **Linux** | broader, and user-configurable ("grant all Linux programs these") | a Linux binary cannot ask, so its alternative to a default is a prompt on every cold path, which trains the user to click yes |

Stated that way, the operator's "allow the user the option to grant all Linux
programs the capabilities by default" is not a special case in the checker — it
is simply an account whose Linux default set is non-empty.

### What this costs, and what is not yet decided

Enforcing parity means the Linux layer checks the rights the native layer does.
Counted against the tree on 2026-09-09 rather than copied from the question,
whose figures were written on 2026-09-03 and have since drifted (it said two
against eight). `linux.rs` holds **three** `require_cap_type` sites, and only
two of them concern the filesystem at all — one File-`READ` and one File-`WRITE`
(the `require_fs_write` helper); the third gates `InputDevice` for keyboard
reads and is unrelated to this question. It checks `Rights::METADATA`
**nowhere** — zero occurrences in the whole file — while `handlers.rs` carries
87 `require_cap_type` sites, **ten** of them requiring `METADATA`.

So the asymmetry is sharper than the question made it sound: on the specific
right at issue it is **ten against zero**, not two against eight. `stat`,
`lstat`, `statx`, `readlink`, `statvfs` and the xattr readers go straight
through. Blast radius as
filed: ~50 Path-Z tests, plus dash, tcc, python and `ld.so`.

That blast radius is the reason this is not a single change: **the prompt path
and the account default set have to exist before parity can be switched on**, or
every one of those launch sites fails with a permission error on a call it has
no reason to expect can fail. Order: account default-grant record → the
suspend-and-prompt path (shared with §918) → parity enforcement in `linux.rs`.

Not decided here, and deliberately left open: what a *headless* boot does when a
check needs a prompt and there is no user to ask. The boot test is exactly that
case, so it has to be answered before parity lands — but it is an implementation
question, not another decision for the operator.

**Where it bites:** `kernel/src/syscall/linux.rs` (the two filesystem `require_cap_type`
sites), `kernel/src/syscall/handlers.rs:8365+` (the native gates),
`kernel/src/proc/spawn.rs` (`SpawnOptions.capabilities`, the only place a grant
is made today).
