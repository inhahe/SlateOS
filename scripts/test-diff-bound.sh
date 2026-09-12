#!/usr/bin/env bash
# Two-probe test for the bound that `diff-wsl.sh` puts around every harness.
#
# A bound needs proof it RUNS and proof it can REFUSE, and the two halves fail
# in opposite directions. A bound that never fires is indistinguishable from no
# bound at all -- which is what this family had, and an `awk` case orphaned for
# 35 minutes because of it -- while a bound that fires on a slow-but-finite case
# turns a verdict flaky and gets the check switched off.
#
# THE LAST THREE CASES ARE THE INTERESTING ONES. They assert that the bound is
# NOT in the subject's exec path, and they exist because two earlier designs put
# it there. The second -- a wrapper script standing in `$bindir` where the
# symlink goes -- looked right and left `stat-diff.sh` at 77-11-19 unchanged, and
# still broke `nohup-diff.sh` from 75-0 to 74-1 on its one `2>&-` case: a shell
# reopens the standard descriptors before the script it interprets runs, so a
# subject launched through a shell-shebang wrapper can no longer tell that its
# caller closed stderr. `timeout` itself is transparent, which is why the six
# language harnesses may keep an inner per-case one.
#
# They are stated RELATIVE to a direct invocation rather than against a fixed
# list, because the harness leaks a descriptor of its own to every subject and
# always has. Hard-coding the list would make this test a record of that leak,
# which is not what it is for, and would fail the day the leak is fixed.
set -u

# Both sides point at real binaries: what this checks is what a subject reached
# through `$bindir` can see, so `$bindir` has to be built normally. `bash`
# reports `argv[0]` and `PATH`; the descriptor question needs `ls`, because a
# shell would reopen the descriptor it is being asked about before it could
# report on it. There is no program called `bound` -- `DIFF_REF` only satisfies
# the preamble's insistence that the named subject have a reference, which the
# family arm never consults.
DIFF_PROG='bound'
DIFF_BINS='bash ls'
OURS=/bin
DIFF_REF=/bin/ls
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0
ck() {
  if [ "$2" = "$3" ]; then
    pass=$((pass+1))
  printf 'ok   %s\n' "$1"
  else
    fail=$((fail+1))
  printf 'FAIL %s\n  expected: %s\n  actual:   %s\n' "$1" "$2" "$3"
  fi
}

# --- a harness of our own, to be bounded -----------------------------------------
# It backgrounds a sleep before hanging, so one run proves both halves: that the
# harness is killed, and that what the harness SPAWNED dies with it. The orphan
# is the part that actually cost 35 minutes.
#
# The pid comes back through a file named on the command line. Not stdout: `$(
# )` waits for every writer to close the pipe and the backgrounded sleep is one
# of them, so the capture would outlive the bound it is measuring. Not
# `$DIFF_TMP` either -- the inner harness sources the preamble too and gets a
# temp directory of its own, which is not this one.
inner=$DIFF_TMP/inner-diff.sh
inner=$DIFF_TMP/inner-diff.sh
cat > "$inner" <<'INNER'
#!/usr/bin/env bash
set -u
DIFF_PROG='inner'
OURS=/usr/bin/true
DIFF_REF=/usr/bin/true
DIFF_NO_BINDIR=1
INNER
# The path is QUOTED in the generated line. This repository lives under
# 'visual studio projects', and an unquoted source line made the inner harness
# die at 'no such file or directory' before it ever reached the preamble --
# so it was never bounded, and this test reported that as the bound failing to
# fire. A test whose fixture is broken accuses the code it is testing.
printf '. "%s/diff-wsl.sh"\n' "$(cd "$(dirname "$0")" && pwd)" >> "$inner"
cat >> "$inner" <<'INNER'
if [ "${1:-}" = quick ]; then exit 7; fi
/bin/sleep 300 &
echo "$!" > "$1"
/bin/sleep 300
INNER
chmod +x "$inner"

# --- proof it does not fire on a harness that finishes -------------------------------
DIFF_TIMEOUT=60 bash "$inner" quick >/dev/null 2>&1; rc=$?
ck "a harness that finishes keeps its own exit status" 7 "$rc"

# --- proof it fires -------------------------------------------------------------------
gcfile=$DIFF_TMP/gc.pid
innerlog=$DIFF_TMP/inner.log
t0=$SECONDS
DIFF_TIMEOUT=3 bash "$inner" "$gcfile" >"$innerlog" 2>&1; rc=$?
el=$((SECONDS - t0))
gc=$(cat "$gcfile" 2>/dev/null || true)
if [ "$rc" != 124 ]; then printf "  inner harness said:
"; sed "s/^/    /" "$innerlog"; fi
ck "a harness that hangs is killed" 124 "$rc"
ck "and killed at the bound, not after the sleep" yes "$([ "$el" -lt 40 ] && echo yes || echo "no (${el}s)")"

# The signal goes to the process GROUP, so the backgrounded sleep dies too.
# Without that this would swap one orphan for another. A pid must actually have
# been recorded, or the check passes by measuring nothing.
if [ -z "$gc" ]; then
  ck "what the harness spawned is killed with it" "a recorded pid" "no pid recorded"
elif kill -0 "$gc" 2>/dev/null; then
  ck "what the harness spawned is killed with it" dead "alive (pid $gc)"
  kill -9 "$gc" 2>/dev/null || true
else
  ck "what the harness spawned is killed with it" dead dead
fi

# --- proof the bound is not in the subject's exec path -----------------------------------
a0=$(/usr/bin/env "PATH=$bindir/ours" bash -s <<< 'echo "argv0=$0"')
ck "argv[0] reaches the subject as the bare word" "argv0=bash" "$a0"
pv=$(/usr/bin/env "PATH=$bindir/ours" bash -s <<< 'echo "path=$PATH"')
ck "PATH reaches the subject with nothing added" "path=$bindir/ours" "$pv"

# Same binary, same redirection, same shell -- the only difference is whether it
# was reached through `$bindir`. Any layer inserted there shows up as a changed
# descriptor table; the harness's own leak cancels.
fd_direct_closed=$(/usr/bin/env /bin/ls /proc/self/fd 2>&- | tr "
" " ")
fd_bindir_closed=$(/usr/bin/env "PATH=$bindir/ours" ls /proc/self/fd 2>&- | tr "
" " ")
ck "a closed stderr reaches the subject closed" "$fd_direct_closed" "$fd_bindir_closed"
fd_direct_open=$(/usr/bin/env /bin/ls /proc/self/fd 2>/dev/null | tr "
" " ")
fd_bindir_open=$(/usr/bin/env "PATH=$bindir/ours" ls /proc/self/fd 2>/dev/null | tr "
" " ")
ck "an open stderr reaches the subject open" "$fd_direct_open" "$fd_bindir_open"
ck "and the two differ, so the check can tell them apart" different "$([ "$fd_direct_closed" != "$fd_direct_open" ] && echo different || echo same)"

printf '\n%d passed, %d failed\n' "$pass" "$fail"
[ "$fail" = 0 ]
