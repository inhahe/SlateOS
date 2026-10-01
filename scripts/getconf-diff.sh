#!/usr/bin/env bash
# getconf-diff.sh — compare our `getconf` against glibc's, inside WSL.
#
# ## What this is checking
#
# `userspace/getconf` is a port of glibc 2.39's `posix/getconf.c` whose
# answers come from the C library's `sysconf`, `pathconf` and `confstr`.
# Built here for Linux, ours calls the same glibc the reference calls, so
# every VALUE is compared, not only the shape: all 320 names of `-a`, the
# limits of real paths, the strings (`PATH`, `GNU_LIBC_VERSION`), the two
# all-ones limits printed unsigned, a path that does not exist, a name that
# does not, and the command line around them (`-v SPEC`, `--`, `_POSIX_`
# left off, usage).
#
# One value moves while the two run: `_AVPHYS_PAGES`, the free memory. Its
# digits are masked in both outputs before the comparison, and only there.
#
# ## Cases that differ on purpose
#
# `--version` prints upstream's package string, `(GNU libc) 2.39`, where
# Ubuntu's build says `(Ubuntu GLIBC 2.39-0ubuntu8.N)`; `--help` names
# upstream's bug-report address where Ubuntu's names Launchpad.
set -u

DIFF_PROG='getconf'
DIFF_PKG='getconf'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0
TO_FULL=

# The free-memory line's digits, which change between the two runs.
mask() { sed -E 's/^(_AVPHYS_PAGES +)[0-9]+$/\1N/'; }

compare() {
  local side out err rc o_out g_out o_err g_err o_rc g_rc
  for side in ours gnu; do
    out=$DIFF_TMP/out-$side; err=$DIFF_TMP/err-$side
    if [ -n "$TO_FULL" ]; then
      ( cd "$DIFF_TMP" && timeout -k 2 30 env PATH="$bindir/$side" getconf "$@" ) >/dev/full 2>"$err"
    else
      ( cd "$DIFF_TMP" && timeout -k 2 30 env PATH="$bindir/$side" getconf "$@" ) >"$out" 2>"$err"
    fi
    rc=$?
    : >>"$out"
    if [ "$side" = ours ]; then
      o_rc=$rc; o_out=$(mask <"$out" | od -An -c); o_err=$(cat "$err")
    else
      g_rc=$rc; g_out=$(mask <"$out" | od -An -c); g_err=$(cat "$err")
    fi
    : >"$out"
  done
  TO_FULL=
  if [ "$o_rc" = "$g_rc" ] && [ "$o_out" = "$g_out" ] && [ "$o_err" = "$g_err" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out{%s} err{%s}\n  gnu  (rc=%s): out{%s} err{%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ' | cut -c1-600)" "$(printf '%s' "$o_err" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ' | cut -c1-600)" "$(printf '%s' "$g_err" | tr '\n' '|')")
}

run_case() {
  local label="getconf $*${TO_FULL:+  [>/dev/full]}"
  compare "$@"
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

xfail_case() {
  local why=$1; shift
  local label="getconf $*"
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$label" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$label" "$why"
  fi
  return 0
}

# --- every name at once ----------------------------------------------------------
run_case -a
run_case -a /
run_case -a /tmp
run_case -a /proc
run_case -a /no/such/path
run_case -a / extra

# --- sysconf -------------------------------------------------------------------------
for v in LONG_BIT WORD_BIT PAGESIZE PAGE_SIZE ARG_MAX CHILD_MAX CLK_TCK OPEN_MAX \
         NGROUPS_MAX _NPROCESSORS_CONF _NPROCESSORS_ONLN _PHYS_PAGES HOST_NAME_MAX \
         LOGIN_NAME_MAX LINE_MAX RE_DUP_MAX SSIZE_MAX INT_MAX INT_MIN CHAR_BIT \
         UINT_MAX ULONG_MAX USHRT_MAX _POSIX_VERSION POSIX_VERSION XOPEN_VERSION \
         _XOPEN_VERSION POSIX2_VERSION _POSIX_THREADS IOV_MAX SYMLOOP_MAX \
         PTHREAD_STACK_MIN NZERO TZNAME_MAX; do
  run_case "$v"
done

# --- confstr ---------------------------------------------------------------------------
for v in PATH CS_PATH GNU_LIBC_VERSION GNU_LIBPTHREAD_VERSION \
         POSIX_V7_LP64_OFF64_CFLAGS POSIX_V7_LP64_OFF64_LDFLAGS POSIX_V7_LP64_OFF64_LIBS \
         POSIX_V7_WIDTH_RESTRICTED_ENVS V7_ENV LFS_CFLAGS LFS64_LDFLAGS; do
  run_case "$v"
done

# --- pathconf ----------------------------------------------------------------------------
for p in / /tmp /proc /dev/null .; do
  for v in NAME_MAX PATH_MAX LINK_MAX PIPE_BUF FILESIZEBITS SYMLINK_MAX \
           _POSIX_CHOWN_RESTRICTED _POSIX_NO_TRUNC _POSIX_VDISABLE MAX_CANON; do
    run_case "$v" "$p"
  done
done
run_case NAME_MAX /no/such/path
run_case NAME_MAX ''
run_case NAME_MAX

# --- the command line --------------------------------------------------------------------
run_case
run_case NO_SUCH_VARIABLE
run_case ''
run_case LONG_BIT extra
run_case PATH extra
run_case LONG_BIT / extra
run_case -- LONG_BIT
run_case -- -a
run_case -v POSIX_V7_LP64_OFF64 LONG_BIT
run_case -vPOSIX_V7_LP64_OFF64 LONG_BIT
run_case -v anything LONG_BIT
run_case -v
run_case -v X
run_case -a -v X
run_case -x
run_case --nope
xfail_case 'upstream bug-report address; Ubuntu names Launchpad' --help extra-after-help
run_case LONG_BIT --help
xfail_case 'upstream package string; Ubuntu names its build' --version
xfail_case 'upstream bug-report address; Ubuntu names Launchpad' --help

# --- write errors ----------------------------------------------------------------------------
TO_FULL=1; run_case -a
TO_FULL=1; run_case LONG_BIT

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)\n' "$xpass"
  exit 1
fi
printf '\n'
[ "$fail" -eq 0 ]
