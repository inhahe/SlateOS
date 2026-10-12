#!/usr/bin/env bash
# Differential test: our `pstree` against psmisc 23.7's.
#
# The reference is Ubuntu's build, in `psmisc`.
#
# A process tree is only comparable if both sides look at the same one, so
# each case runs in a fresh user, pid and mount namespace
# (`unshare --map-auto --map-root-user -pf --mount-proc`) whose PID 1 is a
# fixture ($DIFF_TMP/fix/fix.py). It builds a tree one process at a time --
# each child finishes its own subtree, threads and all, before its parent
# makes the next -- so every pid and thread id comes out the same on both
# sides; then it runs the subject as `pstree ARGS`, and exits with its
# status, which ends the namespace. Leaves that need their own name and
# command line are a sleeper exec'd through a symlink of that name.
#
# What is compared: stdout, stderr and the exit status. Namespace inode
# numbers (-N's headers) are new each run, so both sides' are renumbered in
# order of appearance first.
#
#   * the tree: compaction of identical leaves, subtrees and threads, `-c`,
#     `-p` (which implies it), `-n`, `-g` (a process group of 0 prints as
#     nothing), threads with `-t` and `-T`, zombies (`-a` brackets them);
#   * names and arguments: spaces, parentheses, a backslash, UTF-8 and
#     invalid bytes, control characters, empty arguments, a name of one
#     character, long argument lists cut with `...`;
#   * width: COLUMNS in every form strtol takes or refuses, `-l`, a terminal's
#     window, and a namespace header that eats the first line's columns;
#   * `-u` across real uids (the namespace maps a range), `-S`, `-N` for
#     each type, `-Z`, `-s PID`, `-C age`, a PID or USER operand;
#   * `-h`/`-H` under real and crafted terminfo entries -- sgr0 trimmed for
#     termcap (`me`), generic and hard-copy terminals, and padding, which on
#     a terminal is pad characters at its speed (`pad`, else NUL) or, with
#     `npc`, a pause -- and the symbol choice on a terminal, with
#     setupterm's complaints;
#   * `pstree.x11`, the command line's refusals, closed and full stdout.
#
# The locales are ones WSL has. A name it lacks makes glibc's setlocale fall
# back to C, which `coreutils::locale` does not model -- it takes a name at
# its word, as SlateOS's C library does -- so such a case would measure the
# host's installation rather than pstree.
#
# Cases that differ on purpose: -V names this build.
set -u

DIFF_PROG='pstree'
DIFF_NEED='timeout python3 gcc tic newuidmap unshare'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0; broken=0
fix=$DIFF_TMP/fix
mkdir -p "$fix/x" "$fix/terminfo"
# The fixture's processes that take another uid exec the sleeper too, so
# the way to it must be open to them.
chmod 755 "$DIFF_TMP" "$fix" "$fix/x"

# The sleeper every exec'd leaf is: it ignores its arguments.
printf '#include <unistd.h>\nint main(void) { for (;;) pause(); }\n' >"$fix/sleeper.c"
gcc -O2 -o "$fix/sleeper" "$fix/sleeper.c" || { echo "pstree-diff: cannot build the sleeper" >&2; exit 1; }

