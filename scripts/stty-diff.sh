#!/usr/bin/env bash
# Differential test: our `stty` against GNU coreutils 9.4's.
#
# ## A fresh terminal per side, and what was done to it
#
# `stty` acts on the terminal on its standard input, so every case runs each
# side on a pseudo-terminal of its own (`ttyrun.py`, below), opened fresh with
# the same size and the kernel's default settings. What a side prints is half
# of the comparison; the other half is the terminal afterwards -- its whole
# `struct termios` as the kernel holds it, read with `TCGETS`, and its window
# size -- because most settings print nothing and are right or wrong only in
# what they leave behind.
#
# `@TTY@` in a case's arguments is the side's own terminal, for `-F`.
#
# ## Width
#
# Standard output is a pipe unless a case puts it on the terminal (`TTYOUT`),
# so the wrapping width is `COLUMNS` or 80, as upstream's `screen_columns`
# reads it; the cases on a terminal take it from the window size instead.
#
# ## Cases that differ on purpose
#
# The family's two: `--version`, and `--help`'s closing block of links, which
# is cut from GNU's output before it is compared.
set -u

DIFF_PROG='stty'
DIFF_GNU_SOURCE=9.4
DIFF_NEED="python3"
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0; broken=0
work=$DIFF_TMP/work
mkdir -p "$work"
case_no=0

# ---------------------------------------------------------------------------
# ttyrun.py: a command on a fresh pseudo-terminal.
#   ttyrun.py STATE ROWS COLS TTYOUT -- COMMAND...
# Standard input (and standard output, if TTYOUT is 1) is the terminal's
# slave; `@TTY@` in COMMAND is its name. Afterwards STATE gets the terminal's
# raw `struct termios` and its window size, and our exit status is the
# command's. What the command writes to a terminal standard output comes out
# on ours.
# ---------------------------------------------------------------------------
ttyrun=$DIFF_TMP/ttyrun.py
cat >"$ttyrun" <<'PY'
import fcntl, os, struct, subprocess, sys, termios

state, rows, cols, ttyout = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4] == "1"
cmd = sys.argv[sys.argv.index("--") + 1:]
master, slave = os.openpty()
name = os.ttyname(slave)
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
cmd = [os.fsencode(a).replace(b"@TTY@", os.fsencode(name)) for a in cmd]
captured = b""
if ttyout:
    p = subprocess.Popen(cmd, stdin=slave, stdout=slave)
    # Read until the command exits; the terminal stays open, so read what
    # is there rather than waiting for an end.
    import select
    while True:
        r, _, _ = select.select([master], [], [], 0.05)
        if r:
            captured += os.read(master, 65536)
        elif p.poll() is not None:
            r, _, _ = select.select([master], [], [], 0.05)
            if not r:
                break
    rc = p.wait()
else:
    rc = subprocess.run(cmd, stdin=slave).returncode
raw = fcntl.ioctl(slave, termios.TCGETS, b"\0" * 60)
ws = struct.unpack("HHHH", fcntl.ioctl(slave, termios.TIOCGWINSZ, b"\0" * 8))
with open(state, "w") as f:
    f.write(raw.hex() + " rows=%d cols=%d\n" % (ws[0], ws[1]))
sys.stdout.buffer.write(captured)
sys.stdout.flush()
sys.exit(rc)
PY

# --- knobs ------------------------------------------------------------------
COLS=-           # COLUMNS: `-` for unset
TTYOUT=0         # 1: standard output on the terminal too
ROWS=24; WIDTH=80
LOCALE=C.UTF-8
STDIN=           # `null`: standard input /dev/null rather than a terminal
reset_knobs() { COLS=-; TTYOUT=0; ROWS=24; WIDTH=80; LOCALE=C.UTF-8; STDIN=; }

# $1 = side, $2 = output prefix; the rest is stty's argv.
run_side() {
  local side=$1 p=$2; shift 2
  local -a envs=(env -u COLUMNS "LC_ALL=$LOCALE" "PATH=$bindir/$side:$PATH")
  [ "$COLS" != - ] && envs+=("COLUMNS=$COLS")
  if [ "$STDIN" = null ]; then
    diff_run timeout -k 5 30 "${envs[@]}" stty "$@" </dev/null >"$p.$side.out" 2>"$p.$side.err"
    echo $? >"$p.$side.rc"
    : >"$p.$side.state"
  else
    diff_run timeout -k 5 30 python3 "$ttyrun" "$p.$side.state" "$ROWS" "$WIDTH" "$TTYOUT" -- \
      "${envs[@]}" stty "$@" >"$p.$side.out" 2>"$p.$side.err"
    echo $? >"$p.$side.rc"
  fi
  return 0
}

