#!/usr/bin/env bash
# Differential test: our `wall`, `write` and `mesg` against util-linux
# 2.39.3's, as Ubuntu builds them (with its fix for CVE-2024-28085).
#
# The references are Ubuntu's: `wall` in bsdutils, `write` in bsdextrautils,
# `mesg` in util-linux.
#
# `wall` and `write` read /var/run/utmp and write to /dev/LINE, and neither
# takes another name for either. So both run in a user and mount namespace
# (`unshare -Urm`) where a fixture utmp is bound over /run/utmp and a
# directory of fixture "terminals" -- files, a FIFO with no reader, a
# directory, /dev/full -- over /srv, and the utmp lines name them as
# `../srv/NAME`: `/dev/../srv/NAME`. Each side gets its own copy of the
# terminals, and what each wrote into them is compared too, the banner's
# date and the greeting's time masked (the two sides run a moment apart).
# A second, nested namespace runs them as uid 1000 rather than root, for
# the paths only a user takes.
#
# What is compared: stdout, stderr, the exit status and the terminals'
# contents and modes.
#
#   * wall: the message from words (escaped since CVE-2024-28085), a file and
#     standard input -- control characters, UTF-8 and bytes that are not,
#     lines past 79 columns, no final newline, an empty one; the banner and
#     -n, as root and not; -t; -g by name and gid, upstream's group test
#     included; every kind of utmp entry it skips; terminals that are files,
#     missing, a FIFO nobody reads, a directory, /dev/full;
#   * write: a user's best terminal by access time, the one named, one not in
#     utmp, messages off, the gid that does not match, the user not logged
#     in, more than once, only on the sender's own terminal; the arguments as
#     argc counts them, `--` included;
#   * mesg: on a terminal in each of its states, y, n and what rpmatch
#     refuses, -v, no terminal at all;
#   * the command lines' refusals.
#
# Cases that differ on purpose: --version and -V name this build.
set -u

DIFF_PROG='wall'
DIFF_BINS='wall write mesg'
DIFF_NEED='timeout python3 unshare'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

if [ -n "$DIFF_SKIPPED" ]; then
  echo "wall-diff: no reference for:$DIFF_SKIPPED -- SKIPPED"
  exit 0
fi
if ! unshare -Urm true 2>/dev/null; then
  echo "wall-diff: SKIPPED -- unprivileged user and mount namespaces are refused here"
  exit 0
fi

pass=0; fail=0; xfail=0; xpass=0; broken=0
fix=$DIFF_TMP/fix
mkdir -p "$fix"

python3 - "$fix" <<'PY'
import os, struct, sys, time
d = sys.argv[1]

EMPTY, RUN_LVL, BOOT_TIME, INIT, LOGIN, USER, DEAD = 0, 1, 2, 5, 6, 7, 8

def rec(ty, line=b"", user=b"", host=b"", sec=1700000000, pid=100):
    return struct.pack("<hxxi32s4s32s256shhiii16s20x", ty, pid, line, b"", user,
                       host, 0, 0, 0, sec, 0, b"")

def put(name, data, mode=None):
    path = os.path.join(d, name)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(data)
    if mode is not None:
        os.chmod(path, mode)

# The terminals. Access times are set: `write` chooses the most recent.
srv = os.path.join(d, "srv")
os.makedirs(srv)
for name, mode, atime in (("t1", 0o620, 1700000100), ("t2", 0o620, 1700000300),
                          ("t3", 0o620, 1700000200), ("off", 0o600, 1700000400),
                          ("ro", 0o444, 1700000000), ("zero", 0o620, 0)):
    put("srv/" + name, b"", mode)
    os.utime(os.path.join(srv, name), (atime, 1700000000))
os.mkfifo(os.path.join(srv, "fifo"), 0o620)
os.mkdir(os.path.join(srv, "dir"))

def u(line, user=b"root", ty=USER):
    return rec(ty, b"../srv/" + line if line and not line.startswith(b"/") else line, user)

