#!/usr/bin/env bash
# Differential test: our `lockfile` against procmail 3.24's, as Ubuntu builds it.
#
# What is compared: stdout, stderr and the exit status of every case, and --
# for the cases that lock -- the files left behind: each name, its mode, its
# link count and its contents. That last part is the point: a lockfile that
# printed the right words and left a lock with the wrong mode, an extra link or
# no `0` in it would pass a harness that only read the output.
#
# The cases: no arguments, -h, -v and every usage refusal (including the
# second pass that prints the usage line twice); a lock taken; a lock held by
# someone else, with retries (at -0, so no case waits), with -! before and
# after the failing argument, and the second pass releasing the locks already
# taken; -l forcing a lock older than the timeout and not forcing a younger
# one; a missing directory; a name too long for the filesystem, cut a byte at
# a time; a trailing slash; -ml and -mu (WSL's /var/mail is not writable, so
# both fail -- identically, which is what is compared); and a signal that
# ends the wait.
#
# The reference is not installed (that needs root): this fetches Ubuntu's
# procmail package with `apt-get download`, which apt verifies, and unpacks it
# into a cache. procmail 3.24's own build would do as well; the package is what
# a user of the distribution runs.
set -u

DIFF_PROG='lockfile'
DIFF_PKG='lockfile'
DIFF_NEED='timeout apt-get dpkg-deb stat'
# The reference is fetched below, after the preamble, and so are the PATH
# directories: the preamble re-execs this file inside WSL, so nothing may run
# before it, and it cannot look for a binary that is not fetched yet.
DIFF_NO_REF=1
DIFF_NO_BINDIR=1
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

PROCMAIL_VERSION='3.24-1ubuntu2'
ref_cache=$HOME/.cache/slateos-diff-procmail/$PROCMAIL_VERSION
if [ ! -x "$ref_cache/root/usr/bin/lockfile" ]; then
  mkdir -p "$ref_cache" &&
    (cd "$ref_cache" && apt-get download "procmail=$PROCMAIL_VERSION" >/dev/null 2>&1 &&
      dpkg-deb -x procmail_*.deb root) ||
    { echo "lockfile-diff: cannot fetch procmail $PROCMAIL_VERSION" >&2; exit 1; }
fi
gnu_real=$ref_cache/root/usr/bin/lockfile
mkdir -p "$bindir/ours" "$bindir/gnu"
ln -s "$OURS" "$bindir/ours/lockfile" && ln -s "$gnu_real" "$bindir/gnu/lockfile" || exit 1

pass=0; fail=0; broken=0

# Each side works in a directory of its own, prepared identically, so the files
# left behind can be compared. $1 = side.
work() { printf '%s/%s' "$DIFF_TMP" "$1"; }

# $1 = side, rest = argv. Run from that side's directory, with only that
# side's lockfile on PATH.
run_side() {
  local side=$1; shift
  (cd "$(work "$side")" &&
    diff_run env LC_ALL=C.UTF-8 PATH="$bindir/$side:$PATH" \
      timeout -k 2 30 lockfile "$@")
}

# What a directory holds, one level deep: each name (quoted, since one case
# uses a newline), its mode, its link count and its contents, sorted. The
# temporaries lockfile makes beside a lock must all be gone, so any that
# survive are listed too.
inventory() {
  (cd "$1" && find . -mindepth 1 -maxdepth 2 -print0 | sort -z |
    while IFS= read -r -d '' f; do
      if [ -d "$f" ]; then
        printf '%q/ %s\n' "$f" "$(stat -c '%a' "$f")"
      else
        printf '%q %s %q\n' "$f" "$(stat -c '%a %h' "$f")" "$(cat "$f")"
      fi
    done)
}

