#!/bin/bash
# pwdx-diff.sh -- run our `pwdx` and procps-ng's side by side and report every
# case where they disagree.
#
# ## The reference
#
# Ubuntu's /usr/bin/pwdx, procps 4.0.4-4ubuntu3.2. None of the patches Debian
# and Ubuntu carry touch `src/pwdx.c` -- the package changelog never names
# it -- so the installed binary is upstream's program.
#
# ## The processes it asks about
#
# Sleepers this harness starts, each in a directory chosen for what its name
# tests: a plain one; one whose name holds a newline, a space, a \xff byte and
# an escape sequence (printed raw on standard output, as data); one deleted
# after the sleeper entered it (`readlink` then appends ` (deleted)`); and one
# longer than the 128 bytes `pwdx`'s first `readlink` buffer holds. Then a
# zombie, whose `cwd` link the kernel refuses with ENOENT -- which `pwdx`
# reports as `No such process` -- and init, which is not ours to read. Both
# sides ask about the same processes, so the answers compare byte for byte.
# Every one is killed by its pid when the harness exits.
#
# ## The knobs, reset before each group
#
#   ARGV0  argv[0] for the run, set with `exec -a`. getopt names the program
#          as argv[0] has it and everything else by its last component.
#   ERRTO  `merge` puts standard error into the standard output file, so the
#          two are compared *in order*: a refusal goes out through glibc's
#          `error()`, which flushes standard output first, and a process it
#          cannot read through `fprintf (stderr, ...)`, which does not.
#          `full` and `closed` send it to /dev/full or close it.
#   OUTTO  where standard output goes: `full`, `closed`, `epipe` (a pipe
#          whose reader has gone, SIGPIPE ignored -- procps' close_stdout is
#          silent about that and keeps the status) or `sigpipe` (the same with
#          SIGPIPE at its default, which kills it).
#   ENVV   assignments for `env`: POSIXLY_CORRECT stops getopt at the first
#          operand.
#
# argc == 0 is not a case: since Linux 5.18 the kernel hands a program run
# with an empty argv one empty argument instead, which is the `ARGV0=''` case.
# The branch is covered by the unit tests.
#
# ## Cases that differ on purpose
#
# `--version` names SlateOS, and an argument echoed back on standard error
# that is not printable is escaped there (`\033`, `\377`), where upstream
# writes it raw.
#
# Run `OURS=/usr/bin/pwdx ./scripts/pwdx-diff.sh` to check that the harness
# still discriminates: every case that differs on purpose should then be
# reported as no longer differing, and nothing else.
set -u

DIFF_PROG='pwdx'
DIFF_NEED='timeout python3'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

# --- the processes ------------------------------------------------------------
pids=()
diff_cleanup() {
  [ "${#pids[@]}" -gt 0 ] && kill "${pids[@]}" 2>/dev/null
  chmod -R u+rwx "$DIFF_TMP" 2>/dev/null
  rm -rf "$DIFF_TMP"
  return 0
}

plain=$DIFF_TMP/plain
odd=$DIFF_TMP/$'odd\nname \xff\e[1m'
gone=$DIFF_TMP/gone
long=$DIFF_TMP/$(printf 'd%.0s' $(seq 100))/$(printf 'e%.0s' $(seq 100))
mkdir -p "$plain" "$odd" "$gone" "$long"

# wait_cwd PID DIR: until the process has entered DIR, at most two seconds.
wait_cwd() {
  local n=0
  until [ "$(readlink "/proc/$1/cwd")" = "$2" ]; do
    n=$((n+1))
    if [ "$n" -gt 200 ]; then
      echo "pwdx-diff: process $1 never entered $2" >&2
      exit 1
    fi
    sleep 0.01
  done
}

# sleeper DIR: a process whose working directory is DIR; its pid in $sp.
sleeper() {
  (cd "$1" && exec sleep 600) &
  sp=$!
  pids+=("$sp")
  wait_cwd "$sp" "$1"
}

sleeper "$plain"; p_plain=$sp
sleeper "$odd"; p_odd=$sp
sleeper "$long"; p_long=$sp
sleeper "$gone"; p_gone=$sp
rmdir "$gone"

# A zombie: a child that has exited, of a parent that never waits for it.
python3 -c '
import os, time
pid = os.fork()
if pid == 0:
    os._exit(0)
