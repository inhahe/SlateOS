## §372 — `stat`'s `%C` prints a silent `?`, because a security context is a field this system does not have rather than a lookup that failed

**Date:** 2026-08-23
**Decided by:** Claude (autonomous)

**In short:** `%C` asks `stat` to print a file's SELinux security label — a
Linux access-control tag that does not exist on this system at all. GNU, run on
a machine without SELinux, treats that as an *error*: it prints a complaint to
stderr for every file and exits 1. Ours prints a `?` in that column and says
nothing, the way it already does for other fields it has no value for.

**Measured, GNU 9.4 on a non-SELinux kernel:**

```
$ stat -c '%C' f
stat: failed to get security context of 'f': No data available
?
$ echo $?
1
```

**The decision.** `%C` renders `?`, adds no diagnostic, and does not set the
exit status. It is also omitted from `--help`'s specifier table, and from the
`--terse` format (upstream's terse constant appends `%C` on an SELinux build).

**For:**

* GNU itself uses exactly this rendering — a silent `?`, no message, no failure
  — for a field the kernel does not supply: `print_statfs`'s `%t` on a system
  whose `statfs` has no `f_type` member. Our `%C` is that case, not a failed
  syscall. `%t` in filesystem mode is likewise `?` here, for the same reason.
* Upstream's message would be a lie in its particulars: nothing "failed", and
  there is no errno to report, so the text would have to invent one.
* The failure is not actionable. A user cannot install a security context here;
  telling them once per file that they have not is noise that would make `stat
  -c '%C%s'` exit 1 on every well-formed run.
* Leaving it out of `--help` keeps the help honest: the table lists what the
  program can tell you, and this it cannot.

**Against:**

* A script ported from Linux that *relies* on the non-zero exit to detect an
  SELinux-less system will now see success. This is thin — such a script would
  more naturally test for the tooling — but it is the real behavioural
  difference.
* `%C` accepted-but-empty is a small piece of ambient dishonesty: it lets a
  format look supported when it is not. The counter is that `?` is precisely
  the "no value" marker, and it is the one GNU chose for the same situation.
* If a context concept is ever added, this becomes a directive that silently
  returned `?` for a while, and old output cannot be distinguished from new
  absent-value output. Accepted: at that point `%C` gains a value and the `?`
  means what it always meant.

**If this is revisited,** the thing to change is not the `?` — it is whether
`%C` should be an *unknown specifier* instead. It is currently indistinguishable
from one (both print `?`), which is deliberate: `%Q` and `%C` are equally
unsatisfiable here, and giving them the same rendering means one rule rather
than two.
