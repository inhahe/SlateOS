#!/usr/bin/env bash
# pinky-diff.sh — compare our `pinky` against GNU coreutils 9.4's, inside WSL.
#
# ## Why the cases run in a private namespace
#
# `pinky` takes no FILE: it reads `/var/run/utmp` and the password database,
# and the long format reads `~/.plan` and `~/.project`. On a WSL host those
# hold whatever this machine happens to have, which is usually nobody logged
# in and nobody with a `&` in their real name -- a comparison of two headings.
#
# So each case runs under `unshare -rm` (a user namespace mapping us to root,
# and a private mount namespace, neither of which needs privileges or touches
# the host's own mounts) with a tmpfs over `/run` holding a fixture utmp and a
# fixture passwd bind-mounted over `/etc/passwd`. Both binaries read those,
# GNU's through glibc's `getutxent` and `getpwnam`, ours through `utmpfile` and
# `pwdb`. The fixtures cover:
#
# - sessions of known users, unknown users (`???`, right-aligned), a name of
#   eight bytes and one of thirty-two, a non-UTF-8 name, an empty name and
#   non-user records (neither listed);
# - terminals that accept messages and that do not, that do not exist, that
#   are named by an absolute path, after a space, or not at all, and whose
#   last access was seconds, hours or days ago, or at the epoch (`?????`);
#   the terminals are fixture files, so the idle column is chosen rather than
#   whatever `/dev` says, with one real device (`null`) besides;
# - hosts that are canonicalised (`localhost`), numeric, carry an X display,
#   are only a display (`:0`), or are not UTF-8;
# - GECOS fields with `&` (capitalised and not), commas, nothing at all, and
#   a name longer than the column; homes with a plan, a project, both, one
#   without a final newline, a plan that is a directory, binary content, and
#   a home that does not exist.
#
# The When column's format follows `LC_TIME` -- exactly `C` or `POSIX` gives
# `%b %e %H:%M`, anything else `%Y-%m-%d %H:%M` -- so the cases run under
# several locales, including `LC_ALL=` (set but empty), which counts as unset.
#
# A few cases also read the live files, without the namespace.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `--version` names SlateOS.
#
# A `/var/run/utmp` that is there and cannot be read (here, a directory):
# glibc's `getutxent` cannot report the failure, so GNU prints the heading and
# exits 0 -- "nobody is logged in". Ours reports `pinky: /var/run/utmp: Is a
# directory` and exits 1, as gnulib's own reader does (see `pinky.rs`).
set -u

DIFF_PROG='pinky'
DIFF_GNU_SOURCE=9.4
DIFF_NEED='python3 unshare'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

export TZ=America/New_York
# Every locale variable is chosen per case; nothing is inherited.
for v in $(env | sed -n 's/^\(LC_[A-Z_]*\)=.*/\1/p'); do unset "$v"; done
unset LANG LANGUAGE POSIXLY_CORRECT

pass=0; fail=0; xfail=0; xpass=0
ENVS=
MODE=file
TO_FULL=

fx=$DIFF_TMP/fx
mkdir -p "$fx" || exit 1
cat > "$DIFF_TMP/mkfix.py" <<'PY'
import os, struct, sys, time

fx = sys.argv[1]

# glibc x86_64 `struct utmp`: type, padding, pid, line[32], id[4], user[32],
# host[256], exit (two shorts), session, tv_sec, tv_usec, addr_v6[4],
# reserved[20] -- 384 bytes.
FMT = "<hhi32s4s32s256shhiii4i20s"
assert struct.calcsize(FMT) == 384
EMPTY, RUN_LVL, BOOT_TIME, LOGIN, USER, DEAD = 0, 1, 2, 6, 7, 8

def rec(kind, user, line, host=b"", when=1700000000, pid=1234):
    assert len(line) <= 32 and len(user) <= 32, (line, user)
    return struct.pack(FMT, kind, 0, pid, line, b"ts/0", user, host,
                       0, 0, 0, when, 0, 0, 0, 0, 0, b"")

now = int(time.time())

def tty(name, mode, atime):
    """A fixture terminal: a file with the mode and access time wanted."""
    path = os.path.join(fx, name)
    open(path, "w").close()
    os.chmod(path, mode)
    os.utime(path, (atime, atime))
    return path.encode()

active = tty("ta", 0o620, now - 10)                  # blank idle, writable
hours = tty("tb", 0o600, now - (2 * 3600 + 5 * 60 + 10))  # 02:05, `*`
days = tty("tc", 0o620, now - (3 * 86400 + 100))     # 3d
epoch = tty("td", 0o660, 0)                          # `?????`

def put(name, data):
    with open(os.path.join(fx, name), "wb") as f:
        f.write(data)

