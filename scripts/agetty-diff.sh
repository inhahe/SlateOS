#!/usr/bin/env bash
# Differential test: our `agetty` against util-linux 2.39.3's.
#
# As far as the port has come (stages 1 and 2, `userspace/agetty/src/lib.rs`):
# the command line and its refusals, `--help`, `--version`, `--list-speeds`,
# `--reload`, and `--show-issue` -- every escape of the issue file, the files
# and directories `-f` names, `/etc/issue` itself, and the line's speed; and
# a run on a line: the issue and the prompt, the name typed with its erase,
# kill and parity, and how `login` is run -- its arguments, its terminal
# type, directory and niceness, and the line's modes.
#
# `--show-issue` reads the speed off standard input with `tcgetattr`, so it
# runs with a pseudo-terminal there (`onpty.py` below, which can set the
# speed first) -- and once with `/dev/null`, which it refuses. A run on a
# line is driven by `online.py`, typing at a pseudo-terminal, with
# `fakelogin` as the login program.
#
# The one deliberate difference, `--version` without `plymouth` (SlateOS
# has no boot splash), is taken out of the reference's output before the
# comparison. `\d` and `\t` read the clock, so a case using them is run
# again when the two sides straddle a second.
#
# What is compared: stdout, stderr and the exit status of each case, and
# the same under closed and full descriptors.
set -u

DIFF_PROG='agetty'
DIFF_PKG='agetty'
DIFF_FEATURES='port'
DIFF_NEED='timeout python3'
DIFF_REF=/usr/sbin/agetty
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; broken=0

cd "$DIFF_TMP" || exit 1

# The command after `--`, with a fresh pseudo-terminal as its standard
# input -- at the speed named first (`9600`), or at the kernel's default for
# `-` -- and its own standard output and error.
cat > onpty.py <<'EOF'
import os
import subprocess
import sys
import termios

speed = sys.argv[1]
master, slave = os.openpty()
if speed != '-':
    attrs = termios.tcgetattr(slave)
    code = getattr(termios, 'B' + speed)
    attrs[4] = code
    attrs[5] = code
    termios.tcsetattr(slave, termios.TCSANOW, attrs)
rc = subprocess.run(sys.argv[2:], stdin=slave).returncode
os.close(slave)
os.close(master)
sys.exit(rc if rc >= 0 else 128 - rc)
EOF

# The command after the first two arguments, run as a getty is: a session
# leader whose controlling terminal is a fresh pseudo-terminal, on all three
# standard streams, at the speed named first (`-` the kernel's). The second
# argument is what to do at the keyboard, a JSON list of steps --
# ["wait", TEXT] until the line shows TEXT, ["waitn", TEXT, N] until it has
# shown it N times, ["send", BYTES] (code points below 256), ["sleep",
# SECONDS], ["run", ARGV] to run a command beside it, ["write", PATH, TEXT]
# to rewrite a file. What the line showed goes to standard output; the
# status is the command's (128 + N for signal N).
cat > online.py <<'EOF'
import fcntl
import json
import os
import select
import subprocess
import sys
import termios
import time

speed, steps = sys.argv[1], json.loads(sys.argv[2])
master, slave = os.openpty()
if speed != '-':
    attrs = termios.tcgetattr(slave)
    attrs[4] = attrs[5] = getattr(termios, 'B' + speed)
    termios.tcsetattr(slave, termios.TCSANOW, attrs)


def session():
    os.setsid()
    fcntl.ioctl(0, termios.TIOCSCTTY, 0)


child = subprocess.Popen(sys.argv[3:], stdin=slave, stdout=slave, stderr=slave,
                         preexec_fn=session)
os.close(slave)
shown = bytearray()
deadline = time.time() + 40


def pump(timeout):
    """Read what the line shows: True for something, False at its end."""
    ready, _, _ = select.select([master], [], [], timeout)
    if not ready:
        return None
    try:
        data = os.read(master, 4096)
    except OSError:
        return False
    if not data:
        return False
    shown.extend(data)
    return True