# Crafted terminals, for what the system's entries do not exercise.
cat >"$fix/terminals.ti" <<'TI'
zz-gen|generic and nothing more,
	gn, bold=\E[1m, sgr0=\E[m,
zz-realgen|generic in name only,
	gn, bold=\E[1m, clear=\E[H\E[2J, cup=\E[%i%p1%d;%p2%dH, sgr0=\E[m,
zz-hc|hard copy,
	hc, bold=\E[1m, sgr0=\E[m,
zz-pad|padded,
	bold=\E[1m$<5>, sgr0=\E[0m$<2>,
zz-mpad|mandatory padding,
	bold=\E[1m, sgr0=\E[0m$<2/>,
zz-nobold|no bold,
	sgr0=\E[m,
zz-sgr10|sgr resetting the font,
	bold=\E[1m, sgr0=\E[0;10m, sgr=\E[0%?%p9%t;11%e;10%;%?%p6%t;1%;m,
zz-strstr|sgr0 holding sgr(0),
	bold=\E[1m, sgr0=\E[m\E[0m, sgr=\E[0%?%p9%t;5%;m,
zz-pc|padded with a pad character,
	pad=*, bold=\E[1m$<2>, sgr0=\E[m$<1.5>,
zz-npc|padded by pausing,
	npc, bold=\E[1m$<20>, sgr0=\E[m,
TI
tic -x -o "$fix/terminfo" "$fix/terminals.ti" 2>"$DIFF_TMP/tic.err" || {
  echo "pstree-diff: tic failed:" >&2; cat "$DIFF_TMP/tic.err" >&2; exit 1
}

python3 - "$fix" <<'PY'
import os, sys
d = sys.argv[1]
names = [b"worker", b"leaf", b"member", b"child", b"inside", b"sub",
         b"sp ace", b"par(en)s", b"back\\slash", b"na\xc3\xafve", b"bad\xff\xfe",
         b"ctl\x01x", b"empties", b"long", b"exactly15chars!", b"a", b"timed2"]
for n in names:
    os.symlink(os.path.join(d.encode(), b"sleeper"), os.path.join(d.encode(), b"x", n))
PY

cat >"$fix/fix.py" <<'PY'
"""PID 1 of a fresh pid namespace: build a tree, run pstree, exit with its
status. See pstree-diff.sh."""
import ctypes, fcntl, os, signal, struct, sys, termios, threading

libc = ctypes.CDLL(None, use_errno=True)
PR_SET_NAME, PR_SET_DUMPABLE = 15, 4
XDIR = os.path.join(os.path.dirname(os.path.abspath(__file__)).encode(), b"x")


def set_name(name):
    libc.prctl(PR_SET_NAME, ctypes.c_char_p(name), 0, 0, 0)


class P:
    """A forked process: named, with children, threads, a process group, a
    uid, namespaces of its own -- or a zombie."""
    def __init__(self, name, *kids, threads=(), pgrp=False, uid=None, unshare=0, zombie=False):
        self.name, self.kids, self.threads = name, kids, threads
        self.pgrp, self.uid, self.unshare, self.zombie = pgrp, uid, unshare, zombie


class X:
    """An exec'd sleeper: `name` its comm, `argv` its command line."""
    def __init__(self, name, argv):
        self.name, self.argv = name, argv


def spawn(node):
    r, w = os.pipe2(os.O_CLOEXEC)
    pid = os.fork()
    if pid == 0:
        try:
            os.close(r)
            if isinstance(node, X):
                os.execv(os.path.join(XDIR, node.name), node.argv)
            if node.zombie:
                set_name(node.name)
                os._exit(0)
            build(node, w)
        finally:
            os._exit(127)
    os.close(w)
    # A byte from a process that is ready, or end-of-file from one that has
    # exec'd (the pipe is close-on-exec) or exited.
    os.read(r, 1)
    os.close(r)
    if isinstance(node, P) and node.zombie:
        os.waitid(os.P_PID, pid, os.WEXITED | os.WNOWAIT)


def build(node, w):
    if node.unshare:
        os.unshare(node.unshare)
    if node.pgrp:
        os.setpgid(0, 0)
    if node.uid is not None:
        os.setgroups([])
        os.setgid(node.uid)
        os.setuid(node.uid)
        # Keep /proc/PID owned by the uid, not by root.
        libc.prctl(PR_SET_DUMPABLE, 1, 0, 0, 0)
    set_name(node.name)
    for kid in node.kids:
        spawn(kid)
    stop = threading.Event()
    for tname in node.threads:
        started = threading.Event()

        def run(tname=tname, started=started):
            if tname is not None:
                set_name(tname)
            started.set()
            stop.wait()
        threading.Thread(target=run, daemon=True).start()
        started.wait()
    os.write(w, b"r")
    while True:
        signal.pause()


def worker(argv):
    return X(argv[0], argv)


SCENARIOS = {
    "single": [],
    "rich": [
        P(b"alpha",
          worker([b"worker", b"--id", b"1"]),
          worker([b"worker", b"--id", b"1"]),
          worker([b"worker", b"--id", b"1"]),
          P(b"group", worker([b"leaf"])),
          P(b"group", worker([b"leaf"])),
          worker([b"worker", b"--id", b"2"])),
        P(b"beta", threads=(b"thr", b"thr", b"thr", None)),
        P(b"gamma", P(b"zomb", zombie=True), P(b"zomb", zombie=True), P(b"zomb2", zombie=True)),
        P(b"names",
          X(b"sp ace", [b"sp ace", b"two words", b""]),
          X(b"par(en)s", [b"par(en)s", b")", b"(("]),
          X(b"back\\slash", [b"back\\slash", b"a\\b"]),
          X(b"na\xc3\xafve", [b"na\xc3\xafve", b"caf\xc3\xa9"]),
          X(b"bad\xff\xfe", [b"bad\xff\xfe", b"\xff"]),
          X(b"ctl\x01x", [b"ctl\x01x", b"tab\there", b"nl\nhere"]),
          X(b"empties", [b"empties", b"", b"", b"x", b""]),
          X(b"long", [b"long"] + [b"argument%02d" % i for i in range(30)]),
          X(b"exactly15chars!", [b"exactly15chars!"]),
          X(b"a", [b"a", b"b"])),
        P(b"pgrp", worker([b"member"]), pgrp=True),
        P(b"spaces",
          P(b"uts", worker([b"inside"]), unshare=os.CLONE_NEWUTS),
          P(b"net", unshare=os.CLONE_NEWNET),
          P(b"ipcmnt", unshare=os.CLONE_NEWIPC | os.CLONE_NEWNS),
          P(b"pidns", P(b"init2", worker([b"sub"])), unshare=os.CLONE_NEWPID),
          P(b"cg", unshare=os.CLONE_NEWCGROUP),
          P(b"tns", P(b"timed"), worker([b"timed2"]), unshare=os.CLONE_NEWTIME)),
        P(b"users",
          P(b"asuser", worker([b"child"]), uid=1000),
          P(b"asdaemon", uid=1),
          P(b"asnobody", uid=4242),
          P(b"asroot", uid=0)),
    ],
    "deep": [P(b"d00", P(b"d01", P(b"d02", P(b"d03", P(b"d04", P(b"d05", P(b"d06", P(b"d07",
             P(b"d08", P(b"d09", P(b"d10", P(b"d11", P(b"d12", P(b"d13", P(b"d14", P(b"d15",
             worker([b"leaf", b"at", b"the", b"bottom"])),
             P(b"d15", worker([b"leaf"])))))))))))))))))],
    "wide": [P(b"w", *[P(b"x%02d" % i) for i in range(40)],
               *[P(b"same", P(b"kid", threads=(None, None))) for _ in range(3)],
               P(b"same", P(b"kid", threads=(None,))))],
}


def main():
    scenario, args = sys.argv[1], sys.argv[2:]
    env = {k: v for k, v in os.environ.items() if not k.startswith("FIX_")}
    out_fd, err_fd = os.dup(1), os.dup(2)
    devnull = os.open("/dev/null", os.O_RDWR)
    for fd in (0, 1, 2):
        os.dup2(devnull, fd)
    for node in SCENARIOS[scenario]:
        spawn(node)
    cols = os.environ.get("FIX_PTY")
    master = slave = None
    if cols:
        master, slave = os.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, int(cols), 0, 0))
    argv0 = os.environ.get("FIX_ARGV0", "pstree")
    stdout = os.environ.get("FIX_STDOUT", "")
    pid = os.fork()
    if pid == 0:
        if slave is not None:
            os.dup2(slave, 1)
        elif stdout == "closed":
            os.close(1)
        elif stdout == "full":
            os.dup2(os.open("/dev/full", os.O_WRONLY), 1)
        else:
            os.dup2(out_fd, 1)
        os.dup2(err_fd, 2)
        os.closerange(3, 1024)
        os.execve(os.environ["FIX_BIN"], [argv0] + args, env)
    if master is not None:
        os.close(slave)
        while True:
            try:
                chunk = os.read(master, 65536)
            except OSError:
                break
            if not chunk:
                break
            os.write(out_fd, chunk)
    _, status = os.waitpid(pid, 0)
    code = os.waitstatus_to_exitcode(status)
    os._exit(code if code >= 0 else 128 - code)


main()
PY

# --- knobs ------------------------------------------------------------------
# SCEN: the tree. PTY: run on a terminal this many columns wide. LOCALE: the
# locale. ENVS: the environment's extra entries. ARGV0: the name it is run
# as. STDOUT: closed or full.
SCEN=rich; PTY=; LOCALE=C.UTF-8; ENVS=(); ARGV0=; STDOUT=
reset_knobs() { SCEN=rich; PTY=; LOCALE=C.UTF-8; ENVS=(); ARGV0=; STDOUT=; }

run_side() {
  local side=$1; shift
  local -a envs=(env -i "PATH=/usr/bin:/bin" "LC_ALL=$LOCALE" "${ENVS[@]}"
                 "FIX_BIN=$bindir/$side/pstree" "FIX_PTY=$PTY" "FIX_STDOUT=$STDOUT")
  [ -n "$ARGV0" ] && envs+=("FIX_ARGV0=$ARGV0")
  diff_run timeout -k 2 30 unshare --map-auto --map-root-user -pf --mount-proc \
    "${envs[@]}" python3 "$fix/fix.py" "$SCEN" "$@" </dev/null
}

# Namespace inode numbers, renumbered in order of appearance.
renumber() {
  python3 - "$1" <<'PY'
import re, sys
p = sys.argv[1]
data = open(p, "rb").read()
seen = {}
def sub(m):
    return seen.setdefault(m.group(0), b"[NS%d]" % (len(seen) + 1))
open(p, "wb").write(re.sub(rb"\[\d{6,}\]", sub, data))
PY
}

compare() {
  run_side ours "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side gnu "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  renumber "$DIFF_TMP/o.out"; renumber "$DIFF_TMP/g.out"
  LABEL="pstree $* [$SCEN]"
  [ -n "$PTY" ] && LABEL="$LABEL pty=$PTY"
  [ "$LOCALE" != C.UTF-8 ] && LABEL="$LABEL LC_ALL=$LOCALE"
  [ "${#ENVS[@]}" -gt 0 ] && LABEL="$LABEL [${ENVS[*]:0:3}]"
  [ -n "$ARGV0" ] && LABEL="$LABEL as $ARGV0"
  [ -n "$STDOUT" ] && LABEL="$LABEL stdout=$STDOUT"
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
    printf 'BROKEN %s -- never reached pstree on one or both sides\n%s\n' "$LABEL" "$REPORT"
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

# A guard against vacuous agreement: the reference must have drawn the tree.
run_case -p
if ! grep -q 'alpha(2)' "$DIFF_TMP/g.out" || ! grep -q -- '-child(' "$DIFF_TMP/g.out"; then
  echo "pstree-diff: the reference did not draw the fixture's tree:" >&2
  cat "$DIFF_TMP/g.out" "$DIFF_TMP/g.err" >&2
  exit 1
fi

# --- the tree's drawing, each way ----------------------------------------------
for scen in single rich deep wide; do
  for o in '' -a -c -p -n -g -t -T -u -A -U -G -l -Z -S -ap -ac -apt -acu -npg \
           -aZ -aS -pu -tT -cl -apl -at -aT -nT -pS; do
    # shellcheck disable=SC2086  # the options are words, on purpose
    SCEN=$scen; run_case $o
  done
done
for t in cgroup ipc mnt net pid user uts time; do
  run_case -N "$t"
  run_case -N "$t" -p
  run_case -N "$t" -a
  SCEN=deep; ENVS=(COLUMNS=30); run_case -N "$t"
done
run_case --ns-sort=uts -S

# --- the width ------------------------------------------------------------------
for c in 0 1 5 20 40 41 79 80 132 0x30 077 ' 40' '40 ' -5 abc 2147483646 2147483647 ''; do
  ENVS=("COLUMNS=$c"); run_case
  ENVS=("COLUMNS=$c"); run_case -a
  ENVS=("COLUMNS=$c"); run_case -p
  ENVS=("COLUMNS=$c"); SCEN=deep; run_case
  ENVS=("COLUMNS=$c"); SCEN=deep; run_case -a
done
ENVS=(COLUMNS=20); run_case -l
ENVS=(COLUMNS=20); run_case -al
for cols in 20 40 80 200; do
  PTY=$cols; run_case
  PTY=$cols; run_case -a
  PTY=$cols; SCEN=deep; run_case
  PTY=$cols; ENVS=(COLUMNS=30); run_case
done

# --- operands: a pid, a user, -s ------------------------------------------------
for p in 0 1 2 3 7 9 12 99999 -1; do
  run_case "$p"
  run_case -p "$p"
  run_case -s "$p"
  run_case -sp "$p"
  run_case -sa "$p"
done
for u in root inhahe daemon nobody nosuchuser '' 0; do
  run_case "$u"
  run_case -p "$u"
  run_case -u "$u"
done
run_case 12x
run_case 1 2
run_case -s
run_case -s root

# --- colour and highlight -----------------------------------------------------
run_case -C age
run_case --color=AGE -p
run_case -C age -a
run_case -C bogus
for term in xterm-256color xterm vt100 linux dumb ansi screen nosuch '' \
            zz-gen zz-realgen zz-hc zz-pad zz-mpad zz-nobold zz-sgr10 zz-strstr zz-pc zz-npc; do
  ENVS=("TERM=$term" "TERMINFO=$fix/terminfo"); run_case -h
  ENVS=("TERM=$term" "TERMINFO=$fix/terminfo"); run_case -h -a
  ENVS=("TERM=$term" "TERMINFO=$fix/terminfo"); run_case -H 2
  ENVS=("TERM=$term" "TERMINFO=$fix/terminfo"); run_case -H 9 -p
  ENVS=("TERM=$term" "TERMINFO=$fix/terminfo"); run_case -C age -H 2
done
run_case -H 2
run_case -h
ENVS=(TERM=xterm); run_case -H 0
ENVS=(TERM=xterm); run_case -H abc
ENVS=(TERM=xterm); run_case -H 2 -h
ENVS=(TERM=xterm); run_case -h -H 2
ENVS=(TERM=nosuch); run_case -h -H 2
ENVS=(TERM=xterm); run_case -hh
ENVS=("TERM=$(printf 'x%.0s' $(seq 600))"); run_case -H 2
ENVS=(TERM=xterm "TERMINFO_DIRS=$fix/terminfo"); run_case -H 2
ENVS=(TERM=zz-sgr10 "TERMINFO_DIRS=:$fix/terminfo"); run_case -H 2
ENVS=(TERM=zz-sgr10 HOME=/nonexistent); run_case -H 2

# --- on a terminal: the symbols, and setupterm's verdicts -----------------------
for loc in C.UTF-8 C POSIX C.utf8; do
  for term in xterm vt100 dumb nosuch '' zz-gen zz-realgen zz-hc zz-pad zz-mpad zz-pc zz-npc; do
    LOCALE=$loc; PTY=80; ENVS=("TERM=$term" "TERMINFO=$fix/terminfo"); run_case
    LOCALE=$loc; PTY=80; ENVS=("TERM=$term" "TERMINFO=$fix/terminfo"); run_case -U
    LOCALE=$loc; PTY=80; ENVS=("TERM=$term" "TERMINFO=$fix/terminfo"); run_case -h
  done
  LOCALE=$loc; PTY=80; run_case
  LOCALE=$loc; PTY=80; ENVS=("TERM=$(printf 'y%.0s' $(seq 600))"); run_case
done
# Padding: on the terminal, at its speed; through a pipe, at none.
PTY=80; ENVS=(TERM=zz-pad "TERMINFO=$fix/terminfo"); run_case -H 2 -p
PTY=80; ENVS=(TERM=zz-pc "TERMINFO=$fix/terminfo"); run_case -H 9 -a
PTY=80; ENVS=(TERM=vt100); run_case -h -c
ENVS=(TERM=zz-mpad "TERMINFO=$fix/terminfo"); run_case -h
ENVS=(TERM=zz-pc "TERMINFO=$fix/terminfo"); run_case -h

# --- the command line ---------------------------------------------------------
run_case -x
run_case --nosuch
run_case -C
run_case -H
run_case -N
run_case -N bogus
run_case --arg
run_case --c
run_case --compact
run_case --show-pids --show-pgids
run_case --numeric-sort --thread-names
run_case --hide-threads --uid-changes --ascii
run_case --vt100 --long --unicode
run_case --security-context --ns-changes
run_case --show-parents 3
run_case --highlight-all
run_case -- -p
run_case -p --
run_case 1 -p
xfail_case "our version string, not psmisc's" -- -V
xfail_case "our version string, not psmisc's" -- --version
ARGV0=pstree.x11; run_case
ARGV0=pstree.x11; run_case -p 2
ARGV0=pstree.x11; run_case -x
ARGV0=/usr/bin/pstree.x11; run_case 3
ARGV0=x11; run_case 3

# --- descriptors that cannot be written -----------------------------------------
STDOUT=closed; run_case
STDOUT=full; run_case
STDOUT=full; run_case -a
STDOUT=closed; run_case -x
STDOUT=full; SCEN=wide; run_case -c

printf '\n%d passed, %d differed, %d broken, %d differ on purpose' \
  "$pass" "$fail" "$broken" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ] && [ "$xpass" -eq 0 ]
