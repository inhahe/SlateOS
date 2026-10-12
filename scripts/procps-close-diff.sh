#!/bin/bash
# procps-close-diff.sh -- how each procps program here closes its standard
# output, against procps-ng 4.0.4's own: `local/fileutils.c`'s `close_stdout`,
# which every one of them registers with `atexit`.
#
# ## Why a harness of its own
#
# procps' `close_stdout` is not gnulib's. It skips the report whenever `errno`
# is `EPIPE`, so a procps program whose `SIGPIPE` is ignored and whose reader
# has gone says nothing and keeps the status it had earned, where a GNU one
# says `write error: Broken pipe` and exits 1. Every procps port here
# inherited gnulib's from `coreutils::stdfd::close_stdout`, and none of their
# harnesses closes the reader under `trap '' PIPE`, so nothing noticed. This
# runs that one situation -- and its neighbours, a full disk and a closed
# descriptor, which the two agree on -- across the whole family at once.
#
# What the program writes is thrown away (the reader is gone, or the disk is
# full), so it may differ between the sides -- `ps` and `pgrep` list whatever
# is running -- and only standard error and the status are compared.
#
# ## The reference
#
# Ubuntu's procps 4.0.4-4ubuntu3.2. `sysctl` is in /usr/sbin there, which the
# preamble does not search, so this harness builds its own PATH directories.
set -u

DIFF_PROG='procps-close'
DIFF_BINS='free uptime vmstat w pgrep pkill sysctl ps pwdx'
DIFF_NO_BINDIR=1
DIFF_NO_REF=1
DIFF_NEED='timeout'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

bindir=$DIFF_TMP/bin
mkdir -p "$bindir/ours" "$bindir/gnu"
skipped=
for b in $DIFF_BINS; do
  ref=
  for cand in "/usr/bin/$b" "/bin/$b" "/usr/sbin/$b" "/sbin/$b"; do
    [ -x "$cand" ] && { ref=$cand; break; }
  done
  if [ -n "$OURS" ] && [ -d "$OURS" ]; then
    mine=$OURS/$b
  else
    mine=$(diff_ours "$b")
  fi
  if [ -z "$ref" ] || [ ! -x "$mine" ]; then
    skipped="$skipped $b"
    continue
  fi
  ln -s "$mine" "$bindir/ours/$b" || exit 1
  ln -s "$ref" "$bindir/gnu/$b" || exit 1
done

pass=0; fail=0

# A process for `pkill -0 -e` to name and `pwdx` to read: signal 0 tests
# that it may be signalled and sends nothing.
cp "$(command -v sleep)" "$DIFF_TMP/procpsclose"
"$DIFF_TMP/procpsclose" 600 &
sleeper=$!
diff_cleanup() {
  kill "$sleeper" 2>/dev/null
  chmod -R u+rwx "$DIFF_TMP" 2>/dev/null
  rm -rf "$DIFF_TMP"
  return 0
}

# run1 HOW ERR BINDIR PROG ARGS...: PROG with its standard output sent where
# HOW says, its standard error into ERR; exits with PROG's status.
cat >"$DIFF_TMP/run1" <<'EOF'
#!/bin/bash
how=$1 err=$2 dir=$3
shift 3
exec 2>"$err"
PATH=$dir:$PATH
case $how in
  epipe|sigpipe)
    if [ "$how" = epipe ]; then trap '' PIPE; else trap - PIPE; fi
    # The reader is gone before the first write.
    { sleep 0.3; exec "$@"; } | true
    exit "${PIPESTATUS[0]}"
    ;;
  full) exec "$@" >/dev/full ;;
  closed) exec "$@" >&- ;;
esac
EOF
chmod +x "$DIFF_TMP/run1"

# check HOW PROG ARGS...
check() {
  local how=$1 prog=$2 side
  shift 2
  if [ ! -e "$bindir/ours/$prog" ]; then
    return 0
  fi
  for side in ours gnu; do
    diff_run timeout -k 2 30 env LC_ALL=C.UTF-8 "$DIFF_TMP/run1" "$how" \
      "$DIFF_TMP/$side.err" "$bindir/$side" "$prog" "$@"
    printf '%s\n' "$?" >"$DIFF_TMP/$side.rc"
  done
  if cmp -s "$DIFF_TMP/ours.err" "$DIFF_TMP/gnu.err" \
     && cmp -s "$DIFF_TMP/ours.rc" "$DIFF_TMP/gnu.rc"; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s: %s %s\n' "$how" "$prog" "$*"
  else
    fail=$((fail+1))
    printf 'DIFF %s: %s %s\n' "$how" "$prog" "$*"
    for side in ours gnu; do
      printf -- '--- %s: status %s\n' "$side" "$(cat "$DIFF_TMP/$side.rc")"
      cat "$DIFF_TMP/$side.err"
    done
  fi
  return 0
}

for how in epipe sigpipe full closed; do
  check "$how" free
  check "$how" free --help
  check "$how" uptime
  check "$how" vmstat
  check "$how" w
  check "$how" pgrep -l procpsclose
  check "$how" pkill -0 -e -x procpsclose
  check "$how" sysctl kernel.ostype
  check "$how" ps -p "$sleeper"
  check "$how" pwdx "$sleeper"
  check "$how" pwdx "$sleeper" 2147483646
done

printf '\n%d passed, %d differed\n' "$pass" "$fail"
[ -n "$skipped" ] && printf 'skipped (no reference or no build):%s\n' "$skipped"
[ "$fail" = 0 ]