for step in steps:
    kind, arg = step[0], step[1]
    if kind in ('wait', 'waitn'):
        want = arg.encode('latin-1')
        times = int(step[2]) if kind == 'waitn' else 1
        while shown.count(want) < times and time.time() < deadline:
            if pump(0.1) is False:
                break
    elif kind == 'send':
        os.write(master, arg.encode('latin-1'))
    elif kind == 'sleep':
        time.sleep(float(arg))
    elif kind == 'run':
        subprocess.run(arg, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL)
    elif kind == 'write':
        with open(arg, 'w', encoding='latin-1') as f:
            f.write(step[2])
while time.time() < deadline:
    if child.poll() is not None:
        while pump(0.05):
            pass
        break
    if pump(0.1) is False:
        try:
            child.wait(timeout=max(0.0, deadline - time.time()))
        except subprocess.TimeoutExpired:
            pass
        break
if child.poll() is None:
    child.kill()
    child.wait()
    sys.stdout.buffer.write(bytes(shown))
    sys.exit(124)
sys.stdout.buffer.write(bytes(shown))
rc = child.returncode
sys.exit(rc if rc >= 0 else 128 - rc)
EOF

# The login program the getty runs: it writes down how it was run -- its
# arguments, the terminal type, the directory, the niceness, whether its
# standard input, output and error are the line, the signals it was started
# ignoring, and the line's modes -- in the file `AGETTY_OUT` names.
cat > fakelogin <<'EOF'
#!/bin/sh
# Asked before anything is redirected: what the getty handed over.
fds=
for fd in 0 1 2; do
    if [ -t "$fd" ]; then fds="$fds $fd:tty"; else fds="$fds $fd:none"; fi
done
{
    for a in "$@"; do printf 'ARG %s\n' "$a"; done
    echo "TERM=$TERM"
    echo "PWD=$(pwd)"
    echo "NICE=$(nice)"
    echo "FDS:$fds"
    grep '^SigIgn' "/proc/$$/status"
    stty -g
} > "$AGETTY_OUT" 2>&1
EOF
chmod +x fakelogin

# --- the issue files --------------------------------------------------------
w() { printf '%s\n' "$2" > "$1"; }
w uname '\s|\n|\r|\v|\m'
w domains '\o|\O'
w users '\u|\U'
w net '\4|\6|\4{lo}|\6{lo}|\4{eth0}|\6{eth0}|\4{nope}|\6{nope}|\4{}'
# Not `\S{}`: upstream's scan for an empty name never ends (lib.rs,
# deliberate difference 3).
w osrel '\S|\S{ID}|\S{ANSI_COLOR}|\S{NOPE}|\S{VERSION_ID}|\S{NAME}'
w colours '\e{red}r\e{reset}|\e{bold}b\e{reset}|\e{nope}|\e[0m|\e{}|\e{31}'
w misc '\l|\b|\\|\q|\x|\{|\}'
w clock '\d|\t'
w plain 'just text, no escapes'
printf '%s' 'a\e{red' > unterminated
printf 'end\134' > trailing
printf 'a\000b\377c\\\000d\n' > bytes
printf '\\S{%s}|\\e{%s}|\\4{%s}\n' "$(printf 'A%.0s' $(seq 100))" \
  "$(printf 'r%.0s' $(seq 40))" "$(printf 'l%.0s' $(seq 200))" > long
: > empty
mkdir -p dir/sub.issue
w dir/10-b.issue 'ten \l'
w dir/9-a.issue 'nine'
w dir/.hidden.issue 'hidden'
w dir/readme 'readme'
w dir/.issue 'dot'
ln -s 9-a.issue dir/link.issue
ln -s nowhere dir/dangling.issue
mkdir -p emptydir

# $1 = side, $2 = how stdin is (`-`: /dev/null; else onpty.py's speed),
# rest = argv.
run_side() {
  local side=$1 tty=$2; shift 2
  if [ "$tty" = null ]; then
    diff_run env LC_ALL=C.UTF-8 PATH="$bindir/$side:$PATH" timeout -k 2 20 agetty "$@" </dev/null
  else
    diff_run env LC_ALL=C.UTF-8 PATH="$bindir/$side:$PATH" timeout -k 2 20 \
      python3 "$DIFF_TMP/onpty.py" "$tty" agetty "$@" </dev/null
  fi
}

