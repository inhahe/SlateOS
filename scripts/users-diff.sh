#!/usr/bin/env bash
# users-diff.sh — compare our `users` against GNU's, inside WSL.
#
# ## What this is checking
#
# `users FILE` is a pure function of FILE's records, so most cases hand both
# sides the same utmp file, built here in glibc's x86_64 `struct utmp` layout
# (384 bytes a record -- the layout `utmpfile` mirrors):
#
#   * **which records count** -- `USER_PROCESS` with a name; not `LOGIN`,
#     `DEAD`, `BOOT_TIME`, `RUN_LVL`, or a user record with an empty name.
#   * **the list** -- sorted by byte value (`Bob` before `alice`), one word per
#     session so a user logged in twice appears twice, trailing spaces trimmed
#     and nothing else, a 32-byte name with no NUL, names that are not UTF-8.
#   * **the file** -- empty, and torn (a partial record at the end is
#     dropped). Missing, unreadable, a directory: see below.
#   * **the live files** -- no operand (`/var/run/utmp`, dead sessions
#     dropped), `/var/run/utmp` named (dead sessions kept), `/var/log/wtmp`.
#   * **the command line** -- one FILE at most, `--`, unknown options.
#
# What this cannot reach is the dead-session check itself with a *chosen*
# pid: the live utmp is root's to write. `users.rs`'s unit tests pin that
# rule; here the live file only shows that both sides agree on it.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `--version` names SlateOS.
#
# A FILE that is missing, unreadable or a directory: glibc's `getutxent`
# cannot report the failure, so GNU on Linux prints nothing and exits 0 --
# "nobody is logged in". gnulib's own reader, used where GNU reads the file
# itself as ours does, reports `users: FILE: <reason>` and exits 1; ours does
# too (see `users.rs`).
set -u

DIFF_PROG='users'
DIFF_GNU_SOURCE=9.4
DIFF_NEED='python3'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0
TO_FULL=

fx=$DIFF_TMP/fx
mkdir -p "$fx" || exit 1
cat > "$DIFF_TMP/mkutmp.py" <<'PY'
import os, struct, sys

# glibc x86_64 `struct utmp`: type, padding, pid, line[32], id[4], user[32],
# host[256], exit (two shorts), session, tv_sec, tv_usec, addr_v6[4],
# reserved[20] -- 384 bytes.
FMT = "<hhi32s4s32s256shhiii4i20s"
assert struct.calcsize(FMT) == 384
EMPTY, RUN_LVL, BOOT_TIME, LOGIN, USER, DEAD = 0, 1, 2, 6, 7, 8

def rec(kind, user, pid=1234, line=b"pts/0", host=b""):
    return struct.pack(FMT, kind, 0, pid, line, b"ts/0", user, host,
                       0, 0, 0, 1700000000, 0, 0, 0, 0, 0, b"")

out = sys.argv[1]
def put(name, data):
    with open(os.path.join(out, name), "wb") as f:
        f.write(data)

put("empty", b"")
put("one", rec(USER, b"alice"))
put("mixed", b"".join([
    rec(BOOT_TIME, b"reboot", 0, b"~"),
    rec(RUN_LVL, b"runlevel", 0, b"~"),
    rec(LOGIN, b"LOGIN", 50, b"tty1"),
    rec(USER, b"zoe", 51, b"tty2"),
    rec(USER, b"alice", 52, b"pts/1"),
    rec(DEAD, b"gone", 53, b"pts/2"),
    rec(USER, b"Bob", 54, b"pts/3"),
    rec(USER, b"", 55, b"pts/4"),
    rec(USER, b"alice", 56, b"pts/5"),
    rec(EMPTY, b"", 0, b""),
]))
put("nobody", rec(LOGIN, b"LOGIN", 1, b"tty1") + rec(DEAD, b"x", 2))
put("spaces", rec(USER, b"a  ") + rec(USER, b" b") + rec(USER, b"c d") + rec(USER, b"   "))
put("full32", rec(USER, b"u" * 32) + rec(USER, b"v" * 31))
put("bytes", rec(USER, b"caf\xe9") + rec(USER, b"\xff") + rec(USER, "é".encode()))
put("torn", rec(USER, b"x") + rec(USER, b"y") + rec(USER, b"z")[:100])
put("deadpids", rec(USER, b"ghost", 999999) + rec(USER, b"init", 1) + rec(USER, b"nopid", 0))
put("big", b"".join(rec(USER, b"u%03d" % (i % 37), i + 10) for i in range(500)))
PY
python3 "$DIFF_TMP/mkutmp.py" "$fx" || { echo "users-diff: could not build the fixtures" >&2; exit 1; }
printf 'x\n' > "$fx/unreadable" && chmod 000 "$fx/unreadable"
mkdir "$fx/dir"

compare() {
  local side out err rc o_out g_out o_err g_err o_rc g_rc
  for side in ours gnu; do
    out=$DIFF_TMP/out-$side; err=$DIFF_TMP/err-$side
    if [ -n "$TO_FULL" ]; then
      ( cd "$fx" && timeout -k 2 30 env PATH="$bindir/$side" users "$@" ) >/dev/full 2>"$err"
    else
      ( cd "$fx" && timeout -k 2 30 env PATH="$bindir/$side" users "$@" ) >"$out" 2>"$err"
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

run_case() {
  local label="users $*${TO_FULL:+  [>/dev/full]}"
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
  local label="users $*"
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

# --- which records count, and how the list is made --------------------------------
run_case one
run_case mixed
run_case nobody
run_case spaces
run_case full32
run_case bytes
run_case torn
run_case deadpids
run_case big

# --- files that are not a list of sessions -------------------------------------------
run_case empty
xfail_case 'glibc hides the failure; gnulib and ours report it' missing
xfail_case 'glibc hides the failure; gnulib and ours report it' unreadable
xfail_case 'glibc hides the failure; gnulib and ours report it' dir
xfail_case 'glibc hides the failure; gnulib and ours report it' ''

# --- the live files ------------------------------------------------------------------
run_case
run_case /var/run/utmp
run_case /var/log/wtmp
run_case --

# --- the command line ----------------------------------------------------------------
run_case one mixed
run_case one mixed big
run_case -- one
xfail_case 'glibc hides the failure; gnulib and ours report it' -- -x
run_case -x
run_case --nope
run_case one --nope
run_case --help=1
run_case --ver=1
xfail_case 'our --help omits the GNU ancillary block' --help
xfail_case 'our --help omits the GNU ancillary block' one --help
xfail_case 'our --help omits the GNU ancillary block' --he
xfail_case 'our --version names SlateOS' --version

# --- write errors ---------------------------------------------------------------------
TO_FULL=1; run_case mixed
TO_FULL=1; run_case empty

chmod -R u+rwx "$fx" 2>/dev/null

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)\n' "$xpass"
  exit 1
fi
printf '\n'
[ "$fail" -eq 0 ]
