## §95 — osh `compgen -A hostname`: invalidate the kept list by *value*, not by an assignment hook

**Date:** 2026-07-31
**Decided by:** Claude (autonomous)

**The problem.** bash's `hostname` completion action offers the names in
`$HOSTFILE` (or `/etc/hosts` when it is unset), and it *keeps* the list rather
than re-reading the file per completion. The list's lifetime is observable, and
the two ways it changes are not symmetric:

* naming a different `HOSTFILE` **adds** that file's names to the ones already
  offered, and
* *unsetting* `HOSTFILE` throws the list away, so the next file named starts
  from nothing.

Both were measured against bash 5.2 and both are in bash's manual. But bash
re-reads whenever `HOSTFILE` is **assigned**, not when its value changes — so
`HOSTFILE=f; compgen -A hostname; HOSTFILE=f; compgen -A hostname` offers every
name in `f` twice. That third behaviour is not in the manual; it falls out of
bash hanging the invalidation off `sv_hostfile`, which its assignment path calls
unconditionally.

**Options.**

* **A — a general variable-assignment hook**, bash's
  `stupidly_hack_special_variables`. Reproduces all three behaviours exactly.
  But osh has no such hook today, and adding one means finding *every* path that
  can write a variable — `HOSTFILE=x cmd` prefixes, `read`, `declare`,
  `printf -v`, `local` going out of scope, environment import — and firing a
  side effect from each, in order to emit duplicate completions.
* **B (chosen) — remember the `HOSTFILE` value the list was read from**
  (`hostname_source: Option<Option<Str>>`, the outer `None` meaning "nothing
  read yet", the inner one "`HOSTFILE` unset"), and re-read only when the value
  differs — plus a single hook in `Shell::unbind_var` for the clearing half.
* **C — re-read the file on every completion, no list at all.** Simplest, but
  wrong for both of the *documented* behaviours: the additive one would be lost
  outright, and a `HOSTFILE` naming an unreadable file would stop answering
  where bash keeps answering from the list it already has.

**Why B.** It matches the manual exactly — "the next time hostname completion is
attempted **after the value is changed**" — and it matches bash byte-for-byte on
everything except the redundant-assignment duplicate, which is the one part that
is an implementation artefact rather than a described behaviour. The unset half
*is* behaviour, so it gets a real hook, but `unbind_var` is a single, obvious
choke point (it is exactly where bash's `unbind_variable` reaches
`clear_hostname_list`) rather than a new cross-cutting mechanism. This follows
the same line already drawn for bash's doubled circular-nameref warning
(TD-OILS-NAMEREF-WARNING-COUNT) and its self-inconsistent post-`wait` job table:
copy the behaviour, not the internals that produce it.

**What is given up.** Duplicate candidates after a redundant assignment, logged
as TD-OILS-COMPGEN-HOSTFILE-REASSIGN and deliberately kept out of the corpus.

**How to reverse.** If osh ever grows a general assignment hook for another
reason, `Shell::compgen_hostnames` becomes the trivial "if not initialised,
read" that bash has, and the value comparison drops out. The cache lives in two
fields and one function.