judge() {
  local o_rc=$1 g_rc=$2
  if [ "$o_rc" = 127 ] && [ ! -s "$DIFF_TMP/o.err" ] || [ "$o_rc" = 124 ] || [ "$g_rc" = 124 ]; then
    AGREED=broken
  elif cmp -s "$DIFF_TMP/o.out" "$DIFF_TMP/g.out" \
     && cmp -s "$DIFF_TMP/o.err" "$DIFF_TMP/g.err" && [ "$o_rc" = "$g_rc" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  # NULs shown as `@`: the `bytes` fixture has them, and a shell variable
  # cannot.
  REPORT=$(printf '  ours (rc=%s): out %q err %q\n  gnu  (rc=%s): out %q err %q' \
    "$o_rc" "$(tr '\000' @ < "$DIFF_TMP/o.out")" "$(tr '\000' @ < "$DIFF_TMP/o.err")" \
    "$g_rc" "$(tr '\000' @ < "$DIFF_TMP/g.out")" "$(tr '\000' @ < "$DIFF_TMP/g.err")")
}

report() {
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1)); printf 'BROKEN %s\n%s\n' "$1" "$REPORT"
  elif [ "$AGREED" = yes ]; then
    pass=$((pass + 1)); [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$1"
  else
    fail=$((fail + 1)); printf 'DIFF %s\n%s\n' "$1" "$REPORT"
  fi
  return 0
}

# One case, stdin as $1 says; tried up to three times when the clock is in
# it, since the two sides may read different seconds.
both() {
  local tty=$1 o_rc g_rc tries=1; shift
  case "$*" in *clock*) tries=3 ;; esac
  while [ "$tries" -gt 0 ]; do
    run_side ours "$tty" "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
    run_side gnu "$tty" "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
    # The deliberate difference: no `plymouth` among the features.
    sed -i 's/plymouth, //' "$DIFF_TMP/g.out"
    judge "$o_rc" "$g_rc"
    [ "$AGREED" = no ] || break
    tries=$((tries - 1))
  done
  report "agetty $(printf '%q ' "$@" | sed "s|$DIFF_TMP/||g")<$tty"
}

redir_case() {
  local how=$1 o_rc g_rc; shift
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/ours:$PATH" timeout -k 2 20 \
    python3 "$DIFF_TMP/onpty.py" - bash -c "exec agetty \"\$@\" $how" sh "$@" \
    </dev/null >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/gnu:$PATH" timeout -k 2 20 \
    python3 "$DIFF_TMP/onpty.py" - bash -c "exec agetty \"\$@\" $how" sh "$@" \
    </dev/null >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  sed -i 's/plymouth, //' "$DIFF_TMP/g.out"
  judge "$o_rc" "$g_rc"
  report "agetty $(printf '%q ' "$@" | sed "s|$DIFF_TMP/||g")$how"
}

# --- the command line and its refusals ---------------------------------------
# Every case stops before a line is opened: a refusal, a printed answer, or
# a failure parsing what would have been run.
for args in '' '--help' '--version' '--list-speeds' '--he' '--ver' '--list' \
            '--bogus' '-z' '-V' '-h' '-8' '--show' '--show-issue=x' \
            '-t' '-t abc tty1' '-t -1 tty1' '-t 99999999999 tty1' '-t 0x10' \
            '-d x tty1' '-P x tty1' '-P -21 --list-speeds' '--nice=abc tty1' \
            '--help --bogus' '--bogus --help' '--version --help' '--list-speeds --help' \
            '9600' '9600,19200' '-a' '--autologin' \
            '--erase-chars' '--kill-chars' '--reload' '--reload --bogus' \
            '-ix --help' '--long-hostname --list-speeds' '-L --help' '-L=never --help' \
            '--local-line==auto --help'; do
  eval "set -- $args"
  both null "$@"
done

# A speed upstream's table lacks and a bad `--local-line` go to syslog, and
# end after the ten seconds' wait: only the silence and the status show.
# `--local-line=always` is one: the long option's value must be `=always`.
both null 123 tty1
both null --local-line=always --help

# --- --show-issue ----------------------------------------------------------------
both null --show-issue
both null --show-issue -f plain
for f in uname domains users net osrel colours misc clock plain unterminated trailing \
         bytes long empty dir emptydir dir/link.issue dir/dangling.issue nonexistent \
         /dev/null 'plain:uname' '::plain::misc:' 'nope:plain' 'dir:plain:dir' \
         'emptydir:plain'; do
  both - --show-issue -f "$f"
  both - --show-issue --issue-file="$f"