judge() {
  local o_rc=$1 g_rc=$2
  if [ "$o_rc" = 127 ] || [ "$g_rc" = 127 ] || [ "$o_rc" = 124 ] || [ "$g_rc" = 124 ]; then
    AGREED=broken
  elif cmp -s "$DIFF_TMP/o.out" "$DIFF_TMP/g.out" && cmp -s "$DIFF_TMP/o.err" "$DIFF_TMP/g.err" &&
       [ "$o_rc" = "$g_rc" ] && [ "$(inventory "$(work ours)")" = "$(inventory "$(work gnu)")" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out %q err %q\n    files: %q\n  gnu  (rc=%s): out %q err %q\n    files: %q' \
    "$o_rc" "$(cat "$DIFF_TMP/o.out")" "$(cat "$DIFF_TMP/o.err")" "$(inventory "$(work ours)")" \
    "$g_rc" "$(cat "$DIFF_TMP/g.out")" "$(cat "$DIFF_TMP/g.err")" "$(inventory "$(work gnu)")")
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

# setup_ SCRIPT: empty both directories and run SCRIPT in each.
setup_() {
  local side
  for side in ours gnu; do
    chmod -R u+w "$(work "$side")" 2>/dev/null
    rm -rf "$(work "$side")"
    mkdir -p "$(work "$side")"
    (cd "$(work "$side")" && eval "$1")
  done
}

# case_ ARGV...: run both sides from freshly set-up directories.
case_() {
  local o_rc g_rc
  run_side ours "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side gnu "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "lockfile $*"
}

# A lock held by someone else, written as lockfile writes one.
HELD='printf 0 > held.lock && chmod 444 held.lock'
OLD='printf 0 > old.lock && chmod 444 old.lock && touch -d "2000-01-01" old.lock'
YOUNG='printf 0 > young.lock && chmod 444 young.lock'

# --- the command line --------------------------------------------------------
setup_ ':'
case_
case_ -h
case_ '-?'
case_ -v
case_ -x
case_ -x a.lock
case_ -r x
case_ -s -1
case_ -r
case_ -l
case_ -s
case_ -5x a.lock
case_ --
case_ -
case_ -5
case_ -!-r0 a.lock
case_ -m
case_ -mx
case_ -mlx
case_ -! -r x
case_ -h a.lock
case_ -v a.lock

# --- taking a lock -----------------------------------------------------------
setup_ ':'; case_ a.lock
setup_ ':'; case_ -0 a.lock b.lock
setup_ ':'; case_ -1 -r 3 -l 60 -s 0 a.lock
setup_ 'mkdir sub'; case_ sub/a.lock
setup_ ':'; case_ nodir/a.lock
setup_ 'mkdir sub'; case_ -r0 sub/

# --- a lock someone else holds -------------------------------------------------
setup_ "$HELD"; case_ -r0 held.lock
setup_ "$HELD"; case_ -0 -r 3 held.lock
setup_ "$HELD"; case_ -0 -r4294967296 held.lock
setup_ "$HELD"; case_ -! -r0 held.lock
setup_ "$HELD"; case_ -r0 held.lock -!
setup_ ':';     case_ -! a.lock
setup_ "$HELD"; case_ -r0 a.lock held.lock b.lock
setup_ "$HELD"; case_ -r0 a.lock -! held.lock
setup_ "$HELD"; case_ -r0 -ml held.lock

# --- -l: a stale lock ----------------------------------------------------------
setup_ "$OLD";   case_ -l 10 -s 0 -r 1 -0 old.lock
setup_ "$YOUNG"; case_ -l 3600 -r0 young.lock

# --- names ---------------------------------------------------------------------
LONG=$(printf 'a%.0s' $(seq 1 260))
setup_ ':'; case_ "$LONG"
setup_ ':'; case_ 'with space.lock' 'new
line.lock'

# --- the mailbox ---------------------------------------------------------------
# Only where /var/mail is NOT writable -- WSL's is root's -- so that both
# sides fail the same way and neither touches a real mailbox lock.
mailbox=/var/mail/$(id -un)
if [ -w /var/mail ] || [ -e "$mailbox.lock" ]; then
  printf 'SKIP the -ml/-mu cases: /var/mail is writable here, or %s exists\n' "$mailbox.lock"
else
  setup_ ':'; case_ -ml
  setup_ ':'; case_ -mu
  setup_ ':'; case_ -ml -mu
  # $LOGNAME naming nobody: the caller's own entry is used instead.
  logname_save=${LOGNAME:-}
  export LOGNAME=nosuchuser
  setup_ ':'; case_ -mu
  export LOGNAME="$logname_save"
fi

# --- a signal ends the wait ----------------------------------------------------
# TERM arrives during the 1-second sleep after the first attempt; the next
# attempt sees the flag. --preserve-status hands back lockfile's own status.
signal_case() {
  local o_rc g_rc side
  for side in ours gnu; do
    (cd "$(work "$side")" &&
      env LC_ALL=C.UTF-8 PATH="$bindir/$side:$PATH" \
        timeout --preserve-status -s TERM 0.5 lockfile -1 held.lock) \
      >"$DIFF_TMP/${side:0:1}.out" 2>"$DIFF_TMP/${side:0:1}.err"
    eval "${side:0:1}_rc=\$?"
  done
  judge "$o_rc" "$g_rc"
  report "lockfile -1 held.lock, then SIGTERM"
}
setup_ "$HELD"; signal_case

printf '%s: %d passed, %d differed, %d broken\n' "$DIFF_PROG" "$pass" "$fail" "$broken"
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ]
