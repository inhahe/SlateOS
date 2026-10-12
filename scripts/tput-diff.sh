#!/usr/bin/env bash
# Differential test: ncurses 6.4's `tput`, `clear`, `tset` and `tabs`, ours against Ubuntu's.
#
# The reference is Ubuntu 24.04's 6.4+20240113, in `ncurses-bin`; `reset` is
# `tset` by another name, and `tput` answers to `clear`, `init` and `reset`
# too.
#
# ## A fresh terminal per side, and what was done to it
#
# All four act on a terminal, so every case runs each side on a
# pseudo-terminal of its own (`ttyrun.py`, below): opened fresh, given the
# case's window size, output speed and settings, with the case's choice of
# standard descriptors on it -- the rest go to files, to `/dev/full`, or
# nowhere -- and, when a case asks, as the controlling terminal, so that
# `/dev/tty` reaches it. What is compared: everything written to the
# terminal, standard output and error where they were files, the exit
# status, and the terminal afterwards -- its whole `struct termios` and its
# window size -- since `tset`, `tput reset` and `tabs` are right or wrong
# mostly in what they leave behind.
#
# ## The terminals
#
# The system's entries (`xterm`, `xterm-256color`, `vt100`, `dumb`), and
# crafted ones compiled with `tic -x` into a directory named by `TERMINFO`,
# for what the system's do not exercise: padding at the line's speed with
# and without a pad character and with `npc`; every init and reset string,
# init and reset files and program; the four ways of setting margins, for
# `tset` and for `tabs +m`; tab stops of every spacing, and a terminal whose
# `tbc` is not ANSI's; generic and hard-copy terminals; parameters that are
# strings, standard and extended; `tparm`'s static variables; a command
# character with `CC`, which reaches what the programs read as macros and
# not what `tigetstr` answers; and numbers past a `short`.
#
# Each side runs with an environment of only what the case gives it, `HOME`
# somewhere empty and `PATH` its own directory, so that what is found is the
# same on both sides.
#
# ## Cases that differ on purpose
#
# `-V`, which names this build rather than the ncurses version.
set -u

DIFF_PROG='tput'
DIFF_BINS='tput clear tset tabs'
DIFF_NEED='python3 tic timeout'
# A thousand cases, a few hundred of which settle the terminal for a second
# on each side, as `tset` does after its init strings.
DIFF_TIMEOUT=${DIFF_TIMEOUT:-3600}
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0; broken=0
work=$DIFF_TMP/work
fix=$DIFF_TMP/fix
mkdir -p "$work" "$fix/terminfo" "$fix/home"
case_no=0

# `reset` is `tset` by another name; `tput` answers to three others.
for side in ours gnu; do
  ln -s "$(readlink -f "$bindir/$side/tset")" "$bindir/$side/reset"
  mkdir -p "$DIFF_TMP/alias/$side"
  for a in clear init reset; do
    ln -s "$(readlink -f "$bindir/$side/tput")" "$DIFF_TMP/alias/$side/$a"
  done
done