compare() {
  case_no=$((case_no + 1))
  local p=$work/c$case_no
  run_side ours "$p" "$@"
  run_side gnu "$p" "$@"
  LABEL="stty $*"
  [ "$COLS" != - ] && LABEL="$LABEL [COLUMNS=$COLS]"
  [ "$TTYOUT" = 1 ] && LABEL="$LABEL [stdout on a ${WIDTH}-column terminal]"
  [ "$STDIN" = null ] && LABEL="$LABEL [stdin /dev/null]"
  [ "$LOCALE" != C.UTF-8 ] && LABEL="$LABEL [LC_ALL=$LOCALE]"
  reset_knobs
  local o_rc g_rc
  o_rc=$(cat "$p.ours.rc"); g_rc=$(cat "$p.gnu.rc")
  case "$o_rc $g_rc" in
    *124*|*127*) AGREED=broken; REPORT="  ours rc=$o_rc  gnu rc=$g_rc"; return 0 ;;
  esac
  if cmp -s "$p.ours.out" "$p.gnu.out" && cmp -s "$p.ours.err" "$p.gnu.err" \
     && cmp -s "$p.ours.state" "$p.gnu.state" && [ "$o_rc" = "$g_rc" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  --- ours rc=%s state %s\n%s\n  ~~~ stderr\n%s\n  --- gnu rc=%s state %s\n%s\n  ~~~ stderr\n%s' \
    "$o_rc" "$(cat "$p.ours.state")" "$(cat -A "$p.ours.out" | head -30)" "$(cat -A "$p.ours.err" | head -10)" \
    "$g_rc" "$(cat "$p.gnu.state")" "$(cat -A "$p.gnu.out" | head -30)" "$(cat -A "$p.gnu.err" | head -10)")
}

