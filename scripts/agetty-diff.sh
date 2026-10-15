#!/usr/bin/env bash
# Differential test: our `agetty` against util-linux 2.39.3's.
#
# As far as the port has come (stage 1, `userspace/agetty/src/lib.rs`): the
# command line and its refusals, `--help`, `--version`, `--list-speeds`, `--reload`, and
# `--show-issue` -- every escape of the issue file, the files and
# directories `-f` names, `/etc/issue` itself, and the line's speed.
#
# `--show-issue` reads the speed off standard input with `tcgetattr`, so it
# runs with a pseudo-terminal there (`onpty.py` below, which can set the
# speed first) -- and once with `/dev/null`, which it refuses. Nothing is
# compared that would open a line: that is stage 2.
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
