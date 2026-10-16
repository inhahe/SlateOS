#!/usr/bin/env bash
# Differential test: our `last` and `lastb` against util-linux 2.39.3's.
#
# The reference is Ubuntu's build, in `util-linux`; `lastb` is the same
# program by another name, and both sides are reached by both names.
#
# What is compared: stdout, stderr and the exit status of each case.
#
#   * wtmp files written here, record by record: boots, shutdowns, run-level
#     changes (a NUL level among them), clock changes, logins matched with
#     logouts, logins cut short by a crash or a shutdown, logouts that came
#     before their login, the compatibility guesses for records with no
#     type, ghost entries, ignored and unknown types, `ftpNNN` and `uuNNN`
#     lines, names and hosts that fill their fields with no terminator,
#     control characters, UTF-8 and bytes that are not;
#   * a file of three chunks and more, so records straddle `uread`'s 16 KiB
#     reads; one that is not a whole number of records; one shorter than a
#     record; an empty file, a directory, /dev/null, /dev/zero, a pipe, a
#     missing and an unreadable file -- and two of them in one run, where
#     upstream's static read state carries from one to the next;
#   * logins as recent as this machine's boot, by users who exist and do not,
#     on ttys whose owner is and is not theirs: still logged in, or gone;
#   * the machine's own /var/log/wtmp, copied, and read where it is;
#   * every time format, in several zones -- a half-hour one west of
#     Greenwich among them, which util-linux writes `+00:30`;
#   * -n and -NUMBER (which add up), -R, -a, -w, -x, -i and -d on IPv4,
#     IPv6 and mapped addresses, -s/-t/-p in each form `parse_timestamp`
#     reads, names and ttys to show, and the command line's refusals;
#   * standard output and error closed or full.
#
# Cases that differ on purpose: --version and -V name this build.
set -u

DIFF_PROG='last'
DIFF_NEED='timeout python3'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

# `lastb` is `last` by another name, on both sides.
gnu_lastb=$(dirname "$gnu_real")/lastb
if [ ! -x "$gnu_lastb" ]; then
  echo "last-diff: no lastb beside $gnu_real" >&2
  exit 1
fi
ln -s "$OURS" "$bindir/ours/lastb" || exit 1
ln -s "$gnu_lastb" "$bindir/gnu/lastb" || exit 1

pass=0; fail=0; xfail=0; xpass=0; broken=0
fix=$DIFF_TMP/fix
mkdir -p "$fix"

python3 - "$fix" <<'PY'
import os, struct, sys, time
d = sys.argv[1]

EMPTY, RUN_LVL, BOOT_TIME, NEW_TIME, OLD_TIME = 0, 1, 2, 3, 4
INIT, LOGIN, USER, DEAD, ACCOUNTING = 5, 6, 7, 8, 9

def rec(ty, line=b"", user=b"", host=b"", sec=0, pid=0, uid=b"", addr=b"",
        usec=0, session=0):
    # glibc's x86-64 `struct utmpx`: 384 bytes.
    r = struct.pack("<hxxi32s4s32s256shhiii16s20x", ty, pid, line, uid, user,
                    host, 0, 0, session, sec, usec, addr.ljust(16, b"\0"))
    assert len(r) == 384
    return r

def v4(a, b, c, e):
    return bytes([a, b, c, e])

def put(name, data, mode=None):
    path = os.path.join(d, name)
    with open(path, "wb") as f:
        f.write(data)
    if mode is not None:
        os.chmod(path, mode)

