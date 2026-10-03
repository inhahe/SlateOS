## TD-B-AUDITD-LOGGED-A-DAEMON-START-THAT-NEVER-HAPPENED — 2026-09-15 — FIXED by refusing

**In short:** `auditd` wrote `DaemonStart … res=success` into the audit log and
returned 0, while the process exited immediately — there is no event loop and
no fork. The messages on stdout were honest about it; the log was not.

**The asymmetry is the defect.** stdout said *"auditd: would fork to background
(simulated)"* and *"daemon event loop would run here"* — both true, both
subjunctive, and both read once by whoever ran the command. The audit log said
a daemon started successfully, and an audit log is read **later**, by someone
reconstructing what happened, whose whole reason for consulting it is that it
can be trusted without corroboration. The terminal told the truth and the
durable record did not.

Returning 0 compounded it: an init script would have counted the service as up.

**The finding inside the finding.** Removing the two fabricated writes left
`write_audit_event` with no callers at all. **Those two events were the only
thing this program had ever written to the audit log.** Until today its sole
output was a record of something that did not occur — and an audit log whose
only entry is false is worse than an empty one, because an empty one is
obviously empty.

`write_audit_event` is kept with `#[allow(dead_code)]` and that explanation,
the way `gdb` keeps its inferior-control helpers while there is no `ptrace`:
it is the writer a real event source will need, and its record format is the
part worth preserving.

**How it was found:** by grepping the *shape* after fixing `nsenter` —
`For simulation|in real implementation, this would` across `userspace/*/src/`.
Three hits, two of them real (`unshare`, `audit`), one of them my own comment
quoting the original. Neither `unshare` nor `audit` was on the
advertised-but-unread ranking, because in both the fields **are** read — by
the simulation.

### The sweep was then WIDENED, because three hits was a fact about the query

That first grep matched two exact phrasings. Widening it to
`simulat|in a real (daemon|implementation)|would (fork|run here|be done|actually)`
matches **62 files**, not three — so "three hits" described what I asked, not
what is there. (Lane A's rule, recorded in design-decisions §1022: a negative
about a searchable corpus is a search, not an inference.)

62 mentions are mostly benign, so the population that matters is the
intersection with a real side effect — `Command::new`, `fs::remove_*`,
`fs::write`, `fs::rename`, `execv`. Ranked by that, and **checked rather than
assumed**, the security-critical head of the list came back clean:

| checked | verdict |
|---|---|
| `sudo` | **already fixed.** Calls `authlib::identity::become_user` before spawning. Its own comment records the defect I was reconstructing: *"Until now sudo authorised the command … and then ran it as the caller."* My `setuid` grep missed it because the call goes through `authlib`. |
| `su` | same shared helper, same ordering |
| `authlib::identity::become_user` | correct, and documents its own remaining gap (supplementary groups from `userdb`) |
| `firejail` | its "we simulate it" is `create_symlink` portability in a symlink-installer, not sandboxing |

So the shape is real and rarer than the raw grep count suggests. What separates
a defect from a benign mention is not the word "simulate" — it is whether a
**consequential action happens anyway**, which no text search can answer.

**Where it lives:** `userspace/audit/src/main.rs`, the `auditd` start path.
