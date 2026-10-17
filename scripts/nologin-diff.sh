#!/usr/bin/env bash
# Differential test: our `nologin` against Ubuntu's, shadow 4.13's.
#
# nologin's output is one line and a status; what else it does is write a
# line to the system log. So each case runs in a user and mount namespace of
# its own (`unshare -Urm`, no root) whose /dev is a tmpfs holding a syslog
# socket the harness listens on, and the line each side logged is compared
# too, its timestamp masked. /dev/pts is carried into the namespace, so that
# a case run on a pty has a terminal for `ttyname` to name.
#
# What is compared: stdout, stderr, the exit status and the logged line --
# with no arguments and with any (`--help` among them: shadow's takes none),
# with and without SSH_ORIGINAL_COMMAND, on a pty and not, with no log to
# write to, and with standard output closed and full.
set -u

DIFF_PROG='nologin'
DIFF_PKG='nologin'
DIFF_REF='/usr/sbin/nologin'
DIFF_NEED='timeout unshare mount script python3'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; broken=0

diff_devlog

# inner.sh BIN OUT ERR LOG HOW [nolog] ARGV...: one case, in the namespace.
cat > "$DIFF_TMP/inner.sh" <<'EOF'
set -u
here=$(dirname "$0")
. "$here/devlog.sh"
bin=$1 out=$2 err=$3 log=$4 how=$5; shift 5
# The namespace came up: a side without this file compared nothing.
: > "${out%.out}.ran"
nolog=
if [ "${1:-}" = nolog ]; then nolog=1; shift; fi
mkdir -p "$here/pts.$$" && mount --bind /dev/pts "$here/pts.$$" || exit 90
mount -t tmpfs tmpfs /dev || exit 91
mkdir /dev/pts && mount --bind "$here/pts.$$" /dev/pts || exit 92
# Rust's runtime opens /dev/null for a closed standard descriptor; the
# device cannot be had here, so a file.
: > /dev/null && chmod 666 /dev/null
: > "$log"
[ -n "$nolog" ] || devlog_start "$log"
run() { timeout -k 1 5 "$bin" "$@"; }
case $how in
  plain) run "$@" > "$out" 2> "$err" < "$0" ;;
  pty) run "$@" 2> "$err" ;;
  closed) run "$@" >&- 2> "$err" < "$0" ;;
  full) run "$@" >&5 2> "$err" < "$0" ;;
esac
rc=$?
[ -n "$nolog" ] || devlog_stop
exit "$rc"
EOF

# side SIDE HOW ARGV...: one side's out, err, log and status.
side() {
  local s=$1 how=$2 o=$DIFF_TMP/$1; shift 2
  local bin=$bindir/$s/nologin
  case $how in
    pty)
      diff_run env -i PATH=/usr/bin:/bin ${CASE_ENV:+"$CASE_ENV"} timeout -k 1 15 \
        script -qec "$(printf '%q ' unshare -Urm bash "$DIFF_TMP/inner.sh" "$bin" "$o.out" "$o.err" "$o.log" pty "$@")" /dev/null \
        < /dev/null > "$o.out"
      ;;
    full)
      diff_run env -i PATH=/usr/bin:/bin ${CASE_ENV:+"$CASE_ENV"} timeout -k 1 15 \
        unshare -Urm bash "$DIFF_TMP/inner.sh" "$bin" "$o.out" "$o.err" "$o.log" full "$@" 5>/dev/full
      ;;
    *)
      diff_run env -i PATH=/usr/bin:/bin ${CASE_ENV:+"$CASE_ENV"} timeout -k 1 15 \
        unshare -Urm bash "$DIFF_TMP/inner.sh" "$bin" "$o.out" "$o.err" "$o.log" "$how" "$@"
      ;;
  esac
}

case_() {
  local how=$1 o_rc g_rc label; shift
  rm -f "$DIFF_TMP"/ours.* "$DIFF_TMP"/gnu.*
  side ours "$how" "$@"; o_rc=$?
  side gnu "$how" "$@"; g_rc=$?
  for s in ours gnu; do for f in out err log; do [ -f "$DIFF_TMP/$s.$f" ] || : > "$DIFF_TMP/$s.$f"; done; done
  diff_mask_logged_time "$DIFF_TMP/ours.log" "$DIFF_TMP/gnu.log"
  label="[$how${CASE_ENV:+ $CASE_ENV}] nologin $(printf '%q ' "$@")"
  if [ ! -f "$DIFF_TMP/ours.ran" ] || [ ! -f "$DIFF_TMP/gnu.ran" ] \
     || { [ "$o_rc" -ge 90 ] && [ "$o_rc" -le 93 ]; } || { [ "$g_rc" -ge 90 ] && [ "$g_rc" -le 93 ]; } \
     || [ "$g_rc" = 127 ] || [ "$o_rc" = 127 ]; then
    broken=$((broken + 1)); printf 'BROKEN %s (rc %s/%s)\n' "$label" "$o_rc" "$g_rc"
  elif cmp -s "$DIFF_TMP/ours.out" "$DIFF_TMP/gnu.out" && cmp -s "$DIFF_TMP/ours.err" "$DIFF_TMP/gnu.err" \
     && cmp -s "$DIFF_TMP/ours.log" "$DIFF_TMP/gnu.log" && [ "$o_rc" = "$g_rc" ]; then
    pass=$((pass + 1)); [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail + 1))
    printf 'DIFF %s\n  ours (rc=%s): out %q err %q log %q\n  gnu  (rc=%s): out %q err %q log %q\n' "$label" \
      "$o_rc" "$(cat "$DIFF_TMP/ours.out")" "$(cat "$DIFF_TMP/ours.err")" "$(cat "$DIFF_TMP/ours.log")" \
      "$g_rc" "$(cat "$DIFF_TMP/gnu.out")" "$(cat "$DIFF_TMP/gnu.err")" "$(cat "$DIFF_TMP/gnu.log")"
    if [ -n "${DIFF_FAIL_FAST:-}" ]; then exit 1; fi
  fi
  return 0
}

for args in '' '--help' '-h' '--version' '-V' 'x y z' '--' '-c true' "$(printf 'a\377b')"; do
  # shellcheck disable=SC2086
  case_ plain $args
done
CASE_ENV='SSH_ORIGINAL_COMMAND=ls -l /etc' case_ plain
CASE_ENV='SSH_ORIGINAL_COMMAND=' case_ plain
CASE_ENV="SSH_ORIGINAL_COMMAND=$(printf 'a%%sb\377')" case_ plain
CASE_ENV='LOGNAME=someone' case_ plain
case_ pty
case_ pty --help
CASE_ENV='SSH_ORIGINAL_COMMAND=uptime' case_ pty
case_ closed
case_ full
case_ plain nolog
case_ pty nolog

printf '\nnologin-diff: %d passed, %d differed, %d broken\n' "$pass" "$fail" "$broken"
[ "$fail" = 0 ] && [ "$broken" = 0 ]
