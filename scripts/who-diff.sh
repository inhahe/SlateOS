#!/usr/bin/env bash
# Differential test: our `who` against GNU coreutils 9.4's.
#
# ## Fixture login records
#
# Most cases read a utmp file this harness writes (`mkutmp.py`, below, in
# glibc's 384-byte x86-64 layout): every kind of entry `who` knows, names and
# lines that fill their fields to the last byte, a dead process's exit status,
# a run-level entry's two levels packed into its pid, an X display in a host.
# The live `/var/run/utmp` is read too, by the cases that ask for it, and
# compared as it stands.
#
# ## Terminals with a known state
#
# Idle time and the message status come from the terminal device named by an
# entry's line. A line may be an absolute path, so the fixtures name files in
# a short directory of this harness's own with chosen modes and access times
# -- under a minute, hours, days, the future -- re-set before every case, so
# that the time a case takes cannot move an idle time across a minute.
#
# ## `who -m` and `who am i`
#
# Both look for the entry of the terminal on standard input, so those cases
# run under a pseudo-terminal (`pty.py`, below) and write that terminal's
# name into the fixture first.
#
# ## Cases that differ on purpose
#
# The family's two (`--help`'s link block, `--version`), and a FILE that
# cannot be read: GNU on glibc reads through `getutxent`, which cannot say
# so, and prints nothing; this says `who: FILE: <reason>` and exits 1, as
# `users` and `pinky` do -- see `coreutils::utmp`.
set -u

DIFF_PROG='who'
DIFF_GNU_SOURCE=9.4
DIFF_NEED="python3"
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0
work=$DIFF_TMP/work
mkdir -p "$work"
case_no=0
export TZ=UTC

# A short directory for the fixture terminals: a utmp line holds 32 bytes.
ttys=/tmp/lbw.$$
mkdir -p "$ttys"
trap 'rm -rf "$ttys"; diff_cleanup' EXIT

# ---------------------------------------------------------------------------
# mkutmp.py: utmp records from a spec, one per line:
#   type|pid|line|id|user|host|term|exit|session|sec|usec
# Empty fields are zero or empty; \xHH writes a byte.
# ---------------------------------------------------------------------------
mkutmp=$DIFF_TMP/mkutmp.py
cat >"$mkutmp" <<'PY'
import struct, sys

def field(text):
    return text.encode("utf-8").decode("unicode_escape").encode("latin-1")

def num(s):
    return int(s) if s.strip() else 0

data = b""
for raw in sys.stdin.read().splitlines():
    if not raw.strip() or raw.startswith("#"):
        continue
    p = (raw.split("|") + [""] * 11)[:11]
    rec = struct.pack("<hxxi32s4s32s256shhiii16s20s",
                      num(p[0]), num(p[1]), field(p[2])[:32], field(p[3])[:4],
                      field(p[4])[:32], field(p[5])[:256], num(p[6]), num(p[7]),
                      num(p[8]), num(p[9]), num(p[10]), b"\0" * 16, b"\0" * 20)
    assert len(rec) == 384
    data += rec
sys.stdout.buffer.write(data)
PY

# ---------------------------------------------------------------------------
# pty.py: run a command with a pseudo-terminal on standard input, after
# writing that terminal's name (less /dev/) over @TTY@ in a fixture spec.
#   python3 pty.py SPEC OUTFILE -- command...
# ---------------------------------------------------------------------------
ptypy=$DIFF_TMP/pty.py
cat >"$ptypy" <<'PY'
import os, subprocess, sys
spec, utmp = sys.argv[1], sys.argv[2]
argv = sys.argv[sys.argv.index("--") + 1:]
master, slave = os.openpty()
name = os.ttyname(slave)
line = name[len("/dev/"):] if name.startswith("/dev/") else name
with open(spec) as f:
    text = f.read().replace("@TTY@", line)
with open(utmp, "wb") as f:
    f.write(subprocess.run([sys.executable, os.environ["MKUTMP"]], input=text.encode(),
                           stdout=subprocess.PIPE, check=True).stdout)