t0 = 1700000000  # 2023-11-14 22:13:20 UTC
recs = [
    rec(BOOT_TIME, b"~", b"reboot", b"6.1.0-test", t0),
    rec(RUN_LVL, b"~", b"runlevel", b"6.1.0-test", t0 + 5, pid=ord("5") + 256 * ord("N")),
    rec(LOGIN, b"tty1", b"LOGIN", b"", t0 + 10, pid=900, uid=b"1"),
    rec(USER, b"tty1", b"alice", b"", t0 + 20, pid=1000, uid=b"1"),
    rec(USER, b"pts/0", b"bob", b"192.0.2.10", t0 + 30, pid=1001, uid=b"ts/0",
        addr=v4(192, 0, 2, 10)),
    rec(DEAD, b"pts/0", b"", b"", t0 + 30 + 3600 + 125, pid=1001),
    rec(USER, b"pts/1", b"carol", b"2001:db8::1", t0 + 100, pid=1002,
        addr=bytes.fromhex("20010db8000000000000000000000001")),
    rec(DEAD, b"pts/1", b"", b"", t0 + 100 + 2 * 86400 + 3 * 3600 + 7 * 60, pid=1002),
    rec(USER, b"ftp1234", b"ftpuser", b"198.51.100.7", t0 + 200, pid=1003,
        addr=bytes(10) + b"\xff\xff" + v4(198, 51, 100, 7)),
    rec(DEAD, b"ftp1234", b"", b"", t0 + 260, pid=1003),
    rec(USER, b"uucp7", b"uucp", b"", t0 + 300, pid=1004),
    rec(OLD_TIME, b"|", b"date", b"", t0 + 400),
    rec(NEW_TIME, b"{", b"date", b"", t0 + 4000),
    rec(EMPTY, b"tty2", b"dave", b"", t0 + 5000, pid=1005),
    rec(USER, b"tty3", b"", b"ghost", t0 + 5100, pid=1006),
    rec(ACCOUNTING, b"acct", b"acct", b"", t0 + 5200),
    rec(42, b"what", b"who", b"", t0 + 5300),
    rec(-3, b"neg", b"neg", b"", t0 + 5301),
    rec(USER, b"tty4", b"erin", b"", t0 + 5400, pid=1007),
    rec(RUN_LVL, b"~", b"runlevel", b"", t0 + 6000, pid=ord("0") + 256 * ord("5")),
    rec(EMPTY, b"~", b"shutdown", b"6.1.0-test", t0 + 6001),
    rec(BOOT_TIME, b"~", b"reboot", b"6.1.0-test", t0 + 7000),
    rec(USER, b"tty1", b"alice", b"", t0 + 7100, pid=1100),
    rec(USER, b"pts/2", b"ev\x01l", b"h\x1b[31mred", t0 + 7200, pid=1101),
    rec(DEAD, b"pts/2", b"", b"", t0 + 7260, pid=1101),
    rec(USER, b"pts/3", b"jos\xc3\xa9", b"caf\xe9\xc2\x85\xe2\x80\xa8x", t0 + 7300, pid=1102),
    rec(DEAD, b"pts/3", b"", b"", t0 + 7359, pid=1102),
    rec(USER, b"pts/abcdefghijklmnopqrstuvwxyz12", b"abcdefghijklmnopqrstuvwxyz012345",
        b"h" * 256, t0 + 7400, pid=1103),
    rec(DEAD, b"pts/abcdefghijklmnopqrstuvwxyz12", b"", b"", t0 + 7400 + 5 * 3600, pid=1103),
    rec(USER, b"pts/4", b"frank", b"", t0 + 8000, pid=1104),
    rec(DEAD, b"pts/4", b"", b"", t0 + 7900, pid=1104),
    rec(USER, b"pts/5", b"grace", b"", t0 + 8100, pid=1105),
    rec(DEAD, b"pts/5", b"", b"", t0 + 8100 - 7300, pid=1105),
    rec(USER, b"pts/6", b"heidi", b"", t0 + 8200, pid=1106),
    rec(DEAD, b"pts/6", b"", b"", t0 + 8200 - 90000, pid=1106),
    rec(USER, b"pts/7", b"ivan", b"::1", t0 + 8300, pid=1107, addr=bytes(15) + b"\x01"),
    rec(DEAD, b"pts/7", b"", b"", t0 + 8360, pid=1107),
    rec(USER, b"pts/8", b"judy", b"localhost", t0 + 8400, pid=1108, addr=v4(127, 0, 0, 1)),
    rec(DEAD, b"pts/8", b"", b"", t0 + 8460, pid=1108),
    rec(USER, b"pts/9", b"x", b"", t0 + 8500, pid=1109),
    rec(DEAD, b"pts/9", b"", b"", t0 + 8560, pid=1109),
    rec(RUN_LVL, b"~", b"runlevel", b"", t0 + 9000, pid=0),
    rec(RUN_LVL, b"~", b"runlevel", b"", t0 + 9001, pid=ord("6")),
    rec(BOOT_TIME, b"~", b"reboot", b"6.1.0-test", t0 + 9100),
    rec(USER, b"tty1", b"root", b"", t0 + 9200, pid=1200),
    rec(DEAD, b"tty1", b"", b"", t0 + 9300, pid=1200),
    rec(USER, b"tty1", b"root", b"", t0 + 9400, pid=1201),
]
wtmp = b"".join(recs)
put("wtmp", wtmp)
put("misaligned", wtmp + b"\xee" * 100)
put("short", wtmp[:200])
put("empty", b"")
put("unreadable", wtmp, 0)
os.mkdir(os.path.join(d, "dir"))