done
# The real `/etc/issue`, and none at all.
both - --show-issue
both - -i --show-issue
both - --noissue --show-issue -f plain
# The line's speed, as `\b` shows it.
for speed in 0 50 300 9600 19200 38400 57600 115200 230400 4000000; do
  both "$speed" --show-issue -f misc
done
# Options before it are parsed, and the operands after it are not.
both - --show-issue -f misc tty5 9600
both - -t 5 --show-issue -f misc
both - --show-issue -t x -f misc
both - --show-issue -f misc -- -f uname

# --- on a line ------------------------------------------------------------------
# The line is `-`: standard input, a pseudo-terminal. A pseudo-terminal is no
# virtual console, so this is agetty on a serial line; and without root the
# line cannot be opened by name (it is handed to root). The pseudo-terminal's
# number differs from run to run: it is `pts/N` in what is compared.
w lineissue 'Welcome to \l at \b'

# $1 = side, $2 = speed, $3 = the steps at the keyboard, rest = agetty's
# arguments after `-l fakelogin`.
line_side() {
  local side=$1 speed=$2 steps=$3; shift 3
  rm -f "$DIFF_TMP/login.$side"
  # A case's fixture, made afresh for each side: `LINE_SETUP`, a command.
  if [ -n "${LINE_SETUP:-}" ]; then eval "$LINE_SETUP"; fi
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/$side:$PATH" AGETTY_OUT="$DIFF_TMP/login.$side" \
    timeout -k 2 60 python3 "$DIFF_TMP/online.py" "$speed" "$steps" \
    agetty -l "$DIFF_TMP/fakelogin" "$@" </dev/null
}

# $1 = speed, $2 = steps, rest = agetty's arguments. Compared: what the line
# showed, the status, and how login was run.
line_case() {
  local speed=$1 steps=$2 o_rc g_rc; shift 2
  line_side ours "$speed" "$steps" "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  line_side gnu "$speed" "$steps" "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  { echo '--- login:'; cat "$DIFF_TMP/login.ours" 2>/dev/null; } >> "$DIFF_TMP/o.out"
  { echo '--- login:'; cat "$DIFF_TMP/login.gnu" 2>/dev/null; } >> "$DIFF_TMP/g.out"
  sed -i -E 's#pts/[0-9]+#pts/N#g' "$DIFF_TMP/o.out" "$DIFF_TMP/g.out"
  judge "$o_rc" "$g_rc"
  report "agetty $(printf '%q ' "$@" | sed "s|$DIFF_TMP/||g")on $speed: $steps"
}

name='[["wait","login: "],["send","root\r"]]'
line_case - "$name" -f lineissue -
# The fixture's own proof of life: two sides that never reach login agree
# about everything, so the reference must be seen to get there.
if ! grep -qx 'ARG root' "$DIFF_TMP/login.gnu" 2>/dev/null; then
  broken=$((broken + 1))
  echo "BROKEN the reference never ran login: the line cases measure nothing"
fi
line_case - "$name" -
line_case 9600 "$name" -f lineissue - vt100
line_case 38400 "$name" -f lineissue - 9600,19200 xterm
line_case - '[["wait","login: "],["send","root\n"]]' -f lineissue -
# Erase, kill, the empty line prompted for again, ^C, ^D.
line_case - '[["wait","login: "],["send","ab\u007fc\u0015root\r"]]' -f lineissue -
line_case - '[["wait","login: "],["send","r\b\boot\r"]]' -f lineissue -
line_case - '[["wait","login: "],["send","\r"],["sleep","0.5"],["send","root\r"]]' -f lineissue -
line_case - '[["wait","login: "],["send","ro\u0003ot\r"]]' -f lineissue -
line_case - '[["wait","login: "],["send","ro\u0004"]]' -f lineissue -
line_case - '[["wait","login: "],["send","ab#c@root\r"]]' --erase-chars '#' --kill-chars '@' -f lineissue -
# The eighth bit: parity, or a character.
line_case - '[["wait","login: "],["send","rémy\r"]]' -f lineissue -
line_case - '[["wait","login: "],["send","rémy\r"]]' -8 -f lineissue -
line_case - '[["wait","login: "],["send","ROOT\r"]]' -U -f lineissue -
line_case - '[["wait","login: "],["send","Root\r"]]' -U -f lineissue -
# What is shown before the name.
for opt in -i -N --nohostname --long-hostname --nohints; do
  line_case - "$name" "$opt" -f lineissue -