r = subprocess.run(argv, stdin=slave, stdout=sys.stdout, stderr=sys.stderr)
sys.exit(r.returncode)
PY
export MKUTMP=$mkutmp

# ---------------------------------------------------------------------------
# The fixtures
# ---------------------------------------------------------------------------
# Terminals: re-set before every case (`set_ttys`), relative to now.
set_ttys() {
  local now
  now=$(date +%s)
  : >"$ttys/a"; chmod 0620 "$ttys/a"; touch -a -d "@$((now - 30))" "$ttys/a"
  : >"$ttys/b"; chmod 0600 "$ttys/b"; touch -a -d "@$((now - 11250))" "$ttys/b"
  : >"$ttys/c"; chmod 0620 "$ttys/c"; touch -a -d "@$((now - 200000))" "$ttys/c"
  : >"$ttys/d"; chmod 0644 "$ttys/d"; touch -a -d "@$((now + 3600))" "$ttys/d"
  : >"$ttys/e"; chmod 0660 "$ttys/e"; touch -a -d "@$((now - 90))" "$ttys/e"
}
set_ttys

mk() { python3 "$mkutmp" >"$work/$1"; }

# Every kind of entry, as `who -a` sees them.
mk all <<SPEC
2|0|~|~~|reboot|5.15.0|0|0|0|1700000000|0
1|20021|~|~~|runlevel|5.15.0|0|0|0|1700000005|0
5|99|tty9|9||||||1700000050|0
6|101|tty2|2|LOGIN||0|0|0|1700000100|0
7|102|$ttys/a|ts/0|alice|10.0.0.2|0|0|0|1700000200|0
7|103|$ttys/b|ts/1|bob|localhost:0|0|0|0|1700000250|0
7|104|nosuchtty|ts/9|carol||0|0|0|1700000260|0
7|105|$ttys/c|ts/3|dave|example.invalid|0|0|0|1700000270|0
7|106|$ttys/d|ts/4|erin|:1|0|0|0|1700000280|0
8|107|pts/2|ts/2|||1|2|0|1700000300|0
8|108|pts/5|ts/5|ghost||-1|-2|0|1700000310|0
3|0|||||0|0|0|1700000400|0
4|0|||||0|0|0|1700000390|0
7|109|pts/3|ts/3|||0|0|0|1700000500|0
SPEC

# A boot entry between two sessions on the same terminal: idle is measured
# against the last boot entry *seen so far*, which is upstream's order. The
# boot is dated a day ahead, so the second session's terminal predates it and
# reads ` old ` however long a case takes, while the first reads its minute.
mk bootlate <<SPEC
7|102|$ttys/e|ts/0|alice|||||1700000200|0
2|0|~|~~|reboot|||||$(( $(date +%s) + 86400 ))|0
7|103|$ttys/e|ts/1|bob|||||1700000250|0
SPEC

# Fields filled to their last byte, and names with trailing spaces.
mk full <<SPEC
7|200|0123456789abcdefghijklmnopqrstuv|wxyz|ABCDEFGHIJKLMNOPQRSTUVWXYZ012345|h|0|0|0|1700000000|0
7|201|pts/7|ts/7|spaced   |||||1700000000|0
7|202|pts/8|ts/8|  lead|||||1700000000|0
7|-5|pts/9|ts/9|negpid|||||1700000000|0
7|2147483647|pts/10|t/10|bigpid|||||1700000000|0
7|203|pts/11|t/11|sp ace|||||1700000000|0
7|204|tty 4|t/12|line sp|||||1700000000|0
7|205|pts/12|t/13|epoch|||||0|0
7|206|pts/13|t/14|before|||||-86400|0
7|207|pts/14|t/15|late|||||2147483647|0
7|208|pts/15|t/16|\\xffbyte|h\\xffost||||1700000000|0
SPEC

# Run levels, packed into the pid as previous * 256 + current.
mk runlevels <<SPEC
1|$((78*256 + 53))|~|~~|runlevel|||||1700000000|0
1|$((51*256 + 53))|~|~~|runlevel|||||1700000060|0
1|$((5*256 + 51))|~|~~|runlevel|||||1700000120|0
1|$((83*256))|~|~~|runlevel|||||1700000180|0
1|-1|~|~~|runlevel|||||1700000240|0
SPEC