print(pid, flush=True)
time.sleep(600)
' >"$DIFF_TMP/zombie" &
pids+=("$!")
n=0
until [ -s "$DIFF_TMP/zombie" ]; do
  n=$((n+1)); [ "$n" -gt 200 ] && { echo "pwdx-diff: no zombie" >&2; exit 1; }
  sleep 0.01
done
p_zombie=$(cat "$DIFF_TMP/zombie")
n=0
until grep -q '^State:.*Z' "/proc/$p_zombie/status" 2>/dev/null; do
  n=$((n+1)); [ "$n" -gt 200 ] && { echo "pwdx-diff: $p_zombie never became a zombie" >&2; exit 1; }
  sleep 0.01
done

# A number no process has: above any pid_max Linux allows (2^22).
p_none=2147483646

# --- one run ------------------------------------------------------------------
# run1 OUTTO ERRTO OUT ERR ARGV0 BIN ARGS...: BIN run as ARGV0 with its
# descriptors sent where OUTTO and ERRTO say; exits with BIN's status.
cat >"$DIFF_TMP/run1" <<'EOF'
#!/bin/bash
outto=$1 errto=$2 out=$3 err=$4 a0=$5 bin=$6
shift 6
case $outto in
  file) exec >"$out" ;;
  full) exec >/dev/full ;;
  closed) exec >&- ;;
esac
case $errto in
  file) exec 2>"$err" ;;
  merge) exec 2>&1 ;;
  full) exec 2>/dev/full ;;
  closed) exec 2>&- ;;
esac
case $outto in
  epipe|sigpipe)
    if [ "$outto" = epipe ]; then trap '' PIPE; else trap - PIPE; fi
    # The reader is gone before the first write.
    { sleep 0.3; exec -a "$a0" "$bin" "$@"; } | true
    exit "${PIPESTATUS[0]}"
    ;;
esac
exec -a "$a0" "$bin" "$@"
EOF
chmod +x "$DIFF_TMP/run1"

ARGV0='pwdx'; OUTTO='file'; ERRTO='file'; ENVV=()
reset_knobs() { ARGV0='pwdx'; OUTTO='file'; ERRTO='file'; ENVV=(); }

run_side() {
  local side=$1; shift
  : >"$DIFF_TMP/$side.out"; : >"$DIFF_TMP/$side.err"
  diff_run timeout -k 2 30 env ${ENVV[@]+"${ENVV[@]}"} "$DIFF_TMP/run1" \
    "$OUTTO" "$ERRTO" "$DIFF_TMP/$side.out" "$DIFF_TMP/$side.err" \
    "$ARGV0" "$bindir/$side/pwdx" "$@"
  printf '%s\n' "$?" >"$DIFF_TMP/$side.rc"
}

same() {
  local f
  for f in out err rc; do
    cmp -s "$DIFF_TMP/ours.$f" "$DIFF_TMP/gnu.$f" || return 1
  done
}

show() {
  local side f
  for side in ours gnu; do
    printf -- '--- %s: status %s\n' "$side" "$(cat "$DIFF_TMP/$side.rc")"
    for f in out err; do
      printf '%s:\n' "$f"
      od -An -c "$DIFF_TMP/$side.$f" | head -20
    done
  done
}

# check LABEL ARGS...: both sides, the knobs as they stand.
check() {
  local label=$1; shift
  run_side ours "$@"
  run_side gnu "$@"
  if same; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n' "$label"
    show
  fi
  return 0
}

# xcheck WHY LABEL ARGS...: a case that differs on purpose.
xcheck() {
  local why=$1 label=$2; shift 2
  run_side ours "$@"
  run_side gnu "$@"
  if same; then
    xpass=$((xpass+1))
    printf 'XPASS %s -- expected to differ (%s) and did not\n' "$label" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL %s (%s)\n' "$label" "$why"
  fi
  return 0
}

# --- processes ------------------------------------------------------------------
check "a process" "$p_plain"
check "its /proc path" "/proc/$p_plain"
check "several, one twice" "$p_plain" "$p_long" "$p_plain"
check "a directory longer than the first buffer" "$p_long"
check "a directory named with a newline, a space, \\xff and an escape" "$p_odd"
check "a directory deleted after it was entered" "$p_gone"
check "a zombie" "$p_zombie"
check "init, not ours to read" 1
check "a pid nothing has" "$p_none"
check "this harness" "$$"