put("utmp/basic", b"".join([
    rec(BOOT_TIME, b"~", b"reboot"),
    u(b"t1"),
    u(b"t2", b"daemon"),
    u(b"t3", b"inhahe"),
]))
put("utmp/kinds", b"".join([
    rec(BOOT_TIME, b"~", b"reboot"),
    rec(LOGIN, b"../srv/t1", b"LOGIN"),
    rec(DEAD, b"../srv/t1", b""),
    rec(USER, b"../srv/t1", b""),
    rec(USER, b":0", b"root"),
    rec(USER, b"", b"root"),
    rec(INIT, b"../srv/t2", b"root"),
    u(b"t3"),
    u(b"nosuch"),
    u(b"fifo"),
    u(b"dir"),
    rec(USER, b"../dev/full", b"root"),
    rec(USER, b"../dev/null", b"root"),
]))
put("utmp/groups", b"".join([
    u(b"t1", b"root"),
    u(b"t2", b"daemon"),
    u(b"t3", b"inhahe"),
    u(b"ro", b"nobody"),
    u(b"zero", b"nosuchuser"),
]))
# write: inhahe on three terminals, one with messages off; daemon on one;
# bin only on an entry that is not a user process; root on the zero-atime
# terminal only.
put("utmp/writes", b"".join([
    u(b"t1", b"inhahe"),
    u(b"t2", b"inhahe"),
    u(b"t3", b"inhahe"),
    u(b"off", b"inhahe"),
    u(b"t1", b"daemon"),
    u(b"off", b"sys"),
    rec(LOGIN, b"../srv/t2", b"bin"),
    u(b"zero", b"root"),
    rec(USER, b"../dev/null", b"games"),
    u(b"nosuch", b"man"),
]))
put("utmp/empty", b"")

put("msg/plain", b"Hello there.\nSecond line.\n")
put("msg/controls", b"bell\x07 esc\x1b[31mred\x1b[0m tab\there cr\rx del\x7f nul\x01\n")
put("msg/utf8", "caf\u00e9 \u6f22\u5b57 \u2028sep \u0085nel\n".encode() + b"bad \xe9\xff end\n")
put("msg/long", (b"x" * 200) + b"\n" + (b"y" * 79) + b"\n" + (b"z" * 80) + b"\n")
put("msg/nonl", b"no final newline")
put("msg/empty", b"")
put("msg/wide", ("\u6f22" * 50).encode() + b"\n")
PY

# --- knobs ------------------------------------------------------------------
# UTMP: the utmp fixture. USER: run as uid 1000 in a nested namespace. STDIN:
# a fixture for standard input. ENVS: extra environment. REDIR: redirections.
# PTY: run under a terminal on standard input (mesg). NONS: no namespaces.
UTMP=basic; NONROOT=; STDIN=; ENVS=(); REDIR=; PTY=; NONS=; PTYLOGIN=
reset_knobs() { UTMP=basic; NONROOT=; STDIN=; ENVS=(); REDIR=; PTY=; NONS=; PTYLOGIN=; }

# ptymesg.py: COMMAND with standard input on a fresh terminal; prints the
# terminal's mode before and after, then what the command wrote.
ptyrun=$DIFF_TMP/ptyrun.py
cat >"$ptyrun" <<'PY'
import os, stat, subprocess, sys
mode_before = sys.argv[1]
master, slave = os.openpty()
name = os.ttyname(slave)
if mode_before:
    os.chmod(name, int(mode_before, 8))
before = stat.S_IMODE(os.stat(name).st_mode)
p = subprocess.run(sys.argv[2:], stdin=slave, capture_output=True)
after = stat.S_IMODE(os.stat(name).st_mode)
os.close(slave)
os.close(master)
sys.stdout.buffer.write(p.stdout)
sys.stdout.buffer.write(b"=== terminal mode %o -> %o\n" % (before, after))
sys.stderr.buffer.write(p.stderr)
sys.exit(p.returncode)
PY

# ptylogin.py UTMP SRV USER INPUT -- COMMAND: COMMAND in the namespaces the
# other cases use, with standard input on a fresh terminal that a utmp
# entry, written once the terminal exists, says USER logged in on -- so
# `getlogin ()` names USER while the process is root. INPUT is typed at the
# terminal, then an end of file.
ptylogin=$DIFF_TMP/ptylogin.py
cat >"$ptylogin" <<'PY'
import os, struct, subprocess, sys
utmp, srv, user, inp = sys.argv[1:5]
cmd = sys.argv[6:]
master, slave = os.openpty()
line = os.ttyname(slave)[len("/dev/"):].encode()
def rec(line, who):
    return struct.pack("<hxxi32s4s32s256shhiii16s20x", 7, os.getpid(), line, b"",
                       who, b"", 0, 0, 0, 1700000000, 0, b"")
