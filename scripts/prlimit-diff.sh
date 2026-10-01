#!/usr/bin/env bash
# Differential test: our `prlimit` against util-linux 2.39.3's.
#
# What is compared: stdout, stderr and the exit status of each case, run by
# each side in turn:
#
#   * the table -- every resource, each alone and in any order, --raw,
#     --noheadings, -o in its forms -- and on a terminal of many widths, where
#     libsmartcols (the `smartcols` crate) truncates the TRUNC columns;
#   * setting limits, for a COMMAND that reports what it got (`ulimit`) and
#     for another process (--pid), in every range form upstream parses --
#     `soft:hard`, `soft:`, `:hard`, `unlimited`, and its quirks: `-1` is
#     unlimited, `10abc` is 10, `unlimitedfoo` is unlimited;
#   * the refusals, and a COMMAND that cannot be run;
#   * what glibc's buffering makes of all that: on a pipe the table and
#     --verbose's lines are lost when COMMAND runs, and printed when it
#     cannot; and closed or full standard descriptors.
#
# The one figure that differs run to run is a PID in --verbose's line, which
# names prlimit's own process; it is compared as `pid N`.
#
# At the narrowest terminals upstream never finishes: libsmartcols' width
# arithmetic wraps a size_t to a column width in the quintillions and prints
# spaces until it is killed. The port stops at zero (the smartcols crate's
# docs, "Where it is not upstream's"). Such a case is counted apart, as
# `hung`: it passes only if upstream was the side that had to be killed and
# ours finished cleanly.
set -u

DIFF_PROG='prlimit'
DIFF_PKG='prlimit'
DIFF_NEED='timeout script stty sleep'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; broken=0; hung=0