report() {
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1))
    printf 'BROKEN %s -- never reached stty on one or both sides\n%s\n' "$LABEL" "$REPORT"
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
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass + 1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$LABEL" "$why"
  elif [ "$AGREED" = broken ]; then
    broken=$((broken + 1))
    printf 'BROKEN %s\n%s\n' "$LABEL" "$REPORT"
  else
    xfail=$((xfail + 1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$LABEL" "$why"
  fi
  return 0
}

# ---------------------------------------------------------------------------
# The cases
# ---------------------------------------------------------------------------
# --- the three displays -------------------------------------------------------
run_case
# The terminal must start where the kernel starts one, and a setting must move
# it, or every case below compares two untouched terminals.
if ! grep -q 'rows=24 cols=80' "$work/c1.gnu.state" \
   || ! grep -q '^speed [0-9]* baud; line = 0;$' "$work/c1.gnu.out"; then
  echo "stty-diff: the reference's terminal is not the fixture one; the harness reaches nothing" >&2
  cat -A "$work/c1.gnu.out" "$work/c1.gnu.state" >&2
  exit 1
fi
fresh_state=$work/c1.gnu.state
run_case -a
run_case -g
run_case --all
run_case --save
run_case -F @TTY@
run_case -a -F @TTY@
run_case --file=@TTY@ -g
for c in 40 60 120 200 0 abc '' 0x50 -5 2147483648; do
  COLS=$c; run_case -a
done
COLS=50; run_case
TTYOUT=1; WIDTH=60; run_case -a
TTYOUT=1; WIDTH=132; run_case -a
TTYOUT=1; WIDTH=0; COLS=70; run_case -a
TTYOUT=1; run_case size
ROWS=0; WIDTH=0; run_case -a
ROWS=0; WIDTH=0; run_case size

# --- every flag and its reverse ----------------------------------------------
for f in parenb parodd cmspar cs5 cs6 cs7 cs8 hupcl hup cstopb cread clocal crtscts \
         ignbrk brkint ignpar parmrk inpck istrip inlcr igncr icrnl ixon ixoff tandem \
         iuclc ixany imaxbel iutf8 opost olcuc ocrnl onlcr onocr onlret ofill ofdel \
         nl1 nl0 cr3 cr2 cr1 cr0 tab3 tab2 tab1 tab0 bs1 bs0 vt1 vt0 ff1 ff0 \
         isig icanon iexten echo echoe crterase echok echonl noflsh xcase tostop \
         echoprt prterase echoctl ctlecho echoke crtkill flusho extproc; do
  run_case "$f"
  run_case "-$f"
  # A setting has to reach the terminal, or every case compares two
  # untouched ones: `-echo` must leave a state the fresh one is not.
  if [ "$f" = echo ] && cmp -s "$fresh_state" "$work/c$case_no.gnu.state"; then
    echo "stty-diff: the reference's -echo left the terminal as it found it; settings reach nothing" >&2
    exit 1
  fi
done
# Each change shows in the display of what differs from `sane`.
run_case -echo -icanon parenb cs7 ixany
run_case -echo -icanon parenb cs7 ixany -a

# --- combinations ---------------------------------------------------------------
for f in evenp parity oddp nl ek sane cooked raw pass8 litout cbreak decctlq tabs \
         lcase LCASE crt dec; do
  run_case "$f"
  run_case "-$f"
done
run_case raw -a
run_case -raw -a

# --- control characters, in every spelling -------------------------------------
for v in '^c' '^C' '^?' '^-' undef x '' 0x7f 0177 127 '^abc' 255 256 -1 0x 08 1b 1B; do
  run_case intr "$v"
done
for c in quit erase kill eof eol eol2 swtch start stop susp rprnt werase lnext flush discard; do
  run_case "$c" '^x'
done
run_case min 5
run_case time 10 -icanon
run_case min 300
run_case min x
run_case intr
run_case erase ^h kill ^u -a

# --- speeds ----------------------------------------------------------------------
for s in 0 50 134.5 9600 38400 exta extb 57600 115200 4000000 12345; do
  run_case "$s"
done
run_case ispeed 9600
run_case ospeed 9600
run_case ispeed 9600 ospeed 9600
run_case ispeed 9600 ospeed 38400
run_case ispeed 0
run_case ispeed foo
run_case ospeed
run_case speed
run_case 9600 speed

# --- the window --------------------------------------------------------------------
run_case rows 30
run_case cols 100
run_case columns 101
run_case rows 30 cols 100 size
run_case rows 0
run_case rows 65537
run_case rows x
run_case rows
run_case size

# --- line discipline ---------------------------------------------------------------
run_case line 0
run_case line 1
run_case line 300
run_case line x
run_case line

# --- -g's form, back -----------------------------------------------------------
run_case 500:5:bf:8a3b:3:1c:7f:15:4:0:1:0:11:13:1a:0:12:f:17:16:0:0:0:0:0:0:0:0:0:0:0:0:0:0:0:0
run_case 500:5:bf:8a3b:3:1c:7f:15:4:0:1:0:11:13:1a:0:12:f:17:16:0:0:0:0:0:0:0:0:0:0:0:0:0:0:0
run_case 500:5:bf:8a3b:3:1c:7f:15:4:0:1:0:11:13:1a:0:12:f:17:16:0:0:0:0:0:0:0:0:0:0:0:0:0:0:0:100
run_case 0x500:5:bf:8a3b:3:1c:7f:15:4:0:1:0:11:13:1a:0:12:f:17:16:0:0:0:0:0:0:0:0:0:0:0:0:0:0:0:0
run_case 100000000:5:bf:8a3b:3:1c:7f:15:4:0:1:0:11:13:1a:0:12:f:17:16:0:0:0:0:0:0:0:0:0:0:0:0:0:0:0:0

# --- drain ---------------------------------------------------------------------------
run_case drain
run_case -drain
run_case -drain echo
run_case drain -a

# --- the command line ----------------------------------------------------------------
run_case -a -g
run_case -a echo
run_case -g -echo
run_case -aecho
run_case -F @TTY@ -F @TTY@
run_case -F
run_case --file
run_case -F /nonexistent/tty
run_case -F /dev/null
run_case -- echo
run_case echo -- -echo
run_case -
run_case --foo
run_case -x
run_case frobnicate
run_case -sane
run_case ---debug echo
run_case echo -F @TTY@ -icanon
STDIN=null; run_case
STDIN=null; run_case -a
STDIN=null; run_case echo
STDIN=null; run_case frobnicate

# --- help and version ---------------------------------------------------------------
# GNU's help ends with a block of links the family leaves out: cut it from the
# reference before comparing.
compare --help
sed -i -e '/^GNU coreutils online help:/,$d' "$work/c$case_no.gnu.out"
sed -i -e '${/^$/d}' "$work/c$case_no.gnu.out"
if cmp -s "$work/c$case_no.ours.out" "$work/c$case_no.gnu.out"; then AGREED=yes; else AGREED=no; fi
LABEL="stty --help (less GNU's links)"
REPORT=$(diff "$work/c$case_no.ours.out" "$work/c$case_no.gnu.out" | head -20)
report
xfail_case "our version string" --version

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
[ "$broken" -gt 0 ] && printf ', %d BROKEN (never reached the subject)' "$broken"
printf '\n'
[ -n "${DIFF_SKIPPED:-}" ] && printf 'skipped:%s\n' "$DIFF_SKIPPED"
[ "$fail" = 0 ] && [ "$xpass" = 0 ] && [ "$broken" = 0 ]