with open(utmp, "wb") as f:
    # The terminal, and two fixture ones for somebody to be written to.
    f.write(rec(line, user.encode()) + rec(b"../srv/t1", b"root")
            + rec(b"../srv/t2", b"inhahe"))
wrapper = ['unshare', '-Urm', 'sh', '-c',
           'mount --bind "$1" /run/utmp && mount --bind "$2" /srv || exit 99; '
           'shift 2; exec "$@"', '_', utmp, srv] + cmd
p = subprocess.Popen(wrapper, stdin=slave)
os.close(slave)
data = open(inp, "rb").read() if inp else b""
eof = b"\x04" if not data or data.endswith(b"\n") else b"\x04\x04"
os.write(master, data + eof)
rc = p.wait()
os.close(master)
sys.exit(rc)
PY

# The banner's date and the greeting's time, masked: the two sides run a
# moment apart.
mask_times() {
  sed -E \
    -e 's/\((Mon|Tue|Wed|Thu|Fri|Sat|Sun) [A-Z][a-z]{2} [ 0-9][0-9] [0-9]{2}:[0-9]{2}:[0-9]{2} [0-9]{4}\):/(DATE):/' \
    -e 's/ at [0-9]{2}:[0-9]{2} \.\.\./ at HH:MM .../' \
    -e 's#pts/[0-9]+#pts/N#g' "$@"
}

# The terminals one side left, by name: mode and contents, times masked.
dump_srv() {
  local dir=$1 f
  for f in "$dir"/*; do
    [ -f "$f" ] || continue
    printf '=== %s %s\n' "${f##*/}" "$(stat -c %a "$f")"
    mask_times "$f"
  done
}