# Across 16 KiB chunks: 130 records, a login and a logout on rotating lines.
big = [rec(BOOT_TIME, b"~", b"reboot", b"5.15.0", t0 - 100000)]
for k in range(64):
    line = b"pts/%d" % (k % 7)
    big.append(rec(USER, line, b"user%d" % (k % 5), b"10.0.0.%d" % k,
                   t0 - 90000 + k * 600, pid=2000 + k, addr=v4(10, 0, 0, k)))
    big.append(rec(DEAD, line, b"", b"", t0 - 90000 + k * 600 + 300 + k, pid=2000 + k))
big.append(rec(EMPTY, b"~", b"shutdown", b"5.15.0", t0 - 50000))
put("big", b"".join(big))

# Addresses the hosts file names, so -d asks no resolver: IPv4, IPv6 and
# IPv4 mapped into IPv6.
put("dns", b"".join([
    rec(USER, b"pts/1", b"root", b"one", t0, pid=5000, addr=v4(127, 0, 0, 1)),
    rec(USER, b"pts/2", b"root", b"two", t0 + 1, pid=5001, addr=bytes(15) + b"\x01"),
    rec(USER, b"pts/3", b"root", b"three", t0 + 2, pid=5002,
        addr=bytes(10) + b"\xff\xff" + v4(127, 0, 0, 1)),
    rec(DEAD, b"pts/1", b"", b"", t0 + 60, pid=5000),
]))

# The far ends of a 32-bit time.
put("extremes", b"".join([
    rec(BOOT_TIME, b"~", b"reboot", b"old", -100),
    rec(USER, b"tty1", b"root", b"", -50, pid=3000),
    rec(DEAD, b"tty1", b"", b"", 2147483647, pid=3000),
    rec(USER, b"tty2", b"root", b"", -2147483648, pid=3001),
]))

# Failed logins, as btmp has them.
put("btmp", b"".join([
    rec(LOGIN, b"ssh:notty", b"admin", b"203.0.113.5", t0 + 10, pid=4000, addr=v4(203, 0, 113, 5)),
    rec(LOGIN, b"ssh:notty", b"root", b"203.0.113.6", t0 + 20, pid=4001, addr=v4(203, 0, 113, 6)),
    rec(USER, b"tty1", b"mallory", b"", t0 + 30, pid=4002),
    rec(EMPTY, b"", b"", b"", t0 + 40),
]))

# Since this machine's boot: who is still logged in, and who has gone. A tty
# owned by root, a pid that is not running (so its tty is asked), a user who
# does not exist, and one whose tty is not theirs.
now = int(time.time()) - 30
put("now", b"".join([
    rec(USER, b"null", b"root", b"", now, pid=4194303),
    rec(USER, b"zero", b"nosuchuser", b"", now + 1, pid=4194302),
    rec(USER, b"full", b"daemon", b"", now + 2, pid=4194301),
    rec(USER, b"random", b"root", b"", now + 3, pid=os.getppid()),
    rec(USER, b"nosuchtty", b"root", b"", now + 4, pid=4194300),
    rec(USER, b"pts/0", b"root", b"", 1000, pid=4194299),
]))
PY
cp /var/log/wtmp "$fix/system" 2>/dev/null || : >"$fix/system"

# --- knobs ------------------------------------------------------------------
# PROG: last or lastb. REDIR: redirections after the command. ENVS: the
# environment's extra entries (TZ=UTC unless NOTZ is set). STDIN: a fixture
# piped to the command.
PROG=last
REDIR=
ENVS=()
STDIN=
NOTZ=
reset_knobs() { PROG=last; REDIR=; ENVS=(); STDIN=; NOTZ=; }

