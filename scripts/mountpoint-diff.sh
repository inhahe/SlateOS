#!/usr/bin/env bash
# Differential test: our `mountpoint` against util-linux 2.39.3's.
#
# mountpoint looks a path up in /proc/self/mountinfo as libmount looks up a
# mount point -- as given, made absolute, canonicalized -- so what is
# compared is how each side treats the paths of this machine: real mount
# points and directories that are not, a file bind-mounted on its own
# (WSL's /init), relative paths, symlinks to mount points with and without
# --nofollow, trailing slashes and dot-dot, block and character devices for
# -x, and paths that do not exist; each with -q, -d and -x.
#
# What is compared: stdout, stderr and the exit status of each case (32 is
# "not a mount point"), and the same under closed and full descriptors.
set -u

DIFF_PROG='mountpoint'
DIFF_PKG='mountpoint'
DIFF_NEED='timeout'
DIFF_REF=/usr/bin/mountpoint
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; broken=0

cd "$DIFF_TMP" || exit 1
ln -s /proc link-to-proc
ln -s /nonexistent dangling
ln -s link-to-proc link-to-link
mkdir -p plain/dir
: > plain/file

# $1 = side, rest = argv.
run_side() {
  local side=$1; shift
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/$side:$PATH" timeout -k 2 20 mountpoint "$@"
}

judge() {
  local o_rc=$1 g_rc=$2
  if [ "$o_rc" = 127 ] && [ ! -s "$DIFF_TMP/o.err" ] || [ "$o_rc" = 124 ] || [ "$g_rc" = 124 ]; then
    AGREED=broken
  elif cmp -s "$DIFF_TMP/o.out" "$DIFF_TMP/g.out" \
     && cmp -s "$DIFF_TMP/o.err" "$DIFF_TMP/g.err" && [ "$o_rc" = "$g_rc" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out %q err %q\n  gnu  (rc=%s): out %q err %q' \
    "$o_rc" "$(cat "$DIFF_TMP/o.out")" "$(cat "$DIFF_TMP/o.err")" \
    "$g_rc" "$(cat "$DIFF_TMP/g.out")" "$(cat "$DIFF_TMP/g.err")")
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

both() {
  local o_rc g_rc
  run_side ours "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side gnu "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "mountpoint $(printf '%q ' "$@" | sed "s|$DIFF_TMP/||g")"
}

redir_case() {
  local how=$1 o_rc g_rc; shift
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/ours:$PATH" timeout -k 2 20 \
    bash -c "exec mountpoint \"\$@\" $how" sh "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/gnu:$PATH" timeout -k 2 20 \
    bash -c "exec mountpoint \"\$@\" $how" sh "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "mountpoint $(printf '%q ' "$@")$how"
}

# --- options and their refusals ---------------------------------------------------
for args in '' '-h' '--help' '-V' '--version' '--vers' '--bogus' '-z' '--he' '--no' \
            '/ /proc' '-q' '-x' '-d' '--nofollow' '-q -h' '-h -q' \
            '--nofollow -x /dev/null' '-x --nofollow /' '--devno --nofollow /proc' \
            '-- /' '-- -q' '-qd /' '-dq /proc' '-qx /dev/null'; do
  eval "set -- $args"
  both "$@"
done

# --- paths ----------------------------------------------------------------------------
devices=
for d in /dev/sda /dev/sdb /dev/sdc /dev/sdd /dev/loop0; do
  [ -b "$d" ] && devices="$devices $d"
done
for p in / /proc /proc/ /proc/. /proc/.. //proc /sys /sys/fs/cgroup /dev /dev/pts /run /tmp \
         /etc /etc/ /usr /home /init /etc/hostname /etc/resolv.conf /nonexistent '' . .. \
         link-to-proc link-to-link dangling plain plain/dir plain/file \
         /dev/null /dev/zero /dev/tty $devices; do
  for flags in '' '-q' '-d' '-x' '-q -d' '-q -x' '--nofollow' '--nofollow -d' '--nofollow -q'; do
    # shellcheck disable=SC2086 # each entry is words
    both $flags "$p"
  done
done
# Relative to /, in this shell so the counts are kept.
cd / || exit 1
for p in proc sys etc . ..; do
  both "$p"
  both -d "$p"
done
cd "$DIFF_TMP" || exit 1

# --- closed and full descriptors -------------------------------------------------
for how in '>&-' '>/dev/full' '2>&-' '2>/dev/full'; do
  redir_case "$how" /
  redir_case "$how" /etc
  redir_case "$how" -d /proc
  redir_case "$how" -x /dev/null
  redir_case "$how" /nonexistent
  redir_case "$how" -h
  redir_case "$how" -V
  redir_case "$how" --bogus
done

printf '%d passed, %d differed, %d broken\n' "$pass" "$fail" "$broken"
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ]