put("utmp", b"".join([
    rec(BOOT_TIME, b"reboot", b"~", when=1690000000, pid=0),
    rec(RUN_LVL, b"runlevel", b"~", pid=0),
    rec(LOGIN, b"LOGIN", b"tty1", pid=50),
    rec(USER, b"alice", active, b"", 1700000000),
    rec(USER, b"bob", hours, b"localhost", 1693700000),
    rec(USER, b"longnameuser", days, b"10.0.0.2:0.0", 1701234567),
    rec(USER, b"nosuchuser", b"pts/999", b":0", 1702000000),
    rec(USER, b"carol", b"x " + epoch, b"127.0.0.1", 1703000000),
    rec(USER, b"Zed", b"", b"", 1704000000),
    rec(USER, b"", b"pts/4", b"ghost", 1705000000),
    rec(DEAD, b"alice", b"pts/5", b"", 1706000000),
    rec(USER, b"alice", b"pts/12345678", b"", 1707000000),
    rec(USER, b"dave", b"null", b"", 1708000000),
    rec(USER, b"eightchr", active, b"", 1709000000),
    rec(USER, b"caf\xe9", active, b"h\xe9st", 1710000000),
    rec(USER, b"u" * 32, hours, b"", 1711000000),
    rec(EMPTY, b"", b"", b"", 0, 0),
]))

home = os.path.join(fx, "home")
def mkhome(user, files):
    d = os.path.join(home, user)
    os.makedirs(d)
    for name, data in files.items():
        if data is None:
            os.mkdir(os.path.join(d, name))
        else:
            with open(os.path.join(d, name), "wb") as f:
                f.write(data)
    return d

mkhome("alice", {".plan": b"Tea at four.\nThen croquet.\n", ".project": b"Wonderland\n"})
mkhome("bob", {".plan": b"no final newline"})
mkhome("zed", {".project": b"project, no newline", ".plan": b"plan\n"})
mkhome("carol", {})
mkhome("dave", {".plan": None, ".project": b"dave's project\n"})
mkhome("cafe", {".plan": b"bin\x00ary\xff\n"})

passwd = b"".join(line + b"\n" for line in [
    b"root:x:0:0:root:/root:/bin/bash",
    b"alice:x:1001:1001:& Liddell,Room 1,555-1234:" + home.encode() + b"/alice:/bin/sh",
    b"bob:x:1002:1002:Robert &&:" + home.encode() + b"/bob:/bin/zsh",
    b"Zed:x:1003:1003:x&y:" + home.encode() + b"/zed:/bin/false",
    b"carol:x:1004:1004::" + home.encode() + b"/carol:",
    b"longnameuser:x:1005:1005:A Very Long Real Name Indeed:" + home.encode() + b"/nohome:/bin/sh",
    b"dave:x:1006:1006:Dave,,,:" + home.encode() + b"/dave:/bin/sh",
    b"caf\xe9:x:1007:1007:Caf\xe9 &:" + home.encode() + b"/cafe:/bin/sh",
    b"u" * 32 + b":x:1008:1008:Thirty-two:/:/bin/sh",
])
put("passwd", passwd)
PY
python3 "$DIFF_TMP/mkfix.py" "$fx" || { echo "pinky-diff: could not build the fixtures" >&2; exit 1; }

# The wrapper each namespaced case runs through. 97 is a setup failure, never
# a result: `compare` refuses to count it as an agreement.
cat > "$fx/inns.sh" <<'SH'
# inns.sh MODE COMMAND...: COMMAND with a private /run holding the utmp MODE
# names and the fixture passwd in place of /etc/passwd.
fx=$(dirname "$0")
mount -t tmpfs none /run || exit 97
case $1 in
  file) cp "$fx/utmp" /run/utmp || exit 97 ;;
  empty) : > /run/utmp || exit 97 ;;
  dir) mkdir /run/utmp || exit 97 ;;
  missing) ;;
  *) exit 97 ;;
esac
shift
mount --bind "$fx/passwd" /etc/passwd || exit 97
exec "$@"
SH

if [ "$(readlink /var/run)" != /run ] && [ "$(readlink /var/run)" != ../run ]; then
  echo "pinky-diff: /var/run is not a link to /run here, so a private /run does not reach /var/run/utmp" >&2
  exit 1
fi
# util-linux's `unshare` builds the private namespace every fixture case runs
# in. Linux has it and the Windows host this script re-executes from does not,
# so it is resolved once, here, and its absence said plainly rather than left
# to become an empty probe. Absolute, like `xargs-diff.sh`'s `SETSID`.
UNSHARE=$(command -v unshare) || {
  echo "pinky-diff: unshare(1) is not on PATH; the utmp fixtures need a private mount namespace" >&2
  exit 1
}
probe=$("$UNSHARE" -rm sh "$fx/inns.sh" file sh -c 'test -s /var/run/utmp && head -c 5 /etc/passwd' 2>&1)
if [ "$probe" != "root:" ]; then
  echo "pinky-diff: cannot build the private namespace: $probe" >&2
  exit 1
fi

me=$(id -un)