run_side() {
  local side=$1; shift
  local -a envs=(env -i "PATH=$bindir/$side:/usr/bin:/bin" "LC_ALL=C.UTF-8")
  [ -z "$NOTZ" ] && envs+=("TZ=UTC")
  envs+=("${ENVS[@]}")
  local -a cmd=(timeout -k 2 20 "${envs[@]}" sh -c \
    'cd "$1"; prog=$2; redir=$3; stdin=$4; shift 4
     if [ -n "$stdin" ]; then cat "$stdin" | eval "exec $prog \"\$@\" $redir"
     else eval "exec $prog \"\$@\" $redir"; fi' \
    _ "$fix" "$PROG" "$REDIR" "$STDIN" "$@")
  diff_run "${cmd[@]}" </dev/null
}

compare() {
  run_side ours "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side gnu "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  LABEL="$PROG $*"
  [ "${#ENVS[@]}" -gt 0 ] && LABEL="$LABEL [${ENVS[*]}]"
  [ -n "$NOTZ" ] && LABEL="$LABEL [TZ unset]"
  [ -n "$STDIN" ] && LABEL="$LABEL <|$STDIN"
  [ -n "$REDIR" ] && LABEL="$LABEL $REDIR"
  reset_knobs
  if [ "$o_rc" = 124 ] || [ "$g_rc" = 124 ] || [ "$o_rc" = 127 ] || [ "$g_rc" = 127 ]; then
    AGREED=broken
  elif cmp -s "$DIFF_TMP/o.out" "$DIFF_TMP/g.out" \
     && cmp -s "$DIFF_TMP/o.err" "$DIFF_TMP/g.err" && [ "$o_rc" = "$g_rc" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  --- ours rc=%s\n%s\n  ~~~ stderr\n%s\n  --- gnu rc=%s\n%s\n  ~~~ stderr\n%s' \
    "$o_rc" "$(cat -A "$DIFF_TMP/o.out" | head -40)" "$(cat -A "$DIFF_TMP/o.err" | head -10)" \
    "$g_rc" "$(cat -A "$DIFF_TMP/g.out" | head -40)" "$(cat -A "$DIFF_TMP/g.err" | head -10)")
}

report() {
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1))
    printf 'BROKEN %s -- never reached last on one or both sides\n%s\n' "$LABEL" "$REPORT"
  elif [ "$AGREED" = yes ]; then
    pass=$((pass + 1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$LABEL"
  else
    fail=$((fail + 1))
    printf 'DIFF %s\n%s\n' "$LABEL" "$REPORT"
  fi
  return 0
}

run_case() { compare "$@"; report; }

xfail_case() {
  local why=$1; shift
  [ "${1:-}" = -- ] && shift
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass + 1))
    printf 'XPASS %s -- expected to differ (%s)\n' "$LABEL" "$why"
  elif [ "$AGREED" = broken ]; then
    report
  else
    xfail=$((xfail + 1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL %s (%s)\n' "$LABEL" "$why"
  fi
  return 0
}

# A guard against vacuous agreement: the reference must have listed a login.
run_case -f wtmp
if ! grep -q '^bob ' "$DIFF_TMP/g.out"; then
  echo "last-diff: the reference listed nothing from the fixture:" >&2
  cat "$DIFF_TMP/g.out" "$DIFF_TMP/g.err" >&2
  exit 1
fi

# --- the fixtures, each way of showing them -----------------------------------
for f in wtmp big extremes btmp now misaligned short; do
  for o in '' -x -R -a -w -F -i -Rx -ax -wx -wa -iw -ia; do
    # shellcheck disable=SC2086  # the options are words, on purpose
    run_case $o -f "$f"
  done
  for t in notime short full iso; do
    run_case --time-format "$t" -f "$f"
    run_case --time-format="$t" -x -f "$f"
    run_case --time-format "$t" -a -f "$f"
    run_case --time-format "$t" -R -f "$f"
  done
done
for o in -d -dw -da -di -dR -dF; do
  run_case "$o" -f dns
done

# --- zones ------------------------------------------------------------------------
for z in UTC America/New_York Asia/Kolkata America/St_Johns Pacific/Marquesas \
         'ABC0:30' 'ABC-0:30' Europe/London; do
  for t in short full iso; do
    ENVS=("TZ=$z"); run_case -x --time-format "$t" -f wtmp
    ENVS=("TZ=$z"); run_case --time-format "$t" -f extremes
  done
done
NOTZ=1; run_case -x -f wtmp
NOTZ=1; run_case --time-format iso -f big

# --- how many --------------------------------------------------------------------
for n in 0 1 2 3 5 10 100 -1 4294967297; do
  run_case -n "$n" -f wtmp
  run_case -x --limit="$n" -f wtmp
done
run_case -3 -f wtmp
run_case -1 -2 -f wtmp
run_case -n 5 -3 -f wtmp
run_case -3 -n 2 -f wtmp
run_case -0 -f wtmp
run_case -n x -f wtmp
run_case -n '' -f wtmp
run_case -n 99999999999 -f wtmp
run_case -n ' 5' -f wtmp
run_case -n 5x -f wtmp

# --- names and ttys -----------------------------------------------------------
for who in root alice bob tty1 1 pts/0 ftp ftp1234 uucp uucp7 tty nosuch \
           '' reboot shutdown runlevel 'system boot' '~' x \
           abcdefghijklmnopqrstuvwxyz012345 abcdefghijklmnopqrstuvwxyz0123456789; do
  run_case -f wtmp "$who"
  run_case -x -f wtmp "$who"
done
run_case -f wtmp alice bob tty3
run_case alice -f wtmp -n 1
run_case -f big user1 pts/3

# --- times ------------------------------------------------------------------------
for s in '2023-11-14 22:15:00' '2023-11-14 22:15' '2023-11-14' '2023-11-15' \
         '@1700000100' '@1700007100' 'yesterday' 'today' 'tomorrow' 'now' \
         '-1h' '+1h' '1 hour ago' '2023-11-14T23:00:00' '22:30' '1960-01-01' \
         '1970-01-01' '@0' 'garbage' '' '2023-13-01' '2023-11-14 25:00'; do
  run_case -s "$s" -f wtmp
  run_case -t "$s" -f wtmp
  run_case -p "$s" -f wtmp
  run_case -x --since="$s" --until=2023-11-15 -f wtmp
done
run_case -p '2023-11-14 23:00' -f big
run_case -p '2023-11-14 00:00' -x -f big
run_case -s 2023-11-13 -t 2023-11-14 -f big

# --- files ---------------------------------------------------------------------------
run_case -f nosuch
run_case -f unreadable
run_case -f empty
run_case -f dir
run_case -f dir/
run_case -f ./dir//
run_case -f /dev/null
run_case -f /dev/zero
run_case -f /
run_case -f //
run_case -f ''
run_case -f wtmp -f big
run_case -f big -f wtmp -x
run_case -f wtmp -f nosuch -f big
run_case -n 3 -f wtmp -f big
run_case -n 1 -f wtmp -f /dev/zero
run_case -n 2 -f big -f /dev/zero -f wtmp
run_case -f wtmp -f /dev/null
run_case -f empty -f wtmp
STDIN=wtmp; run_case -f /dev/stdin
STDIN=big; run_case -f /dev/stdin -x
STDIN=wtmp; run_case -n 1 -f big -f /dev/stdin
run_case -f system
run_case -x -f system
run_case -F -f system
run_case --time-format iso -x -w -f system
run_case
run_case -x
run_case -n 3
run_case -i
run_case root

# --- lastb ------------------------------------------------------------------------
PROG=lastb; run_case -f btmp
PROG=lastb; run_case -f wtmp
PROG=lastb; run_case -x -f wtmp
PROG=lastb; run_case -n 2 -f big
PROG=lastb; run_case -F -a -f btmp
PROG=lastb; run_case -f btmp admin
PROG=lastb; run_case
PROG=lastb; run_case -h
PROG=lastb; run_case --help

# --- the command line ----------------------------------------------------------------
run_case -h
run_case --help
run_case --help -Z
run_case -Z
run_case --nosuch
run_case -f
run_case --file
run_case -n
run_case -s
run_case --time-format
run_case --time-format bogus -f wtmp
run_case --time-format '' -f wtmp
run_case --time-format ISO -f wtmp
run_case -F --time-format iso -f wtmp
run_case --time-format iso -F -f wtmp
run_case --time-format iso --time-format full -f wtmp
run_case -F -F -f wtmp
run_case --fu -f wtmp
run_case --fullt -f wtmp
run_case --fulln -f wtmp
run_case --time -f wtmp
run_case --time=iso -f wtmp
run_case --sys -f wtmp
run_case --li=2 -f wtmp
run_case -f wtmp -- -x
run_case -f wtmp -- root
run_case root -f wtmp -x
xfail_case "our version string, not util-linux's" -- -V
xfail_case "our version string, not util-linux's" -- --version
PROG=lastb; xfail_case "our version string, not util-linux's" -- -V

# --- descriptors that cannot be written -------------------------------------------------
for redir in '>&-' '>/dev/full' '2>&-' '2>/dev/full'; do
  REDIR=$redir; run_case -f wtmp
  REDIR=$redir; run_case -f big -x
  REDIR=$redir; run_case -f nosuch
  REDIR=$redir; run_case --help
  REDIR=$redir; run_case -f dir
done

printf '\n%d passed, %d differed, %d broken, %d differ on purpose' \
  "$pass" "$fail" "$broken" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ] && [ "$xpass" -eq 0 ]
