### TD-OILS-MSYS-EXIT127. Host probe artifact: MSYS bash exits **127** for fatal expansion errors where real bash exits **1** — NOT AN OSH BUG (osh is correct) 2026-07-19

**What:** when a *non-interactive* shell terminates due to a fatal parameter-
expansion error — an unbound variable under `set -u`, or a `${var:?word}` /
`${var:?}` reference to an unset/null variable — the shell prints the message and
exits. The **exit status** should be `1` (real GNU bash on Linux; confirmed via
bash docs/community references). The MSYS2 bash used for host comparison testing
(`5.2.37(1)-release (x86_64-pc-msys)`) instead exits with **127** for these
cases. `osh` correctly exits `1` and correctly *stops* (does not run the
following command), matching real bash on both axes.

**Why it's logged here:** so a future host `chk`/status-comparing probe that
flags `set -u; $undef`, `: ${x:?}`, or `${x:?msg}` as a status DIFF (osh 1 vs
MSYS-bash 127) is recognised as an **MSYS artifact**, not an osh regression —
do **not** "fix" osh to emit 127. This joins the other known MSYS host-probe
false positives (C-locale UTF-8 `$'\uXXXX'`, Windows path/OS-error text, `/tmp`
path-root, signal-number table, BASH_VERSINFO/BASHPID).