compare() {
  local side out err rc o_out g_out o_err g_err o_rc g_rc
  local -a run
  for side in ours gnu; do
    out=$DIFF_TMP/out-$side; err=$DIFF_TMP/err-$side
    if [ "$MODE" = live ]; then
      run=(env PATH="$bindir/$side:$PATH" pinky "$@")
    else
      run=("$UNSHARE" -rm sh "$fx/inns.sh" "$MODE" env PATH="$bindir/$side:$PATH" pinky "$@")
    fi
    if [ -n "$TO_FULL" ]; then
      ( if [ -n "$ENVS" ]; then eval "export $ENVS"; fi; timeout -k 2 30 "${run[@]}" ) >/dev/full 2>"$err" </dev/null
    else
      ( if [ -n "$ENVS" ]; then eval "export $ENVS"; fi; timeout -k 2 30 "${run[@]}" ) >"$out" 2>"$err" </dev/null
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
  if [ "$o_rc" = 97 ] || [ "$g_rc" = 97 ]; then
    AGREED=setup
  elif [ "$o_rc" = "$g_rc" ] && [ "$o_out" = "$g_out" ] && [ "$o_err" = "$g_err" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out{%s} err{%s}\n  gnu  (rc=%s): out{%s} err{%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_err" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_err" | tr '\n' '|')")
}

label_of() {
  printf '%s' "${ENVS:+$ENVS }[$MODE] pinky $*${TO_FULL:+  [>/dev/full]}"
}

run_case() {
  local label; label=$(label_of "$@")
  compare "$@"
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    [ "$AGREED" = setup ] && printf 'SETUP FAILED (status 97) -- '
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
  elif [ "$AGREED" = setup ]; then
    fail=$((fail+1))
    printf 'SETUP FAILED (status 97) -- %s\n%s\n' "$label" "$REPORT"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$label" "$why"
  fi
  return 0
}

MODE=file
for ENVS in 'LC_ALL=C.UTF-8' 'LC_ALL=C' 'LC_ALL=POSIX' 'LANG=C.UTF-8' \
            'LC_ALL= LANG=C.UTF-8' 'LC_ALL= LC_TIME=POSIX LANG=C.UTF-8' \
            'LC_TIME=C.UTF-8 LANG=C' ''; do
  # --- the short format, under every column switch -------------------------------
  run_case
  run_case -s
  run_case -f
  run_case -w
  run_case -i
  run_case -q
  run_case -f -q
  run_case -w -i
  run_case -l -s
  run_case -lbhp -s
done

ENVS='LC_ALL=C.UTF-8'
# --- choosing sessions by user -------------------------------------------------------
run_case alice
run_case alice bob
run_case bob alice alice
run_case nosuchuser
run_case "$(printf 'caf\351')"
run_case "$(printf 'u%.0s' $(seq 32))"
run_case ''
run_case notloggedin
run_case -q alice carol
run_case alice -q
run_case -- alice
ENVS='LC_ALL=C.UTF-8 POSIXLY_CORRECT=1'
run_case alice -q

# --- the long format -----------------------------------------------------------------
for ENVS in 'LC_ALL=C.UTF-8' 'LC_ALL=C'; do
  run_case -l alice
  run_case -l bob
  run_case -l Zed
  run_case -l carol
  run_case -l dave
  run_case -l longnameuser
  run_case -l "$(printf 'caf\351')"
  run_case -l "$(printf 'u%.0s' $(seq 32))"
  run_case -l nosuchuser
  run_case -l ''
  run_case -l root
  run_case -l alice bob nosuchuser Zed
  run_case -lb alice
  run_case -lh alice
  run_case -lp alice
  run_case -l -b -h -p alice
  run_case -lhp Zed
  run_case -s -l alice
  run_case -l -f -w -i -q alice
done

# --- utmp files that hold nobody -----------------------------------------------------
ENVS='LC_ALL=C.UTF-8'
MODE=empty; run_case; run_case -q
MODE=missing; run_case; run_case -f
MODE=missing; run_case -l alice
MODE=dir
xfail_case 'glibc hides the failure; gnulib and ours report it'
xfail_case 'glibc hides the failure; gnulib and ours report it' -f
run_case -l alice

# --- errors and the command line -----------------------------------------------------
MODE=file
run_case -l
run_case -l -f
run_case -x
run_case -lx alice
run_case --nope
run_case alice --nope
run_case --help=1
run_case --ver=1
xfail_case 'an abbreviation of --help, whose text is ours' --h
xfail_case 'our --help omits the GNU ancillary block' --help
xfail_case 'our --help omits the GNU ancillary block' -l --help
xfail_case 'our --version names SlateOS' --version

# --- write errors --------------------------------------------------------------------
TO_FULL=1; run_case
TO_FULL=1; run_case -l alice

# --- the live files, outside the namespace -------------------------------------------
MODE=live
for ENVS in 'LC_ALL=C.UTF-8' 'LC_ALL=C'; do
  run_case
  run_case -q
  run_case -l root
  run_case -l "$me"
  run_case -l root "$me" nosuchuser
done
ENVS=

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)\n' "$xpass"
  exit 1
fi
printf '\n'
[ "$fail" -eq 0 ]
