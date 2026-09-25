#!/usr/bin/env bash
# pathchk-diff.sh — compare our `pathchk` against GNU's, inside WSL.
#
# ## What this is checking
#
# `pathchk` reads the filesystem but never changes it, so one fixture serves
# every case on both sides: a file, an unsearchable directory, an ordinary
# one. The cases follow upstream's `validate_file_name`, which stops at the
# first problem with each name:
#
#   * **`-P`** -- a leading `-` in any component, and the empty name.
#   * **`-p`** -- the portable character set, reported one *character* at a
#     time (a UTF-8 `é` whole, a stray byte as `\377`), and POSIX's minimum
#     limits: 256 for the whole name, 14 per component.
#   * **no option** -- `lstat` first (`ENOENT` is fine; `EACCES`, `ENOTDIR`,
#     `ENAMETOOLONG` are reported), then `pathconf` limits, asked only for
#     names that do not exist and are long enough to need asking -- including
#     a component under a directory that does not exist, which inherits its
#     parent's limit.
#   * **the command line** -- `+pP`: the first name ends the options, so
#     `pathchk a --help` checks a file called `--help`.
#
# Runs unprivileged, so the unsearchable directory really is.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `--version` names SlateOS.
set -u

DIFF_PROG='pathchk'
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0
TO_FULL=

fx=$DIFF_TMP/fx
mkdir -p "$fx" && (
  cd "$fx" &&
  printf 'x\n' > f &&
  printf 'x\n' > 'sp ace' &&
  mkdir dir d &&
  chmod 000 d
) || { echo "pathchk-diff: could not build the fixture" >&2; exit 1; }

# `rep CHAR N`: N copies of CHAR.
rep() { printf "%${2}s" '' | tr ' ' "$1"; }
# `path N`: a name of exactly N bytes made of one-byte components, `a/a/.../a`.
path() {
  local n=$1 s=''
  while [ "${#s}" -lt "$((n - 1))" ]; do s="${s}a/"; done
  [ "${#s}" -lt "$n" ] && s="${s}a"
  printf '%s' "${s:0:$n}"
}

a14=$(rep a 14); a15=$(rep a 15); a20=$(rep a 20); a256=$(rep a 256)
p255=$(path 255); p256=$(path 256); p4095=$(path 4095); p4096=$(path 4096)

compare() {
  local side out err rc o_out g_out o_err g_err o_rc g_rc
  for side in ours gnu; do
    out=$DIFF_TMP/out-$side; err=$DIFF_TMP/err-$side
    if [ -n "$TO_FULL" ]; then
      ( cd "$fx" && timeout -k 2 30 env PATH="$bindir/$side" pathchk "$@" ) >/dev/full 2>"$err"
    else
      ( cd "$fx" && timeout -k 2 30 env PATH="$bindir/$side" pathchk "$@" ) >"$out" 2>"$err"
    fi
    rc=$?
    : >>"$out"
    if [ "$side" = ours ]; then
      o_rc=$rc; o_out=$(od -An -c <"$out"); o_err=$(cat "$err")
    else
      g_rc=$rc; g_out=$(od -An -c <"$out"); g_err=$(cat "$err")
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
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_err" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_err" | tr '\n' '|')")
}

# Labels are cut short: some names here are four thousand bytes long.
label_of() {
  local l="pathchk $*"
  [ "${#l}" -gt 90 ] && l="${l:0:87}..."
  printf '%s' "$l${TO_FULL:+  [>/dev/full]}"
}

run_case() {
  local label; label=$(label_of "$@")
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
  local label; label=$(label_of "$@")
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

# --- no option: lstat, then the limits it needs ----------------------------------
run_case a
run_case f
run_case dir
run_case /
run_case //
run_case .
run_case ..
run_case ./a
run_case a/b/c
run_case /nonexistent/x
run_case ''
run_case f/x
run_case f/
run_case 'sp ace/x'
run_case d/x
run_case d
run_case "$a20"
run_case "nodir/$a20"
run_case "nodir/$a20/$a20"
run_case "dir/$a20"
run_case "$a256"
run_case "nodir/$a256"
run_case "$p255"
run_case "$p256"
run_case "$p4095"
run_case "$p4096"
run_case "/$p4095"
run_case "$a14"

# --- -p: portable characters and POSIX's minimum limits --------------------------
run_case -p a
run_case -p ''
run_case -p 'a b'
run_case -p 'xé'
run_case -p "$(printf 'x\377y')"
run_case -p "$(printf 'a\nb')"
run_case -p 'a:b'
run_case -p '~'
run_case -p 'ok_Name.txt-1/sub.d'
run_case -p "$a14"
run_case -p "$a15"
run_case -p "dir/$a15"
run_case -p "$p255"
run_case -p "$p256"
run_case -p "/$p255"
run_case -p f/x
run_case -p d/x

# --- -P: empty names and leading dashes ------------------------------------------
run_case -P ''
run_case -P -- -a
run_case -P -- -
run_case -P a/-b
run_case -P /-
run_case -P a-b
run_case -P a/b-
run_case -P "$a20"
run_case --portability ''
run_case --portability -- '-a b'
run_case --portability 'a b'
run_case --portability "$a15"
run_case -pP -- -a
run_case -Pp ''

# --- several names: each is reported, and the run carries on ----------------------
run_case a 'b c' f/x d/x
run_case -p a 'b c' ok "$a15"
run_case -P '' a --

# --- the command line --------------------------------------------------------------
run_case
run_case -p
run_case -P
run_case --portability
run_case -x a
run_case --nope a
run_case --port 'a b'
run_case --p 'a b'
run_case a -p
run_case a --help
run_case a --nope
run_case -- -p
run_case --help=x
run_case --portability=x a
xfail_case 'our --help omits the GNU ancillary block' --help
xfail_case 'our --help omits the GNU ancillary block' --he
xfail_case 'our --help omits the GNU ancillary block' -p --help
xfail_case 'our --version names SlateOS' --version

# --- nothing to write, so nothing to fail --------------------------------------------
TO_FULL=1; run_case a
TO_FULL=1; run_case -p 'a b'

chmod -R u+rwx "$fx" 2>/dev/null

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)\n' "$xpass"
  exit 1
fi
printf '\n'
[ "$fail" -eq 0 ]