done
line_case - "$name" -I 'X\033[0mY\\Z' -f lineissue -
line_case - '[["wait","ENTER"],["send","\r"],["wait","login: "],["send","root\r"]]' -p -f lineissue -
line_case - '[["sleep","0.5"],["send","x\r"],["wait","login: "],["send","root\r"]]' -w -f lineissue -
# How login is run.
line_case - "$name" -o '-p -- \u' -f lineissue -
line_case - "$name" -o '  -p  ' -f lineissue -
line_case - "$name" -E -H far.example -f lineissue -
line_case - "$name" -E --nohostname -f lineissue -
line_case - "$name" -E -f lineissue -
line_case - '[]' -a root -f lineissue -
line_case - '[]' -n -f lineissue -
line_case - '[]' -n -a root -o '-f \u' -f lineissue -
line_case - "$name" --chdir /tmp -f lineissue -
line_case - "$name" --nice 5 -f lineissue -
line_case - "$name" --delay 1 -f lineissue -
# The line's modes for login.
for opt in -L -L=never -h -c -s; do
  line_case 19200 "$name" "$opt" -f lineissue -
done
# A BREAK tries the next speed, which the issue's \b then shows.
line_case 9600 '[["wait","login: "],["send","\u0000"],["sleep","2"],["send","root\r"]]' \
  -f lineissue - 9600,19200
# A timer, cleared by the name, or set off.
line_case - "$name" -t 30 -f lineissue -
line_case - '[]' -t 1 -f lineissue -
# Refusals, after the ten seconds' wait.
line_case - '[["wait","login: "],["send","-froot\r"]]' -f lineissue -
line_case - "$name" -l /nonexistent/login -f lineissue -

# --- a reload -------------------------------------------------------------------
# `agetty --reload` touches /run/agetty.reload, which a getty waiting for a
# name watches; the issue is shown again if it changed, and only while
# nothing is typed. Only where this user may write that file.
if [ -w /run/agetty.reload ]; then
  re="$DIFF_TMP/reissue"
  LINE_SETUP='w reissue before'
  reload='["run",["agetty","--reload"]]'
  line_case - '[["wait","login: "],["write","'"$re"'","after\n"],'"$reload"',["waitn","login: ",2],["send","root\r"]]' -f reissue -
  line_case - '[["wait","login: "],'"$reload"',["sleep","2"],["send","root\r"]]' -f reissue -
  line_case - '[["wait","login: "],["send","ro"],["write","'"$re"'","after\n"],'"$reload"',["sleep","2"],["send","ot\r"]]' -f reissue -
  line_case - '[["wait","login: "],["send","r\u007f"],["write","'"$re"'","after\n"],'"$reload"',["waitn","login: ",2],["send","root\r"]]' -f reissue -
  line_case - '[["wait","ENTER"],["write","'"$re"'","after\n"],'"$reload"',["waitn","ENTER",2],["send","\r"],["wait","login: "],["send","root\r"]]' -p -f reissue -
  line_case - '[["wait","login: "],'"$reload"',["waitn","login: ",2],["send","root\r"]]' -i -
  LINE_SETUP=
else
  echo "SKIP the reload cases: /run/agetty.reload is not writable here"
  echo "     (as root, once: install -m 666 /dev/null /run/agetty.reload)"
fi

# --- closed and full descriptors ----------------------------------------------
for how in '>&-' '>/dev/full' '2>&-' '2>/dev/full'; do
  redir_case "$how" --help
  redir_case "$how" --version
  redir_case "$how" --list-speeds
  redir_case "$how" --bogus
  redir_case "$how" --show-issue -f uname
  redir_case "$how" --show-issue -f osrel
  redir_case "$how" tty1 -t x
done

printf '%d passed, %d differed, %d broken\n' "$pass" "$fail" "$broken"
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ]