# $1 = side, rest = argv.
run_side() {
  local side=$1; shift
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/$side:$PATH" timeout -k 2 20 prlimit "$@"
}
# $1 = side, $2 = a bash script that runs `prlimit` from that side's PATH.
run_side_sh() {
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/$1:$PATH" timeout -k 2 20 bash -c "$2"
}
# $1 = side, $2 = columns, rest = argv: on a pty of that width.
run_side_pty() {
  local side=$1 cols=$2 cmd; shift 2
  cmd="stty cols $cols rows 60; prlimit"
  if [ $# -gt 0 ]; then cmd="$cmd$(printf ' %q' "$@")"; fi
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/$side:$PATH" \
    timeout -k 1 5 script -qec "$cmd" /dev/null </dev/null
}

normalize() { sed -E 's/ for pid [0-9]+:/ for pid N:/'; }

judge() {
  local o_rc=$1 g_rc=$2
  if [ "$o_rc" = 127 ] && [ ! -s "$DIFF_TMP/o.err" ] || [ "$o_rc" = 124 ] || [ "$g_rc" = 124 ]; then
    AGREED=broken
  elif [ "$(normalize < "$DIFF_TMP/o.out")" = "$(normalize < "$DIFF_TMP/g.out")" ] \
     && cmp -s "$DIFF_TMP/o.err" "$DIFF_TMP/g.err" && [ "$o_rc" = "$g_rc" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out %q err %q\n  gnu  (rc=%s): out %q err %q' \
    "$o_rc" "$(head -c 2000 "$DIFF_TMP/o.out")" "$(cat "$DIFF_TMP/o.err")" \
    "$g_rc" "$(head -c 2000 "$DIFF_TMP/g.out")" "$(cat "$DIFF_TMP/g.err")")
}

report() {
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1)); printf 'BROKEN %s\n%s\n' "$1" "$REPORT"
  elif [ "$AGREED" = yes ]; then
    pass=$((pass + 1)); [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$1"
  else
    fail=$((fail + 1)); printf 'DIFF %s\n%s\n' "$1" "$REPORT"
  fi
  return 0
}

# case_ ARGV...
case_() {
  local o_rc g_rc
  run_side ours "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side gnu "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "prlimit $(printf '%q ' "$@")"
}
# sh_case SCRIPT: a bash script calling `prlimit`, run for each side.
sh_case() {
  local o_rc g_rc
  run_side_sh ours "$1" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side_sh gnu "$1" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "bash -c $(printf '%q' "$1")"
}
# pty_case COLS ARGV...
pty_case() {
  local cols=$1 o_rc g_rc; shift
  run_side_pty ours "$cols" "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side_pty gnu "$cols" "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  if { [ "$g_rc" = 124 ] || [ "$g_rc" = 137 ]; } && [ "$o_rc" = 0 ]; then
    hung=$((hung + 1))
    if [ -n "${VERBOSE:-}" ]; then
      printf 'HUNG [pty %s cols] prlimit %s (upstream never finished; ours did)\n' "$cols" "$*"
    fi
    return 0
  fi
  judge "$o_rc" "$g_rc"
  report "[pty ${cols} cols] prlimit $(printf '%q ' "$@")"
}
# redir_case HOW ARGV...: under an unwritable descriptor, applied by the
# shell that execs prlimit, since the harness's own `diff_run` needs 2.
redir_case() {
  local how=$1 o_rc g_rc; shift
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/ours:$PATH" timeout -k 2 20 \
    bash -c "exec prlimit \"\$@\" $how" prlimit "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/gnu:$PATH" timeout -k 2 20 \
    bash -c "exec prlimit \"\$@\" $how" prlimit "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "prlimit $(printf '%q ' "$@")$how"
}

# --- options and their refusals -----------------------------------------------
case_ -h
case_ --help
case_ -V
case_ --version
case_ --vers
case_ --bogus
case_ -z
case_ --s
case_ --no
case_ --noh
case_ --nof
case_ --r
case_ -o
case_ -o FOO
case_ -o RESOURCE,FOO,SOFT
case_ -o soft,,hard
case_ -o ''
case_ -o ,
case_ -o SOFT,
case_ -o RESOURCE,SOFT,HARD,UNITS,DESCRIPTION,RESOURCE,SOFT,HARD,UNITS,DESCRIPTION
case_ -o RESOURCE,SOFT,HARD,UNITS,DESCRIPTION,RESOURCE,SOFT,HARD,UNITS,DESCRIPTION,RESOURCE
case_ -p
case_ -p x
case_ -p ''
case_ -p 99999999999
case_ -p 1 true
case_ -p 0 -p 1
case_ -p 1 -p 1
case_ -p 2147483647
case_ --nofile=abc
case_ --nofile=
case_ -n=5:4
case_ --nofile=:x
case_ --nofile=5:x
case_ --nofile=99999999999999999999
case_ --nofile==5 true
case_ -n==5

# --- the table ------------------------------------------------------------------
for args in '' '--raw' '--noheadings' '--raw --noheadings' '-o RESOURCE' \
            '-o hard,soft' '-o UNITS,RESOURCE,UNITS' '-o DESCRIPTION -o SOFT' \
            '-c' '-d' '-e' '-f' '-i' '-l' '-m' '-n' '-q' '-r' '-s' '-t' '-u' '-v' '-x' '-y' \
            '--core' '--as' '--rttime' '--nofile --nice --stack' '-n -n' \
            '--nofile --raw -o SOFT,HARD'; do
  # Word-split on purpose: each entry is an argument list.
  # shellcheck disable=SC2086
  case_ $args
done
for cols in 1 10 20 30 40 45 50 55 60 64 70 80 100 132; do
  pty_case "$cols"
  pty_case "$cols" --nofile --nice
  pty_case "$cols" -o DESCRIPTION,RESOURCE
  pty_case "$cols" -o HARD
done

# --- setting limits, for a command -----------------------------------------------
REPORT_N='ulimit -Sn; ulimit -Hn'
for v in 100:200 100: :4096 100 unlimited -1 10abc unlimitedfoo 0:0 \
         4096:unlimited unlimited:; do
  case_ --nofile="$v" sh -c "$REPORT_N"
done
case_ -n=100:200 sh -c "$REPORT_N"
case_ -n100:200 sh -c "$REPORT_N"
case_ --nofile=100:200 --stack=:unlimited sh -c "$REPORT_N; ulimit -Hs"
case_ --verbose --nofile=100:200 true
case_ --verbose --nofile=100:200
case_ --verbose --nofile=100:200 --nice=0:0
case_ --nofile --nice sh -c 'echo child'
case_ --nofile=100:200 --nice sh -c "$REPORT_N"
case_ --cpu=5:10 sh -c 'ulimit -St; ulimit -Ht'
case_ --core=0 sh -c 'ulimit -c'
case_ --nofile nosuchcommand
case_ --nofile /etc/passwd
case_ --nofile /tmp
case_ --verbose --nofile=100:200 nosuchcommand
case_ -- sh -c 'echo only'

# --- another process -------------------------------------------------------------
# Each side gets a fresh sleeper, so a limit one side sets is not the other's.
for args in '--nofile' '' '--nofile=50:60' '--nofile=:70 --verbose' \
            '--nofile=50: --stack'; do
  sh_case "sleep 30 & p=\$!; prlimit --pid \$p $args; rc=\$?; prlimit --pid \$p --nofile --stack; kill \$p; exit \$rc"
done
sh_case 'sleep 30 & p=$!; kill $p; wait $p; prlimit --pid $p --nofile'

# --- what the buffer loses, and closed or full descriptors -----------------------
for how in '>&-' '>/dev/full' '2>&-' '2>/dev/full'; do
  redir_case "$how" -h
  redir_case "$how" -V
  redir_case "$how"
  redir_case "$how" --bogus
  redir_case "$how" --nofile=abc
  redir_case "$how" --nofile sh -c 'echo out; echo err >&2'
  redir_case "$how" --nofile nosuchcommand
  redir_case "$how" --verbose --nofile=100:200
done

printf '%d passed, %d differed, %d broken, %d where only upstream hung\n' "$pass" "$fail" "$broken" "$hung"
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ]
