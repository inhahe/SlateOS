### TD-OILS-COMMAND-V-DOES-NOT-CONSULT-THE-HASH-TABLE. a described name does not ask the `hash` table — ✅ **FIXED 2026-08-04**

**Where:** `userspace/oils/src/interp.rs` — `Shell::command_describe_file` and
`Shell::builtin_type`, which went straight to the `$PATH` walk. The hash table
(`Shell::cmd_hash`) was read on the *execution* path but not on the
*description* one.

**Note on the original report.** This entry was first written from a model that
turned out to be partly wrong, and its original reproducers do not reproduce.
Measured against bash 5.2.37: `command -v` / `type` never *hash* a name (only
running it does), and **any** assignment to `$PATH` flushes the whole table — so
the original "hash it with `command -v`, then reassign `$PATH`" recipes emptied
the table before the interesting line ran. osh already got both of those right,
and already reported `tool is hashed (…)` from `type`'s verbose form.

**The real divergences** (measured with a fixture that is *run* to hash it, and
without touching `$PATH` afterwards) were narrower:

- `command -v` and `command -V` did not consult the table at all.
- `type -a` counted a hash entry as "found" when it should not: bash's `-a`
  (without `-P`) is the one form that ignores the table and walks `$PATH`.
- `type -P` and `type -p` checked the `$PATH` hits before the table, so a hash
  entry lost to a later `$PATH` answer.

**Reproduce (needs a real run to populate the table, and no later `PATH=`):**

```sh
mkdir -p bin bin2
printf '#!/bin/sh\necho one\n' > bin/tool
printf '#!/bin/sh\necho two\n' > bin2/tool
chmod +x bin/tool bin2/tool
PATH=bin:bin2; hash -r; tool > /dev/null     # remembers bin/tool
command -v tool          # bash: ./bin/tool   osh (before): bin/tool
type -P tool             # bash: ./bin/tool   osh (before): bin/tool
type -aP tool            # bash: ./bin/tool (one line, from the table)
type -ap tool            # bash: bin/tool then bin2/tool (the $PATH walk)
```

**The measured rule.** The table answers **every** describing form *except*
`-a` without `-P`: `command -v`, `command -V`, `type`, `type -t`, `type -p`,
`type -P` and `type -aP` all short-circuit to it, while `type -a`/`-at`/`-ap`
walk `$PATH`. A `PATH=` written as a *prefix* on the describing command bypasses
the table entirely — it is asking about a different search. `$EXECIGNORE` never
reaches a hash hit, because it filters the search and the search never runs.

**The fix.** Two predicates next to `hash_remember`, so no call site can drift
from another:

- `hashed_description(name) -> Option<Str>` — the one place that turns a table
  entry into the spelling it is *described* by (see
  TD-OILS-A-DESCRIBED-HASH-HIT-LOSES-ITS-DOT-SLASH below for what that spelling
  is).
- `hash_answers_description(mode_a, mode_pp) -> bool` — `mode_pp || !mode_a`,
  the whole truth table above in one expression.

`command_describe_file` now asks `hashed_description` before
`find_in_path_described`, gated on there being no `PATH=` prefix and no slash in
the name; `builtin_type` computes `hashed` once through
`hash_answers_description` and consults it ahead of `files` in every arm.
Covered by the unit test
`a_hashed_command_is_described_with_a_dot_slash_while_it_still_runs`; it cannot
be a corpus case because populating the table needs a *runnable* fixture, and
shebang scripts do not run on the Windows dev host
(HOST-OILS-SHEBANG-SCRIPTS-DO-NOT-RUN-ON-THE-WINDOWS-DEV-HOST).