# --- the crafted terminals ------------------------------------------------------
printf '<initfile>\n' >"$fix/initfile"
# Past `BUFSIZ`, so that copying it to a full standard output fails on the way.
head -c 10000 /dev/zero | tr '\0' 'x' >"$fix/bigfile"
printf '<resetfile>\n' >"$fix/resetfile"
printf '#!/bin/sh\necho "<iprog TERMCAP=${TERMCAP-unset}>"\n' >"$fix/iprog.sh"
chmod 755 "$fix/iprog.sh"
cat >"$fix/terminals.ti.in" <<'TI'
zz-pad|padded,
	am, cols#20, lines#5,
	bold=\E[1m$<5>, clear=\E[H\E[2J$<3*>, el=\E[K$<1.5>,
	flash=\E[?5h$<10/>\E[?5l, sgr0=\E[0m$<2*>,
zz-pc|padded with a pad character,
	pad=*, bold=\E[1m$<2>, clear=\E[H\E[J$<1*>,
zz-npc|padded by pausing,
	npc, bold=\E[1m$<20>, flash=\E[?5h$<50/>\E[?5l, sgr0=\E[m,
zz-init|every init string,
	cols#20, lines#6, it#4,
	cr=^M, hts=\EH, tbc=\E[3g,
	if=@FIX@/initfile, is1=<is1>, is2=<is2>, is3=<is3>,
	mgc=<mgc>, rf=@FIX@/resetfile, rs1=<rs1>, rs2=<rs2>,
	smglr=<lr%p1%d.%p2%d>,
zz-lr|margins by smglr,
	cols#20, it#8,
	is2=<is2>, smglr=\E[%i%p1%d;%p2%ds,
zz-lrp|margins by smglp and smgrp,
	cols#20,
	smglp=\E[%p1%dL, smgrp=\E[%p1%dR,
zz-lm|margins by smgl and smgr with cuf,
	cols#20,
	cr=\r, cuf=\E[%p1%dC, smgl=\E[L, smgr=\E[R,
zz-lms|margins by smgl and smgr with spaces,
	cols#10,
	smgl=\E[L, smgr=\E[R,
zz-tabs3|tabs every 3,
	cols#10, it#3,
	cr=^M, hts=\EH, tbc=\E[3g,
zz-tabs1|tabs every 1,
	cols#10, it#1,
	hts=\EH, tbc=\E[3g,
zz-tabs40|tabs wider than the screen,
	cols#20, it#40,
	hts=\EH, tbc=\E[3g,
zz-iprog|an init program,
	iprog=@FIX@/iprog.sh, is2=<is2>, rs2=<rs2>,
zz-badif|an init file that is not there,
	if=@FIX@/nosuch, is1=<is1>,
zz-os|overstriking with a one-byte backspace,
	os,
	kbs=^H,
zz-nel|newline as nel,
	is2=<is2>, nel=\n,
zz-noclear|no clear,
	bold=\E[1m,
zz-e3|clear and E3,
	E3=\E[3J, clear=\E[H\E[2J,
zz-gen|generic and nothing more,
	gn,
	bold=\E[1m,
zz-realgen|generic in name only,
	gn,
	bold=\E[1m, clear=\E[H\E[2J, cup=\E[%i%p1%d;%p2%dH,
zz-hc|hard copy,
	hc,
	bold=\E[1m, clear=\E[H\E[2J,
zz-str|string parameters,
	Cs=\E]12;%p1%s\007, Ms=\E]52;%p1%s;%p2%s\007,
	Xm=<Xm %p1%d %p2%s>, Xn=<Xn %p1%d %p2%d>, Xs=<Xs %p1%s>,
	pfkey=<pfkey %p1%d %p2%s>, pfloc=<pfloc %p1%d %p2%s>,
	pfx=<pfx %p1%d %p2%s>, pfxl=<pfxl %p1%d %p2%s %p3%s>,
	pln=<pln %p1%d %p2%s>, u1=<u1 %p1%s>,
zz-static|static variables,
	u0=<%gA%d %p1%PA>, u2=<%gB%d %p1%PB %p2%Pb %gb%d>,
	u3=<%gb%d %p1%Pb>,
zz-cmdch|a command character,
	cols#12, it#4,
	E3=@E3, bold=@[1m, clear=@[H, cmdch=@, cr=@r, el=x@y, hts=@H,
	is2=@is2, sgr0=@[m, tbc=@[3g,
zz-cmdos|a command character for a backspace,
	os,
	cmdch=@, kbs=@,
zz-big|numbers past a short,
	cols#40000, it#40000, lines#40000,
	hts=\EH, tbc=\E[3g,
zz-bigif|an init file past a buffer,
	if=@FIX@/bigfile,
zz-dirif|an init file that is a directory,
	if=@FIX@, is1=<is1>,
zz-tbc|tabs with a clear that is not ANSI's,
	cols#30,
	hts=\EH, tbc=\E3,
zz-csi|tabs with an eight-bit CSI,
	cols#20,
	hts=\EH, tbc=\2333g,
zz-nohts|a clear and no set,
	tbc=\E[3g,
zz-mgn|margins with smgl, mgc and hpa,
	cols#40,
	hpa=<hpa%p1%d>, hts=\EH, mgc=<mgc>, smgl=<smgl>, tbc=\E[3g,
zz-mgncuf|margins with smgl and cuf,
	cols#40,
	cuf=<cuf%p1%d>, hts=\EH, smgl=<smgl>, tbc=\E[3g,
zz-mgnsp|margins with smgl and spaces,
	cols#40,
	hts=\EH, smgl=<smgl>, tbc=\E[3g,
zz-mgnp|margins with smglp alone,
	cols#40,
	hts=\EH, smglp=<smglp%p1%d>, tbc=\E[3g,
zz-mgnp2|margins with a two-parameter smglp,
	cols#40,
	hts=\EH, smglp=<smglp%p1%d.%p2%d>, tbc=\E[3g,
zz-mgnpr|margins with smglp and smgrp,
	cols#40,
	hts=\EH, smglp=<smglp%p1%d>, smgrp=<smgrp%p1%d>, tbc=\E[3g,
zz-mgnlr|margins with smglr,
	cols#40,
	hts=\EH, smglr=<smglr%p1%d.%p2%d>, tbc=\E[3g,
TI
sed "s|@FIX@|$fix|g" "$fix/terminals.ti.in" >"$fix/terminals.ti"
tic -x -o "$fix/terminfo" "$fix/terminals.ti" 2>"$DIFF_TMP/tic.err" || {
  echo "tput-diff: tic failed:" >&2; cat "$DIFF_TMP/tic.err" >&2; exit 1
}

# ---------------------------------------------------------------------------
# ttyrun.py: a command on a fresh pseudo-terminal.
#   ttyrun.py STATE ROWS COLS FDS CTTY PRESET SPEED OUT ERR IN -- COMMAND...
# FDS lists the standard descriptors on the terminal (`012`, `2`, `-` for
# none); the others are OUT, ERR and IN -- a file, `-` for /dev/null,
# `full` for /dev/full, or `closed` for none at all.
# CTTY 1 makes the terminal the command's controlling terminal; otherwise
# it has none. PRESET and SPEED set the terminal up first. Afterwards STATE
# gets its raw `struct termios` and window size, what was written to it
# comes out on our standard output, and our status is the command's.
# ---------------------------------------------------------------------------
ttyrun=$DIFF_TMP/ttyrun.py
cat >"$ttyrun" <<'PY'
import fcntl, os, select, struct, subprocess, sys, termios

(state, rows, cols, fds, ctty, preset, speed, outp, errp, inp) = sys.argv[1:11]
cmd = sys.argv[sys.argv.index("--") + 1:]
master, slave = os.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", int(rows), int(cols), 0, 0))
t = termios.tcgetattr(slave)
iflag, oflag, cflag, lflag, ispeed, ospeed, cc = t
T = termios
if preset == "odd":
    cc[T.VERASE] = b"\x08"
    cc[T.VINTR] = b"\x7f"
elif preset == "disabled":
    for v in (T.VERASE, T.VINTR, T.VKILL, T.VQUIT, T.VEOF, T.VSUSP, T.VSTART,
              T.VSTOP, T.VLNEXT, T.VWERASE, T.VREPRINT, T.VDISCARD):
        cc[v] = b"\x00"
elif preset == "raw":
    iflag &= ~(T.IGNBRK | T.BRKINT | T.PARMRK | T.ISTRIP | T.INLCR | T.IGNCR | T.ICRNL | T.IXON)
    oflag &= ~T.OPOST
    lflag &= ~(T.ECHO | T.ECHONL | T.ICANON | T.ISIG | T.IEXTEN)
    cflag &= ~(T.CSIZE | T.PARENB)
    cflag |= T.CS8
elif preset == "weird":
    iflag |= T.IGNBRK | T.PARMRK | T.INPCK | T.ISTRIP | T.INLCR | T.IGNCR | T.IUCLC | T.IXANY | T.IXOFF
    iflag &= ~(T.BRKINT | T.ICRNL | T.IXON | T.IMAXBEL)
    oflag |= T.OLCUC | T.OCRNL | T.ONOCR | T.ONLRET | T.OFILL | T.OFDEL | T.NL1 | T.CR2 | T.TAB3 | T.BS1 | T.VT1 | T.FF1
    oflag &= ~(T.OPOST | T.ONLCR)
    cflag &= ~T.CSIZE
    cflag |= T.CS7 | T.CSTOPB | T.PARENB | T.PARODD | T.CLOCAL
    lflag |= T.ECHONL | T.NOFLSH | T.TOSTOP | T.ECHOPRT | T.XCASE
    lflag &= ~(T.ICANON | T.ECHO | T.ECHOE | T.ECHOK | T.ECHOCTL | T.ECHOKE | T.ISIG)
    cc[T.VERASE] = b"\x00"
    cc[T.VKILL] = b"x"
elif preset == "tab3":
    oflag |= T.TAB3
elif preset == "olcuc":
    oflag |= T.OLCUC | T.ONLRET
elif preset == "ocrnl":
    oflag |= T.OCRNL
elif preset != "fresh":
    sys.exit("ttyrun: no preset %r" % preset)
if speed != "-":
    ispeed = ospeed = getattr(T, "B" + speed)
termios.tcsetattr(slave, termios.TCSANOW, [iflag, oflag, cflag, lflag, ispeed, ospeed, cc])

closing = []
def target(fd, path, mode):
    if str(fd) in fds:
        return slave
    if path == "-":
        return open(os.devnull, mode)
    if path == "full":
        return open("/dev/full", mode)
    if path == "closed":
        closing.append(fd)
        return open(os.devnull, mode)
    return open(path, mode)

stdin = target(0, inp, "rb")
stdout = target(1, outp, "wb")
stderr = target(2, errp, "wb")

def become_session():
    if ctty == "1":
        fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
    for fd in closing:
        os.close(fd)

p = subprocess.Popen(cmd, stdin=stdin, stdout=stdout, stderr=stderr,
                     start_new_session=True, preexec_fn=become_session)
captured = b""
while True:
    r, _, _ = select.select([master], [], [], 0.05)
    if r:
        try:
            captured += os.read(master, 65536)
        except OSError:
            break
    elif p.poll() is not None:
        r, _, _ = select.select([master], [], [], 0.05)
        if not r:
            break
rc = p.wait()
raw = fcntl.ioctl(slave, termios.TCGETS, b"\0" * 60)
ws = struct.unpack("HHHH", fcntl.ioctl(slave, termios.TIOCGWINSZ, b"\0" * 8))
with open(state, "w") as f:
    f.write(raw.hex() + " rows=%d cols=%d\n" % (ws[0], ws[1]))
sys.stdout.buffer.write(captured)
sys.stdout.flush()
sys.exit(rc)
PY

# --- knobs ------------------------------------------------------------------
ROWS=24; WIDTH=80   # the window
FDS=012             # the standard descriptors on the terminal
CTTY=0              # 1: the terminal is the controlling one
PRESET=fresh        # the settings it starts with
SPEED=-             # its speed, or `-` for the kernel's
STDIN=              # text for a standard input that is not the terminal
ALIAS=              # run `tput` as `clear`, `init` or `reset`
ENVS=()             # the environment, beyond PATH and HOME
OUTTO=              # standard output off the terminal: `full` or `closed`
ERRTO=              # the same for standard error
reset_knobs() { ROWS=24; WIDTH=80; FDS=012; CTTY=0; PRESET=fresh; SPEED=-; STDIN=; ALIAS=; ENVS=(); OUTTO=; ERRTO=; }

# $1 = side, $2 = output prefix; the rest is the program and its argv.
run_side() {
  local side=$1 p=$2; shift 2
  local dir=$bindir/$side
  [ -n "$ALIAS" ] && dir=$DIFF_TMP/alias/$side
  local in=-
  if [ -n "$STDIN" ]; then
    printf '%b' "$STDIN" >"$p.in"
    in=$p.in
  fi
  diff_run timeout -k 5 30 python3 "$ttyrun" "$p.$side.state" "$ROWS" "$WIDTH" \
    "${FDS:--}" "$CTTY" "$PRESET" "$SPEED" "${OUTTO:-$p.$side.out}" "${ERRTO:-$p.$side.err}" "$in" -- \
    env -i "PATH=$dir" "HOME=$fix/home" "${ENVS[@]}" "$@" >"$p.$side.pty" 2>"$p.$side.runerr"
  echo $? >"$p.$side.rc"
  [ -f "$p.$side.out" ] || : >"$p.$side.out"
  [ -f "$p.$side.err" ] || : >"$p.$side.err"
  return 0
}

compare() {
  case_no=$((case_no + 1))
  local p=$work/c$case_no
  run_side ours "$p" "$@"
  run_side gnu "$p" "$@"
  LABEL="$*"
  [ ${#ENVS[@]} -gt 0 ] && LABEL="$LABEL [${ENVS[*]}]"
  [ "$FDS" != 012 ] && LABEL="$LABEL [on the terminal: ${FDS:-none}]"
  [ "$CTTY" = 1 ] && LABEL="$LABEL [controlling]"
  [ "$PRESET" != fresh ] && LABEL="$LABEL [$PRESET]"
  [ "$SPEED" != - ] && LABEL="$LABEL [${SPEED} baud]"
  [ "$ROWS $WIDTH" != "24 80" ] && LABEL="$LABEL [${ROWS}x$WIDTH]"
  [ -n "$STDIN" ] && LABEL="$LABEL [stdin $STDIN]"
  [ -n "$ALIAS" ] && LABEL="$LABEL [tput as $1]"
  [ -n "$OUTTO" ] && LABEL="$LABEL [stdout $OUTTO]"
  [ -n "$ERRTO" ] && LABEL="$LABEL [stderr $ERRTO]"
  reset_knobs
  local o_rc g_rc
  o_rc=$(cat "$p.ours.rc"); g_rc=$(cat "$p.gnu.rc")
  case "$o_rc $g_rc" in
    *124*|*127*) AGREED=broken
      REPORT="  ours rc=$o_rc  gnu rc=$g_rc
$(cat "$p.ours.runerr" "$p.gnu.runerr" | head -5)"
      return 0 ;;
  esac
  if cmp -s "$p.ours.pty" "$p.gnu.pty" && cmp -s "$p.ours.out" "$p.gnu.out" \
     && cmp -s "$p.ours.err" "$p.gnu.err" && cmp -s "$p.ours.state" "$p.gnu.state" \
     && [ "$o_rc" = "$g_rc" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  --- ours rc=%s state %s\n  ~~~ terminal\n%s\n  ~~~ stdout\n%s\n  ~~~ stderr\n%s\n  --- reference rc=%s state %s\n  ~~~ terminal\n%s\n  ~~~ stdout\n%s\n  ~~~ stderr\n%s' \
    "$o_rc" "$(cat "$p.ours.state")" "$(cat -A "$p.ours.pty" | head -20)" \
    "$(cat -A "$p.ours.out" | head -20)" "$(cat -A "$p.ours.err" | head -10)" \
    "$g_rc" "$(cat "$p.gnu.state")" "$(cat -A "$p.gnu.pty" | head -20)" \
    "$(cat -A "$p.gnu.out" | head -20)" "$(cat -A "$p.gnu.err" | head -10)")
}

report() {
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1))
    printf 'BROKEN %s -- never ran on one or both sides\n%s\n' "$LABEL" "$REPORT"
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

TI="TERMINFO=$fix/terminfo"

# ---------------------------------------------------------------------------
# The cases
# ---------------------------------------------------------------------------
# --- the harness reaches the terminal -------------------------------------------
ENVS=(TERM=xterm-256color); run_case tput cols
if ! grep -q 'rows=24 cols=80' "$work/c1.gnu.state" || [ "$(cat -A "$work/c1.gnu.pty")" != '80^M$' ]; then
  echo "tput-diff: the reference did not answer from the fixture terminal; the harness reaches nothing" >&2
  cat -A "$work/c1.gnu.pty" "$work/c1.gnu.state" >&2
  exit 1
fi

# --- capabilities: booleans, numbers, strings -------------------------------------
for term in xterm-256color xterm vt100 dumb linux; do
  for cap in cols lines colors it am bw km xenl npc bold sgr0 smcup rmcup el bel kbs \
             clear cup hpa longname flash cnorm civis cvvis smacs acsc nosuch '' Ms E3 XM; do
    ENVS=("TERM=$term"); run_case tput "$cap"
  done
done
# Off the terminal: the size is the entry's, padding goes, and `flash` waits.
for cap in cols lines bold flash clear; do
  FDS=; ENVS=(TERM=xterm); run_case tput "$cap"
  FDS=2; ENVS=(TERM=xterm); run_case tput "$cap"
  FDS=0; ENVS=(TERM=xterm); run_case tput "$cap"
done

# --- parameters -------------------------------------------------------------------
for args in 'cup 5 5' 'cup 5' 'cup' 'cup 5 5 5' 'setaf 1' 'setaf 1 2' 'setaf 300' \
            'cup 0x10 010' 'cup 0x10' 'cup 99999999999 1' 'cup 5 x' 'cup x 5' \
            'hpa 10 bold' 'bold cols' 'cup 5 5 bold' 'bold cols lines' 'cols lines' \
            'setaf 1 setab 2' 'cuf 3' 'cuf -- 3' 'csr 1 10' 'Ms a b' 'Ms 1 2' 'Ms a' \
            'sgr 1 0 0 0 0 0 0 0 0' 'sgr 0 0 0 0 1 0 0 0 1' 'initc 1 2 3 4'; do
  # shellcheck disable=SC2086  # the words are the case
  ENVS=(TERM=xterm-256color); run_case tput $args
  # shellcheck disable=SC2086
  ENVS=(TERM=xterm-256color); run_case tput -v $args
done
ENVS=(TERM=xterm-256color); run_case tput cup ' 5' '+6'
ENVS=(TERM=xterm-256color); run_case tput cup '5 ' 6
ENVS=(TERM=xterm-256color); run_case tput -v cup ' 5' '+6'
ENVS=(TERM=xterm-256color); run_case tput cup -1 2
ENVS=(TERM=xterm-256color); run_case tput -- cup -1 2
ENVS=(TERM=xterm-256color); run_case tput -- -v
# Strings for parameters, standard and extended; and the rest refused.
for args in 'Cs red' 'Cs' 'Cs 5' 'Ms a b' 'Ms 1' 'pfkey 1 abc' 'pfkey abc' 'pfkey 1' \
            'pfloc 2 x y' 'pfx 3 z' 'pln 4 w' 'pfxl 1 a b' 'pfxl 1 a' 'pfxl 1' \
            'Xs foo' 'Xn 1 2' 'Xn 1' 'Xm 1 a' 'u1 foo' 'Xs'; do
  # shellcheck disable=SC2086
  ENVS=(TERM=zz-str "$TI"); run_case tput -v $args
done
# The static variables a capability sets are still set for the next.
ENVS=(TERM=zz-static "$TI"); run_case tput u0 5 u0 7 u0 9
ENVS=(TERM=zz-static "$TI"); run_case tput u2 1 2 u2 3 4 u2 5
ENVS=(TERM=zz-static "$TI"); run_case tput u3 5 u3 7

# --- -S: the commands from standard input ------------------------------------------
for text in 'cols\n' 'cols\nlines\n' 'bold cols\n' 'cup 1 2\nsgr0\n' 'cols\nnosuch\nbold\n' \
            'am\nbw\nbw\nbw\n' 'bw\n' '\n\n' '  cols \t lines  \n' 'cols' 'col\0s\nlines\n' \
            'clear\n' 'longname\n' 'reset\n' 'init\n'; do
  FDS=12; STDIN=$text; ENVS=(TERM=xterm-256color); run_case tput -S
done
FDS=12; STDIN='u0 5\nu0 7\n'; ENVS=(TERM=zz-static "$TI"); run_case tput -S
FDS=12; STDIN='cols\n'; ENVS=(TERM=xterm-256color); run_case tput -S -S
FDS=12; STDIN='cols\n'; ENVS=(TERM=xterm-256color); run_case tput -S cols
FDS=12; STDIN='bold\nflash\n'; ENVS=(TERM=vt100 "$TI"); run_case tput -S -T zz-pad
FDS=12; STDIN="$(printf 'cols%.0s ' $(seq 3000))\n"; ENVS=(TERM=xterm-256color); run_case tput -S
FDS=12; STDIN="$(printf 'x%.0s' $(seq 8200))\n"; ENVS=(TERM=xterm-256color); run_case tput -S

# --- the command line -----------------------------------------------------------------
for args in '' '-z' '--version' '-T' '-Tvt100 bold' '-T vt100 bold' '-T dumb cols' \
            '-T nosuch cols' '-x clear' '-v' 'cols -T vt100' '-T xterm -T vt100 cols' \
            '-z -V'; do
  # shellcheck disable=SC2086
  ENVS=(TERM=xterm-256color); run_case tput $args
done
ENVS=(TERM=xterm-256color); run_case tput -T '' cols
ENVS=(TERM=xterm-256color); run_case tput -T '' clear
ENVS=(TERM=xterm-256color); xfail_case "our version string, not ncurses'" tput -V
ENVS=(TERM=xterm-256color); xfail_case "our version string, not ncurses'" tput -V -z
for t in '' nosuch "$(printf 'x%.0s' $(seq 600))" zz-gen zz-realgen zz-hc; do
  ENVS=("TERM=$t" "$TI"); run_case tput cols
  ENVS=("TERM=$t" "$TI"); run_case tput bold
done
run_case tput cols
run_case tput -T xterm cols
# The size: the window's, then LINES and COLUMNS, then the entry's.
for e in LINES=40000 COLUMNS=99 LINES=abc COLUMNS=0x50 LINES=-5 COLUMNS=0 'LINES= 7' COLUMNS=7x; do
  ENVS=(TERM=xterm "$e"); run_case tput lines
  ENVS=(TERM=xterm "$e"); run_case tput cols
  ENVS=(TERM=xterm "$e"); run_case tput -T xterm cols
done
ROWS=0; WIDTH=0; ENVS=(TERM=xterm); run_case tput cols
ROWS=0; WIDTH=0; ENVS=(TERM=xterm); run_case tput lines
ROWS=50; WIDTH=132; ENVS=(TERM=xterm); run_case tput cols
ENVS=(TERM=zz-big "$TI"); run_case tput -T zz-big cols
FDS=; ENVS=(TERM=zz-big "$TI"); run_case tput cols
FDS=; ENVS=(TERM=zz-big "$TI"); run_case tput it

# --- padding --------------------------------------------------------------------------
for speed in - 9600 300 0; do
  for cap in bold clear el flash sgr0; do
    SPEED=$speed; ENVS=(TERM=zz-pad "$TI"); run_case tput "$cap"
    SPEED=$speed; ENVS=(TERM=zz-pc "$TI"); run_case tput "$cap"
    SPEED=$speed; ENVS=(TERM=zz-npc "$TI"); run_case tput "$cap"
  done
done
SPEED=9600; ENVS=(TERM=zz-pad "$TI"); run_case tput clear
SPEED=9600; ENVS=(TERM=zz-pad "$TI"); run_case clear
SPEED=9600; ENVS=(TERM=zz-pad "$TI" LINES=3); run_case clear

# --- a command character ---------------------------------------------------------------
for cc in X XY '' '%'; do
  for cap in bold el sgr0 cmdch; do
    ENVS=(TERM=zz-cmdch "$TI" "CC=$cc"); run_case tput "$cap"
  done
done

# --- clear ------------------------------------------------------------------------------
for t in xterm-256color xterm vt100 dumb zz-e3 zz-noclear zz-gen zz-realgen zz-hc nosuch ''; do
  ENVS=("TERM=$t" "$TI"); run_case clear
  ENVS=("TERM=$t" "$TI"); run_case clear -x
  ENVS=("TERM=$t" "$TI"); run_case tput clear
  ENVS=("TERM=$t" "$TI"); run_case tput -x clear
done
run_case clear
for args in '-q' 'foo' '-x foo' '--version' '-T' '-T vt100' '-Tdumb' '-T vt100 -x' '-- foo' \
            '-q -V'; do
  # shellcheck disable=SC2086
  ENVS=(TERM=xterm); run_case clear $args
done
xfail_case "our version string, not ncurses'" clear -V
FDS=; ENVS=(TERM=xterm); run_case clear
FDS=2; ENVS=(TERM=xterm); run_case clear

# --- tput by its other names --------------------------------------------------------------
for args in '' '-x' '-x foo' 'cols' 'bold cols' '-S' '-v' '-T vt100' '-T vt100 cols' '--'; do
  # shellcheck disable=SC2086
  ALIAS=1; ENVS=(TERM=xterm-256color); run_case clear $args
  # shellcheck disable=SC2086
  ALIAS=1; ENVS=(TERM=xterm-256color); run_case init $args
  # shellcheck disable=SC2086
  ALIAS=1; ENVS=(TERM=xterm-256color); run_case reset $args
done

# --- tput init and reset: the strings, and the terminal afterwards ------------------------
for t in xterm-256color xterm vt100 linux dumb zz-init zz-lr zz-lrp zz-lm zz-lms zz-tabs3 \
         zz-tabs1 zz-tabs40 zz-iprog zz-badif zz-os zz-nel zz-big zz-pad; do
  for cmd in init reset; do
    ENVS=("TERM=$t" "$TI"); run_case tput "$cmd"
  done
done
for preset in odd disabled raw weird tab3 olcuc; do
  for cmd in init reset 'reset cols' 'init init'; do
    # shellcheck disable=SC2086
    PRESET=$preset; ENVS=(TERM=xterm "$TI"); run_case tput $cmd
  done
  PRESET=$preset; ENVS=(TERM=zz-os "$TI"); run_case tput reset
  PRESET=$preset; ENVS=(TERM=zz-nel "$TI"); run_case tput init
done
ROWS=0; WIDTH=0; ENVS=(TERM=xterm); run_case tput init
ROWS=0; WIDTH=0; ENVS=(TERM=zz-big "$TI"); run_case tput init
ROWS=0; WIDTH=0; ENVS=(TERM=xterm LINES=30 COLUMNS=100); run_case tput reset
ENVS=(TERM=zz-iprog "$TI" TERMCAP=foo); run_case tput init
FDS=; ENVS=(TERM=zz-iprog "$TI"); CTTY=1; run_case tput init
FDS=; ENVS=(TERM=xterm); run_case tput init
FDS=; ENVS=(TERM=xterm); CTTY=1; run_case tput reset
FDS=0; ENVS=(TERM=xterm); run_case tput reset
FDS=1; ENVS=(TERM=xterm); run_case tput reset
FDS=2; ENVS=(TERM=xterm); run_case tput reset
FDS=2; ENVS=(TERM=zz-tabs3 "$TI"); run_case tput reset it
ENVS=(TERM=zz-tabs40 "$TI"); run_case tput reset reset
FDS=12; STDIN='init\nit\n'; ENVS=(TERM=zz-tabs40 "$TI"); run_case tput -S

# --- tset and reset ------------------------------------------------------------------------
for prog in tset reset; do
  for args in '-q' '-q xterm' '-' '-Q' '-I' '-r' '-c' '-w' '-s' '-Q -I' '-QI vt100' \
              '-e' '-e^?' '-e ^h' '-i^C' '-k^U' '-e x' '-e ^' '-ek' '-e -Q' \
              '-i' '-k' '-e -i -k' '-z' 'a b' '--version' '-S' '-S -q' '-q -S' '-z -V' \
              '-m xterm:vt100 -q' '-m vt100 -q' '-m :vt100 -q' '-m >110:vt100 -q' \
              '-m >9600:vt100 -q' '-m @38400:vt100 -q' '-m <134:vt100 -q' '-m !110:vt100 -q' \
              '-m !>110:vt100 -q' '-m xterm>B110:vt100 -q' '-m xterm@0:dumb -q' '-m @:vt -q' \
              '-m <>110:x -q' '-m xterm -q' '-m xterm:vt100 -m :dumb -q' '-a vt100 -q' \
              '-d x:y -q' '-p >75:dumb -q' '-m 134.5:x -q' '-m B50:x -q' '-m b50:x -q' \
              '-m >110 -q' '-m xterm:vt100' '-m @:vt -V'; do
    # shellcheck disable=SC2086
    ENVS=(TERM=xterm); run_case "$prog" $args
  done
  ENVS=(TERM=xterm); xfail_case "our version string, not ncurses'" "$prog" -V
  ENVS=(TERM=xterm); xfail_case "our version string, not ncurses'" "$prog" -q -V
  ENVS=(TERM=xterm); xfail_case "our version string, not ncurses'" "$prog" -V -m @:vt
  # Empty, and bytes past 127, which are negative `char`s.
  ENVS=(TERM=xterm); run_case "$prog" -e ''
  ENVS=(TERM=xterm); run_case "$prog" -e "$(printf '\377')"
  ENVS=(TERM=xterm); run_case "$prog" -e "$(printf '\200')" -i "$(printf '\177')"
  for preset in odd disabled raw weird tab3 olcuc; do
    PRESET=$preset; ENVS=(TERM=xterm "$TI"); run_case "$prog"
    PRESET=$preset; ENVS=(TERM=xterm "$TI"); run_case "$prog" -q
    PRESET=$preset; ENVS=(TERM=xterm "$TI"); run_case "$prog" -e^H -i^? -k^X
    PRESET=$preset; ENVS=(TERM=zz-os "$TI"); run_case "$prog"
    PRESET=$preset; ENVS=(TERM=zz-nel "$TI"); run_case "$prog"
    PRESET=$preset; ENVS=(TERM=zz-init "$TI"); run_case "$prog" -c
  done
  for t in xterm-256color vt100 dumb zz-init zz-lr zz-lms zz-tabs3 zz-iprog zz-badif \
           zz-big zz-pad zz-gen zz-realgen zz-hc nosuch unknown ''; do
    STDIN='xterm\n'; FDS=2; ENVS=("TERM=$t" "$TI"); run_case "$prog"
  done
  # Asking: a type to ask for, a default, the end of the input.
  for text in '' '\n' 'xterm\n' 'nosuch\nvt100\n' '\n\nvt100\n' 'vt100' 'nosu\0ch\nvt100\n' \
              "$(printf 'y%.0s' $(seq 300))\nvt100\n"; do
    STDIN=$text; FDS=2; run_case "$prog" -q
    STDIN=$text; FDS=2; ENVS=('TERM=?xterm'); run_case "$prog" -q
    STDIN=$text; FDS=2; ENVS=('TERM=?'); run_case "$prog" -q
    STDIN=$text; FDS=2; ENVS=(TERM=nosuch); run_case "$prog" -Q
    STDIN=$text; FDS=2; ENVS=(TERM=xterm); run_case "$prog" -q -m 'xterm:?vt100'
  done
  # Which terminal: standard error, output, input, then /dev/tty.
  FDS=; ENVS=(TERM=xterm); run_case "$prog"
  FDS=; CTTY=1; ENVS=(TERM=xterm); run_case "$prog"
  FDS=; CTTY=1; ENVS=(TERM=xterm); run_case "$prog" -q
  FDS=0; ENVS=(TERM=xterm); run_case "$prog"
  FDS=1; ENVS=(TERM=xterm); run_case "$prog"
  FDS=2; ENVS=(TERM=xterm); run_case "$prog"
  # The window: given when it has none, read when it has one.
  ROWS=0; WIDTH=0; ENVS=(TERM=xterm); run_case "$prog"
  ROWS=0; WIDTH=0; ENVS=(TERM=xterm); run_case "$prog" -c
  ROWS=0; WIDTH=0; ENVS=(TERM=xterm); run_case "$prog" -w
  ROWS=0; WIDTH=0; ENVS=(TERM=zz-big "$TI"); run_case "$prog" -w
  ROWS=0; WIDTH=0; ENVS=(TERM=xterm LINES=30 COLUMNS=100); run_case "$prog" -w
  # The line's speed decides a mapping.
  for speed in 0 50 75 110 134 9600; do
    SPEED=$speed; ENVS=(TERM=xterm); run_case "$prog" -q -m '>75:vt100' -m '@0:dumb'
    SPEED=$speed; ENVS=(TERM=xterm); run_case "$prog" -q -m 'xterm<110:vt100'
  done
  # TERMCAP goes unless it is a path, which an init program sees.
  for tc in foo /etc/termcap '' x/y; do
    ENVS=(TERM=zz-iprog "$TI" "TERMCAP=$tc"); run_case "$prog"
  done
  # The shells `-s` knows.
  for sh in /bin/sh /bin/csh /usr/bin/tcsh csh /bin/xcsh /bin/cs '' /bin/ksh; do
    ENVS=(TERM=xterm "SHELL=$sh"); run_case "$prog" -s -Q
  done
done

# --- descriptors that cannot be written -----------------------------------------------
# Nothing here closes standard output: what cannot be written is lost, and
# the status is the program's.
for args in 'tput cols' 'tput bold' 'tput clear' 'tput longname' 'tput nosuch' 'tput reset' \
            'clear' 'clear -q' 'tset -q' 'tset -s -Q' 'tset -z' 'reset -r' 'tabs -d -n' 'tabs 1,a'; do
  # shellcheck disable=SC2086
  FDS=02; OUTTO=closed; ENVS=(TERM=xterm); run_case $args
  # shellcheck disable=SC2086
  FDS=02; OUTTO=full; ENVS=(TERM=xterm); run_case $args
  # shellcheck disable=SC2086
  FDS=01; ERRTO=closed; ENVS=(TERM=xterm); run_case $args
  # shellcheck disable=SC2086
  FDS=01; ERRTO=full; ENVS=(TERM=xterm); run_case $args
done
FDS=2; STDIN='cols\nbold\n'; OUTTO=closed; ENVS=(TERM=xterm); run_case tput -S
FDS=2; STDIN='cols\nbold\n'; OUTTO=full; ENVS=(TERM=xterm); run_case tput -S
# An init file is copied with its writes checked: one that cannot all be
# written fails, where strings that cannot be are lost in silence. A
# directory opens, and reads as nothing.
FDS=02; OUTTO=full; ENVS=(TERM=zz-bigif "$TI"); run_case tput init
FDS=02; OUTTO=full; ENVS=(TERM=zz-init "$TI"); run_case tput init
FDS=02; OUTTO=closed; ENVS=(TERM=zz-bigif "$TI"); run_case tput init
FDS=01; ERRTO=closed; ENVS=(TERM=zz-init "$TI"); run_case tset
FDS=01; ERRTO=full; ENVS=(TERM=zz-init "$TI"); run_case tset
FDS=01; ERRTO=full; ENVS=(TERM=zz-init "$TI"); run_case reset
ENVS=(TERM=zz-bigif "$TI"); run_case tput init
ENVS=(TERM=zz-dirif "$TI"); run_case tput init
ENVS=(TERM=zz-dirif "$TI"); run_case tset

# --- -T on a window with no size: the window's nothing, not the entry's --------------
ROWS=0; WIDTH=0; ENVS=(TERM=xterm); run_case tput -T xterm cols
ROWS=0; WIDTH=0; ENVS=(TERM=xterm); run_case tput -T xterm lines
ROWS=0; WIDTH=0; ENVS=(TERM=xterm COLUMNS=50); run_case tput -T xterm cols
ROWS=0; WIDTH=0; ENVS=(TERM=xterm); run_case clear -T xterm
ROWS=0; WIDTH=0; ENVS=(TERM=xterm); run_case tput -T xterm init

# --- the command character, which reaches what the programs read as macros ----------
for cc in X ''; do
  for args in 'clear' 'clear -x' 'tput clear' 'tput init' 'tput reset' 'tput E3' 'tput is2' \
              'tset' 'tset -I' 'tabs' 'tabs 3'; do
    # shellcheck disable=SC2086
    ENVS=(TERM=zz-cmdch "$TI" "CC=$cc"); run_case $args
  done
  PRESET=disabled; ENVS=(TERM=zz-cmdos "$TI" "CC=$cc"); run_case tset
  PRESET=disabled; ENVS=(TERM=zz-cmdos "$TI" "CC=$cc"); run_case tput init
done

# --- tabs -------------------------------------------------------------------------
for args in '' '-8' '-0' '-1' '-4' '-16' '-100' '1,5,9' '1 5 9' '1, 5,9' ' 1,5 ' '1,+10,+10' \
            '+10 +10' '+5' '1 +5' '-a' '-a2' '-ad' '-ax' '-c' '-c2' '-c3' '-c4' '-f' '-p' \
            '-s' '-u' '1 -a' '1 -a 9' '1 -a +5' '-a 1,5' '1,5 -a' '5,1' '5,1,2' '1,a' '-1,' \
            '1,' ',5' '1,,6' '-x' '-' '+' '-d' '-n' '-dn' '-d 1,5,9' '-d -c' '-d 1,90' '-d 0' \
            '-d 1' '+m' '+m5' '+m0' '+m5 -d' '+mx' '+m -0' '-T' '-Tvt100 4' '-T vt100 4' \
            '-T dumb' '-T nosuch' '4 -Tvt100' '-T zz-nohts' '-d 3 -T xterm' '-1,5' '-1 5' \
            '-d -1,5' '-d 1,+5,+5' '-d +5 +5' '-d 1 5 -a 9'; do
  # shellcheck disable=SC2086
  ENVS=(TERM=xterm "$TI"); run_case tabs $args
done
ENVS=(TERM=xterm); run_case tabs -d -n "1 5"
ENVS=(TERM=xterm); run_case tabs -d -n -- 5
ENVS=(TERM=xterm); xfail_case "our version string, not ncurses'" tabs -V
ENVS=(TERM=xterm); xfail_case "our version string, not ncurses'" tabs -dV
for t in xterm-256color vt100 linux dumb zz-tbc zz-csi zz-nohts zz-tabs3 zz-big nosuch ''; do
  ENVS=("TERM=$t" "$TI"); run_case tabs
  ENVS=("TERM=$t" "$TI"); run_case tabs -d 1,10,20
done
run_case tabs
run_case tabs -d -n
for t in zz-mgn zz-mgncuf zz-mgnsp zz-mgnp zz-mgnp2 zz-mgnpr zz-mgnlr xterm; do
  for m in +m +m0 +m1 +m5 +m79 +m200; do
    ENVS=("TERM=$t" "$TI"); run_case tabs "$m" -d 1,10,20
    ENVS=("TERM=$t" "$TI"); run_case tabs "$m" -n -d 4
  done
done
# Off the terminal no `-ocrnl`, and lines end in a bare newline; on it,
# `ocrnl` is cleared while the stops are set and put back after.
FDS=2; ENVS=(TERM=xterm); run_case tabs -d 1,10
FDS=0; ENVS=(TERM=xterm); run_case tabs -d 1,10
FDS=; ENVS=(TERM=xterm); run_case tabs -d 1,10
PRESET=ocrnl; ENVS=(TERM=xterm); run_case tabs -d 1,10
PRESET=ocrnl; ENVS=(TERM=xterm); run_case tabs -n -d 1,10
PRESET=weird; ENVS=(TERM=xterm); run_case tabs -d 1,10
# The width: the window's, COLUMNS, the entry's, else 80.
WIDTH=40; ENVS=(TERM=xterm); run_case tabs -d 4
WIDTH=133; ENVS=(TERM=xterm); run_case tabs -d -c3
ROWS=0; WIDTH=0; ENVS=(TERM=xterm); run_case tabs -d 4
ROWS=0; WIDTH=0; ENVS=(TERM=zz-tbc "$TI"); run_case tabs -d 4
ROWS=0; WIDTH=0; ENVS=(TERM=zz-big "$TI"); run_case tabs -d -n 4
ENVS=(TERM=xterm COLUMNS=30); run_case tabs -d 4
ENVS=(TERM=xterm COLUMNS=30); run_case tabs -d +m5 4

printf '\n%d passed, %d differed, %d broken, %d differ on purpose' \
  "$pass" "$fail" "$broken" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ] && [ "$xpass" -eq 0 ]
