## `TD-C-A-PUSH-UNDER-MSYS2-ARG-CONV-EXCL-HIDES-THE-HOOKS-TEMP-FILES` (lane C, 2026-10-07)

**Status:** OPEN (procedure, and a hardening the hook could take)

**In short:** a `git push` run with `MSYS2_ARG_CONV_EXCL='*'` in its
environment -- the setting that stops Git Bash rewriting arguments that look
like paths, which this machine's notes prescribe for running Windows programs
from bash -- is refused by the pre-push hook. Two gates hand a Windows Python a
list in a temporary file named `/tmp/...`; Git Bash normally rewrites that to
the Windows temporary directory on the way into the program, and with the
setting in force it does not, so Python looks for `\tmp\...` on the current
drive, finds nothing, and the gate refuses the push as having reached no
verdict. Nothing is wrong with the code being pushed. Push without the
setting.

**What it looks like.**

```
check-doc-links: cannot read /tmp/pre-push-doclink-dirs.198485: [Errno 2] No such file or directory
pre-push: REFUSING to push — the doc-links-<sha> checker never reached a verdict.
```

**Where.** `scripts/hooks/pre-push`: `doclink_list="${TMPDIR:-/tmp}/..."`
(the doc-links gate, read with `--paths-from`) and
`rootwrites_list="${TMPDIR:-/tmp}/..."` (the root-writes gate). The checkers
are run with `$py`, which on this machine is a Windows Python
(`command -v python3`). Git Bash's own mapping of `/tmp` -- to the user's
`AppData/Local/Temp` -- reaches a Windows program only through the argument
rewriting the setting turns off.

**Why it happened.** The setting is the right one for `cmd /c`, `taskkill
/PID` and the like, and it was reached for out of habit for `git push`. An
environment variable is inherited by every process below it: the hook, and
every checker the hook starts.

**The procedure.** Never put `MSYS2_ARG_CONV_EXCL` on `git push` or `git
commit` -- anything that runs the repository's hooks, which are bash scripts
written for Git Bash's normal rewriting.

**The hardening, if the hook's owners want it.** Name the temporary files in
a form both sides read the same: `cygpath -m` turns `/tmp/x` into
`C:/Users/.../Temp/x`, which bash and a Windows Python both open, with or
without the rewriting. Two lines, behaviour unchanged where the push works
today. `scripts/` is no lane's (A-Q11), so this is left as a suggestion here
rather than made from lane C.

**If never fixed:** a push made the wrong way is refused -- loudly, with the
cause printed -- and made again the right way. Nothing is published unchecked.