# --- what strtol takes and the kernel does not ------------------------------------
check "a leading space" " $p_plain"
xcheck "escaped on stderr" "a leading tab and newline" $'\t\n'"$p_plain"
check "a plus sign" "+$p_plain"
check "a leading zero" "0$p_plain"
check "/proc/ then a space" "/proc/ $p_plain"
check "a number past int" 4294967297
check "the largest long" 9223372036854775807

# --- what it refuses -----------------------------------------------------------
# After `--`, so that a word starting with `-` reaches strtol and not getopt.
for a in 0 -1 -0 '' ' ' + - 12x '12 ' 0x10 /proc/ "/proc/$p_plain/" \
         "/proc//$p_plain" "proc/$p_plain" abc 99999999999999999999 \
         -9223372036854775808 -9223372036854775809; do
  check "refuses '$a'" -- "$a"
done
check "-1 without -- is an option" -1
check "stops at the first refusal" "$p_plain" bad "$p_long"
xcheck "escaped on stderr" "refuses an escape sequence" $'\e[31mred'
xcheck "escaped on stderr" "refuses a \\xff byte" $'12\xff'
xcheck "escaped on stderr" "a tab before a pid nothing has" $'\t'"$p_none"

# --- the order of the two streams ---------------------------------------------
ERRTO=merge
check "a refusal comes after what was printed" "$p_plain" bad "$p_long"
check "a process it cannot read comes before what is buffered" \
  "$p_plain" "$p_none" "$p_long"
many=()
for _ in $(seq 150); do many+=("$p_long"); done
check "past one buffer, a failure in the middle" "${many[@]}" "$p_none" "${many[@]}"
check "past one buffer, a refusal in the middle" "${many[@]}" bad "${many[@]}"
reset_knobs

# --- options -------------------------------------------------------------------
check "no operands"
xcheck "names SlateOS" "-V" -V
xcheck "names SlateOS" "--version" --version
xcheck "names SlateOS" "--vers" --vers
xcheck "names SlateOS" "-Vh" -Vh
xcheck "names SlateOS" "-V after an operand" "$p_plain" -V
for a in -h --help --he --h -hV; do
  check "$a" "$a"
done
check "-h after an operand" "$p_plain" -h
check "-h after a refusal" bad -h
for a in -x -Z --bogus --help=x --version=x -- - --=x; do
  check "$a alone" "$a"
done
check "-- then a pid" -- "$p_plain"
check "-- then -h" -- -h
ENVV=(POSIXLY_CORRECT=1)
check "POSIXLY_CORRECT: -h after an operand is an operand" "$p_plain" -h
check "POSIXLY_CORRECT: -h first" -h
reset_knobs

# --- argv[0] -------------------------------------------------------------------
for a0 in /usr/local/bin/pwdx ./pwdx '' bin/ $'pw\ndx' x; do
  ARGV0=$a0
  check "argv[0] '$a0': an unknown option" -x
  check "argv[0] '$a0': no operands"
  check "argv[0] '$a0': a refusal" bad
  check "argv[0] '$a0': --help" --help
  check "argv[0] '$a0': a process" "$p_plain"
done
reset_knobs

# --- where the output goes -------------------------------------------------------
OUTTO=full
check "stdout full: a process" "$p_plain"
check "stdout full: --help" --help
check "stdout full: --version" --version
check "stdout full: a refusal after a process" "$p_plain" bad
check "stdout full: a pid nothing has" "$p_none"
check "stdout full: past one buffer" "${many[@]}"
OUTTO=closed
check "stdout closed: a process" "$p_plain"
check "stdout closed: a pid nothing has" "$p_none"
check "stdout closed: no operands"
OUTTO='file'; ERRTO='full'
check "stderr full: a pid nothing has" "$p_none"
check "stderr full: a refusal" bad
check "stderr full: an unknown option" -x
check "stderr full: a process" "$p_plain"
ERRTO=closed
check "stderr closed: a pid nothing has" "$p_none"
check "stderr closed: no operands"
ERRTO='file'; OUTTO='epipe'
check "reader gone, SIGPIPE ignored: a process" "$p_plain"
check "reader gone, SIGPIPE ignored: and a pid nothing has" "$p_plain" "$p_none"
check "reader gone, SIGPIPE ignored: --help" --help
check "reader gone, SIGPIPE ignored: past one buffer" "${many[@]}"
OUTTO=sigpipe
check "reader gone: a process" "$p_plain"
check "reader gone: past one buffer" "${many[@]}"
reset_knobs

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