# Nothing at all, and a torn record at the end.
: >"$work/empty"
cp "$work/all" "$work/torn"; printf 'partial' >>"$work/torn"
# A FILE that cannot be read.
: >"$work/unreadable"; chmod 000 "$work/unreadable"

# --- knobs -------------------------------------------------------------------
KIND=
LOCALE=
reset_knobs() { KIND=; LOCALE=; }

classify() {
  local first
  first=$(head -c 200 "$1" | head -1)
  if [ ! -s "$1" ]; then echo empty
  elif [ "${first#Usage: who }" != "$first" ]; then echo help
  elif [ "${first#who \(}" != "$first" ]; then echo version
  else echo other
  fi
}

run_direct() {
  local side=$1 out=$2 err=$3 rcf=$4; shift 4
  set_ttys
  ( cd "$work" && LC_ALL=${LOCALE:-C.UTF-8} PATH="$bindir/$side:$PATH" \
      timeout -k 5 60 who "$@" </dev/null >"$out" 2>"$err" )
  echo $? >"$rcf"
  return 0
}

judge() {
  local o_out=$1 g_out=$2 o_err=$3 g_err=$4 o_rc=$5 g_rc=$6 label=$7
  local o_show g_show o_e g_e o_r g_r
  if [ -n "$KIND" ]; then
    o_show="class $(classify "$o_out")"; g_show="class $(classify "$g_out")"
  else
    o_show=$(cat "$o_out"); g_show=$(cat "$g_out")
  fi
  o_e=$(cat "$o_err"); g_e=$(cat "$g_err")
  o_r=$(cat "$o_rc"); g_r=$(cat "$g_rc")
  if [ "$o_show" = "$g_show" ] && [ "$o_e" = "$g_e" ] && [ "$o_r" = "$g_r" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours: rc=%s out{%s} err{%s}\n  gnu : rc=%s out{%s} err{%s}' \
    "$o_r" "$(printf '%s' "$o_show" | tr '\n' '|')" "$(printf '%s' "$o_e" | tr '\n' '|')" \
    "$g_r" "$(printf '%s' "$g_show" | tr '\n' '|')" "$(printf '%s' "$g_e" | tr '\n' '|')")
  LABEL=$label
}

compare_direct() {
  case_no=$((case_no+1))
  local p=$work/c$case_no
  run_direct ours "$p.oo" "$p.oe" "$p.or" "$@"
  run_direct gnu  "$p.go" "$p.ge" "$p.gr" "$@"
  judge "$p.oo" "$p.go" "$p.oe" "$p.ge" "$p.or" "$p.gr" "${LOCALE:+[LC_ALL=$LOCALE] }who $*"
  reset_knobs
}

report() {
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$LABEL"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$LABEL" "$REPORT"
  fi
  return 0
}

run_case() { compare_direct "$@"; report; }

xfail_case() {
  local why="$1"; shift
  compare_direct "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$LABEL" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$LABEL" "$why"
  fi
  return 0
}

# A pty on standard input; SPEC is a fixture spec with @TTY@ for its name.
pty_case() {
  local spec=$1; shift
  case_no=$((case_no+1))
  local p=$work/c$case_no
  printf '%s\n' "$spec" >"$p.spec"
  for side in ours gnu; do
    set_ttys
    ( cd "$work" && LC_ALL=C.UTF-8 PATH="$bindir/$side:$PATH" timeout -k 5 60 \
        python3 "$ptypy" "$p.spec" "$p.utmp" -- who "$@" ) \
      >"$p.${side:0:1}o" 2>"$p.${side:0:1}e"
    echo $? >"$p.${side:0:1}r"
  done
  judge "$p.oo" "$p.go" "$p.oe" "$p.ge" "$p.or" "$p.gr" "[pty] who $*"
  report
}

echo "who-diff:"
echo "  ours: $OURS"
echo "  gnu:  $gnu_real"

# =============================================================================
# 1. Each option, over every kind of entry
# =============================================================================

for opts in "" -a -b -d -H -l -p -q -r -s -t -T -w -u --lookup -aH -bH -dH -lH -pH -qH \
            -rH -tH -uH -TH -uT -us -ds -as -ab -bdlprtu -q -qa -qH -m; do
  # shellcheck disable=SC2086
  run_case $opts all
done
for opts in --all --boot --count --dead --heading --login --process --runlevel --short \
            --time --users --mesg --message --writable --me --w --hea --pro --ru; do
  run_case "$opts" all
done
LOCALE=C; run_case -a all
LOCALE=C; run_case all
LOCALE=POSIX; run_case -b all
LOCALE=C.UTF-8; run_case -a all

# =============================================================================
# 2. Boot entries and idle time
# =============================================================================

run_case -u bootlate
run_case -a bootlate
run_case -T bootlate

# =============================================================================
# 3. Fields to their last byte, odd names, odd times
# =============================================================================

run_case full
run_case -a full
run_case -q full
run_case -u full
run_case --lookup full
run_case -r runlevels
run_case -a runlevels
run_case runlevels
run_case -a empty
run_case -q empty
run_case -a torn
run_case -q torn

# =============================================================================
# 4. The live utmp
# =============================================================================

run_case
run_case -b
run_case -q
run_case -r
run_case -H
run_case -T
run_case -bH
run_case am i
run_case mom likes
run_case -m
run_case /var/run/utmp
run_case -b /var/run/utmp

# =============================================================================
# 5. The command line
# =============================================================================

run_case a b c
run_case -x
run_case --bogus
run_case --l
run_case --lo
run_case --m
KIND=1; run_case --help
KIND=1; run_case --he
KIND=1; run_case --v
KIND=1; run_case --ver
KIND=1; run_case -a --help
xfail_case 'help omits the GNU project link block' --help
xfail_case 'version names SlateOS' --version
xfail_case 'a FILE that cannot be read is reported' /nonexistent
xfail_case 'a FILE that cannot be read is reported' "$work/unreadable"
xfail_case 'a FILE that cannot be read is reported' /tmp
xfail_case 'a FILE that cannot be read is reported' -q /nonexistent

# =============================================================================
# 6. The terminal on standard input
# =============================================================================

pty_case '7|300|@TTY@|ts/0|me|somehost|||||1700000000|0
7|301|pts/999|ts/9|other|||||1700000000|0' -m "$work/c$((case_no+1)).utmp"
pty_case '7|300|@TTY@|ts/0|me||||||1700000000|0
7|302|@TTY@|ts/1|again||||||1700000060|0' -mH "$work/c$((case_no+1)).utmp"
pty_case '7|301|pts/999|ts/9|other|||||1700000000|0' -m "$work/c$((case_no+1)).utmp"

# =============================================================================
# 7. Descriptors closed or full
# =============================================================================

sh_case() {
  case_no=$((case_no+1))
  local p=$work/c$case_no
  for side in ours gnu; do
    set_ttys
    ( cd "$work" && LC_ALL=C.UTF-8 PATH="$bindir/$side:$PATH" timeout -k 5 60 \
        bash -c "$1"'; echo $? >&9' </dev/null ) \
      >"$p.${side:0:1}o" 2>"$p.${side:0:1}e" 9>"$p.${side:0:1}r"
  done
  judge "$p.oo" "$p.go" "$p.oe" "$p.ge" "$p.or" "$p.gr" "[bash] $1"
  report
}
sh_case 'who all >&-'
sh_case 'who all >/dev/full'
sh_case 'who empty >&-'
sh_case 'who -q all >/dev/full'
sh_case 'who a b c 2>&-'
sh_case 'who --help >/dev/full'

# =============================================================================
# Summary
# =============================================================================
total=$((pass+fail+xfail+xpass))
printf 'who          %d case(s): %d passed, %d differed, %d differ on purpose, %d unexpectedly agreed\n' \
  "$total" "$pass" "$fail" "$xfail" "$xpass"
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