run_side() {
  local side=$1; shift
  local prog=$1; shift
  local srv=$DIFF_TMP/srv-$side
  rm -rf "$srv"
  cp -a "$fix/srv" "$srv"
  # The access times `write` chooses by, set on the copy: copying the
  # template reads it, and under relatime that moves its own to now.
  touch -a -d @1700000100 "$srv/t1"
  touch -a -d @1700000300 "$srv/t2"
  touch -a -d @1700000200 "$srv/t3"
  touch -a -d @1700000400 "$srv/off"
  touch -a -d @1700000000 "$srv/ro"
  touch -a -d @0 "$srv/zero"
  local -a envs=(env -i "PATH=$bindir/$side:/usr/bin:/bin" "LC_ALL=C.UTF-8" "TZ=UTC" "${ENVS[@]}")
  local stdin=/dev/null
  [ -n "$STDIN" ] && stdin=$fix/msg/$STDIN
  if [ -n "$PTY" ]; then
    diff_run timeout -k 2 20 "${envs[@]}" python3 "$ptyrun" "${PTY#=}" "$prog" "$@" <"$stdin"
    return
  fi
  if [ -n "$PTYLOGIN" ]; then
    local input=
    [ -n "$STDIN" ] && input=$fix/msg/$STDIN
    ( cd "$DIFF_TMP" && diff_run timeout -k 2 20 "${envs[@]}" python3 "$ptylogin" \
        "$DIFF_TMP/utmp-$side" "$srv" "$PTYLOGIN" "$input" -- "$prog" "$@" </dev/null )
    local rc=$?
    dump_srv "$srv" >>"$DIFF_TMP/$side.after"
    return $rc
  fi
  if [ -n "$NONS" ]; then
    diff_run timeout -k 2 20 "${envs[@]}" sh -c \
      'redir=$1; shift; eval "exec \"\$@\" $redir"' _ "$REDIR" "$prog" "$@" <"$stdin"
    return
  fi
  local inner=""
  [ -n "$NONROOT" ] && inner="unshare --user --map-user=1000 --map-group=1000"
  ( cd "$DIFF_TMP" && diff_run timeout -k 2 20 "${envs[@]}" unshare -Urm sh -c \
      'mount --bind "$1" /run/utmp && mount --bind "$2" /srv || exit 99
       inner=$3; redir=$4; shift 4
       eval "exec $inner \"\$@\" $redir"' \
      _ "$fix/utmp/$UTMP" "$srv" "$inner" "$REDIR" "$prog" "$@" <"$stdin" )
  local rc=$?
  dump_srv "$srv" >>"$DIFF_TMP/$side.after"
  return $rc
}

compare() {
  local prog=$1
  : >"$DIFF_TMP/ours.after"; : >"$DIFF_TMP/gnu.after"
  run_side ours "$@" >"$DIFF_TMP/o.raw" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side gnu "$@" >"$DIFF_TMP/g.raw" 2>"$DIFF_TMP/g.err"; g_rc=$?
  # `write` to a terminal chosen with no access time writes on its own
  # standard output, greeting and all.
  mask_times "$DIFF_TMP/o.raw" >"$DIFF_TMP/o.out"
  mask_times "$DIFF_TMP/g.raw" >"$DIFF_TMP/g.out"
  cat "$DIFF_TMP/ours.after" >>"$DIFF_TMP/o.out"
  cat "$DIFF_TMP/gnu.after" >>"$DIFF_TMP/g.out"
  LABEL="$*"
  [ -z "$NONS" ] && [ -z "$PTY" ] && [ -z "$PTYLOGIN" ] && LABEL="$LABEL [utmp $UTMP]"
  [ -n "$NONROOT" ] && LABEL="$LABEL [uid 1000]"
  [ -n "$STDIN" ] && LABEL="$LABEL <$STDIN"
  [ -n "$PTY" ] && LABEL="$LABEL [pty${PTY#=}]"
  [ -n "$NONS" ] && LABEL="$LABEL [no namespace]"
  [ -n "$PTYLOGIN" ] && LABEL="$LABEL [terminal, utmp says $PTYLOGIN]"
  [ "${#ENVS[@]}" -gt 0 ] && LABEL="$LABEL [${ENVS[*]}]"
  [ -n "$REDIR" ] && LABEL="$LABEL $REDIR"
  reset_knobs
  if [ "$o_rc" = 124 ] || [ "$g_rc" = 124 ] || [ "$o_rc" = 127 ] || [ "$g_rc" = 127 ] \
     || [ "$o_rc" = 99 ] || [ "$g_rc" = 99 ]; then
    AGREED=broken
  elif cmp -s "$DIFF_TMP/o.out" "$DIFF_TMP/g.out" \
     && cmp -s "$DIFF_TMP/o.err" "$DIFF_TMP/g.err" && [ "$o_rc" = "$g_rc" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  --- ours rc=%s\n%s\n  ~~~ stderr\n%s\n  --- gnu rc=%s\n%s\n  ~~~ stderr\n%s' \
    "$o_rc" "$(cat -A "$DIFF_TMP/o.out" | head -60)" "$(cat -A "$DIFF_TMP/o.err" | head -12)" \
    "$g_rc" "$(cat -A "$DIFF_TMP/g.out" | head -60)" "$(cat -A "$DIFF_TMP/g.err" | head -12)")
}

report() {
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1))
    printf 'BROKEN %s -- never reached the program on one or both sides\n%s\n' "$LABEL" "$REPORT"
  elif [ "$AGREED" = yes ]; then
    pass=$((pass + 1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$LABEL"
    # SHOW=PATTERN prints what both sides did for the passing cases whose
    # label holds PATTERN: the check that agreement is about something.
    case "$LABEL" in
      *"${SHOW:-$'\001'}"*) printf 'SHOW %s\n%s\n' "$LABEL" "$REPORT" ;;
    esac
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

# A guard against vacuous agreement: the reference must have written to t1.
run_case wall -n hello
if ! grep -q '^hello' "$DIFF_TMP/g.out"; then
  echo "wall-diff: the reference wrote nothing into the fixture terminals:" >&2
  cat "$DIFF_TMP/g.out" "$DIFF_TMP/g.err" >&2
  exit 1
fi

# --- wall: the message ---------------------------------------------------------
for m in plain controls utf8 long nonl empty wide; do
  STDIN=$m; run_case wall
  STDIN=$m; run_case wall -n
  run_case wall -n "msg/$m"
  run_case wall "$fix/msg/$m"
  NONROOT=1; STDIN=$m; run_case wall
done
run_case wall hello world
run_case wall -n "$(printf 'esc\033[2J bell\007 tab\tx')" "$(printf 'two\nlines')"
run_case wall -n "$(printf 'caf\303\251 \351')"
run_case wall -n "$(printf '%0100d' 0)" "$(printf '%080d' 0)"
run_case wall -n ''
run_case wall -n '' ''
run_case wall -n nosuchfile
run_case wall -n nosuchfile also
run_case wall -n "$fix/msg/plain" second
run_case wall -n /
run_case wall -n --
run_case wall -n -- -n
NONROOT=1; run_case wall -n words
NONROOT=1; run_case wall --nobanner words
run_case wall -n -t 1 timed
run_case wall -n --timeout=5 timed

# --- wall: who receives it -------------------------------------------------------
for ut in kinds groups empty; do
  run_case wall -n "to $ut"
  UTMP=$ut; run_case wall -n "to $ut"
  UTMP=$ut; NONROOT=1; run_case wall "to $ut"
done
for g in root 0 daemon 1 inhahe 1000 adm 4 users 100 nogroup 65534 nosuch 99999 '' 4294967296 -1; do
  UTMP=groups; run_case wall -n -g "$g" "group $g"
done
UTMP=groups; run_case wall -n -g daemon -g adm "two groups"

# --- write -----------------------------------------------------------------------------
for who in inhahe daemon sys bin root games man nosuch ''; do
  UTMP=writes; STDIN=plain; run_case write "$who"
  UTMP=writes; STDIN=plain; NONROOT=1; run_case write "$who"
done
for tty in ../srv/t1 /dev/../srv/t2 ../srv/off ../srv/nosuch t1 ../dev/null ../srv/zero; do
  UTMP=writes; STDIN=controls; run_case write inhahe "$tty"
  UTMP=writes; STDIN=controls; NONROOT=1; run_case write inhahe "$tty"
  UTMP=writes; STDIN=controls; run_case write games "$tty"
done
UTMP=writes; STDIN=utf8; run_case write daemon
UTMP=writes; STDIN=long; run_case write daemon
UTMP=writes; STDIN=nonl; run_case write daemon
UTMP=writes; STDIN=empty; run_case write daemon
UTMP=writes; STDIN=plain; run_case write inhahe --
UTMP=writes; STDIN=plain; run_case write -- inhahe
UTMP=writes; STDIN=plain; run_case write inhahe -- ../srv/t1
UTMP=writes; STDIN=plain; run_case write a b c
UTMP=writes; STDIN=plain; run_case write
UTMP=writes; STDIN=plain; ENVS=(POSIXLY_CORRECT=1); run_case write inhahe -h
# getlogin () naming someone other than the real uid: the greeting's "(as".
PTYLOGIN=inhahe; STDIN=plain; run_case write root
PTYLOGIN=root; STDIN=plain; run_case write root
PTYLOGIN=inhahe; STDIN=nonl; run_case write root
PTYLOGIN=inhahe; STDIN=controls; run_case write root /dev/null

# --- mesg ----------------------------------------------------------------------------------
for mode in 620 600 622 602 640 666; do
  for a in '' y n yes No Y N 1 maybe '' -v; do
    # shellcheck disable=SC2086  # an empty $a is no argument, on purpose
    PTY="=$mode"; run_case mesg $a
  done
  PTY="=$mode"; run_case mesg -v y
  PTY="=$mode"; run_case mesg -v n
  PTY="=$mode"; run_case mesg y extra
done
NONS=1; run_case mesg
NONS=1; run_case mesg -v
NONS=1; run_case mesg -v y

# --- the command lines --------------------------------------------------------------------------
for p in wall write mesg; do
  NONS=1; run_case "$p" -h
  NONS=1; run_case "$p" --help
  NONS=1; run_case "$p" -Z
  NONS=1; run_case "$p" --nosuch
  xfail_case "our version string, not util-linux's" -- "$p" -V
done
NONS=1; run_case wall -t
NONS=1; run_case wall -t 0 x
NONS=1; run_case wall -t x x
NONS=1; run_case wall -t 99999999999 x
NONS=1; run_case wall -t -1 x
NONS=1; run_case wall -g
NONS=1; REDIR='>&-'; run_case wall --help
NONS=1; REDIR='>/dev/full'; run_case wall --help
NONS=1; REDIR='>/dev/full'; run_case mesg --help

printf '\n%d passed, %d differed, %d broken, %d differ on purpose' \
  "$pass" "$fail" "$broken" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ] && [ "$xpass" -eq 0 ]
