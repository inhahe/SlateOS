### TOOLING-BASH-5.2.37-SOURCE. A local copy of the reference shell's own source, at `D:\refsrc\bash-5.2`

⚠️ **Scope note (2026-08-14, §305):** the parity goal described below is now
**capped** — see the banner at the top of this file. This source tree remains the
right way to *answer* a parity question, but the question should only be asked
for a divergence that passes §305's criterion. Historically, it was asked for
everything.

The oils work is driven toward byte-exact parity with the bash on this machine,
`C:\Program Files\Git\usr\bin\bash.exe`, which reports
`5.2.37(1)-release (x86_64-pc-msys)`. Matching a diagnostic by measurement alone
repeatedly produced plausible-but-wrong rules (see the `expand_declaration_argument`
history in
TD-OILS-A-WHOLE-ARRAY-REFERENCE-UNDER-A-DECLARATION-BUILTIN-IS-MISSING-ITS-BAD-SUBSCRIPT-LINE),
so the matching source is now on disk:

* `D:\refsrc\bash-5.2` — `bash-5.2.tar.gz` from ftp.gnu.org with all 37 official
  patches (`bash-5.2-patches/bash52-001` … `-037`) applied via
  `patch -p0 -s -N`. `patchlevel.h` reads `#define PATCHLEVEL 37`, i.e. exactly
  the shell osh is diffed against.

Nothing in the repo depends on it; it is a read-only reference. The files that
come up most often are `builtins/declare.def`, `subst.c`, `arrayfunc.c`,
`variables.c` and `execute_cmd.c`.

**Method note.** Measure bash first, then read the source to *explain* the
measurement — never the other way round, and never infer which agent emitted a
line from its wording alone (the tag is `this_command_name` at emission time,
which several paths reach with it unset). Where a recorded hypothesis and a
fresh measurement disagree, the measurement wins.
