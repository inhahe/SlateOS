#!/bin/bash
# Differential test: our watch against procps-ng 4.0.4's.
#
# ## The reference
#
# Ubuntu's /usr/bin/watch, procps 4.0.4-4ubuntu3.2, built with
# `--enable-watch8bit` and `--enable-colorwatch` (colour on unless `-C`);
# none of Debian's or Ubuntu's patches touch `src/watch.c`. It draws through
# Ubuntu's libncursesw 6.4, ours through the curses crate.
#
# ## How a run is captured
#
# The full-screen runs are on a pseudo-terminal of their own
# (`scripts/curses-ptyrun.py`), which sends a signal, types a key or resizes
# the terminal once the program has drawn and gone quiet. Most end by
# themselves: `-g` when the output changes, `-q` when it stops changing, `-e`
# at a key. The command is mostly a counter -- a file in a directory of the
# side's own, read and bumped by each run -- so that every run of either side
# sees the same sequence of outputs.
#
# The clock both sides read is pinned by a preloaded shim compiled here:
# `time`, `gettimeofday` and `CLOCK_REALTIME` start at a fixed date and run on
# with the monotonic clock, so the title's date is the same on both sides.
# Every case compares what the terminal was sent, byte for byte, with
# standard error and the exit status; the option cases, what went to
# standard output and error.
#
# ## Cases that differ on purpose
#
# `--version` names SlateOS.
#
# Run `OURS=/usr/bin/watch ./scripts/watch-diff.sh` to check that the harness
# still discriminates: the cases that differ on purpose should then be
# reported as no longer differing, and nothing else.
set -u

DIFF_PROG='watch'
DIFF_NEED='timeout python3 cc'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

runner=$root/scripts/curses-ptyrun.py
pass=0; fail=0; xfail=0; xpass=0

shim=$DIFF_TMP/fixtime.so
cat > "$DIFF_TMP/fixtime.c" <<'EOF'
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdlib.h>
#include <sys/time.h>
#include <time.h>

static int (*real_clock_gettime) (clockid_t, struct timespec *);
static struct timespec start;
static long long base = 1790000000;
static int ready;

static void init (void)
{
  const char *s;
  if (ready)
    return;
  ready = 1;
  real_clock_gettime = dlsym (RTLD_NEXT, "clock_gettime");
  s = getenv ("FIXTIME");
  if (s)
    base = atoll (s);
  real_clock_gettime (CLOCK_MONOTONIC, &start);
}

static void now (struct timespec *ts)
{
  struct timespec m;
  long long sec;
  long nsec;
  init ();
  real_clock_gettime (CLOCK_MONOTONIC, &m);
  sec = m.tv_sec - start.tv_sec;
  nsec = m.tv_nsec - start.tv_nsec;
  if (nsec < 0)
    {
      nsec += 1000000000L;
      sec--;
    }
  ts->tv_sec = base + sec;
  ts->tv_nsec = nsec;
}

time_t time (time_t *t)
{
  struct timespec ts;
  now (&ts);
  if (t)
    *t = ts.tv_sec;
  return ts.tv_sec;
}

int gettimeofday (struct timeval *tv, void *tz)
{
  struct timespec ts;
  (void) tz;
  now (&ts);
  if (tv)
    {
      tv->tv_sec = ts.tv_sec;
      tv->tv_usec = ts.tv_nsec / 1000;
    }
  return 0;
}

int clock_gettime (clockid_t c, struct timespec *ts)
{
  init ();
  if (c == CLOCK_REALTIME || c == CLOCK_REALTIME_COARSE)
    {
      now (ts);
      return 0;
    }
  return real_clock_gettime (c, ts);
}
EOF
if ! cc -O2 -shared -fPIC -o "$shim" "$DIFF_TMP/fixtime.c" -ldl; then
  echo "could not build the clock shim" >&2
  exit 1
fi

TERMV=xterm-256color
LOC=C
TZV=UTC
ENVX=()
# The counter: `run N`, N one more each time, from a file in the side's own
# directory.
COUNTER='n=$(cat c); echo $((n+1)) > c; echo "run $n"'

# Where watch's standard error goes: `tty`, the terminal, as when it is run
# from one -- it measures the terminal there -- or `file`.
STDERR='tty'

# side SIDE SIZE STEP... -- ARGS...: watch on a terminal of SIZE, in a fresh
# directory whose counter starts at 0.
side() {
  local s=$1 size=$2; shift 2
  local -a steps=()
  while [ "$1" != -- ]; do steps+=(--then "$1"); shift; done
  shift
  [ "$STDERR" = tty ] && steps+=(--tty-stderr)
  local dir=$DIFF_TMP/run-$s
  rm -rf "$dir"; mkdir -p "$dir"; echo 0 > "$dir/c"
  ( cd "$dir" &&
    diff_run timeout -k 2 30 env -i PATH="$bindir/$s:/usr/bin:/bin" TERM="$TERMV" \
      LC_ALL="$LOC" TZ="$TZV" LD_PRELOAD="$shim" "${ENVX[@]}" \
      python3 "$runner" "${steps[@]}" "${size%x*}" "${size#*x}" \
      "$DIFF_TMP/$s.out" "$DIFF_TMP/$s.err" watch "$@" < /dev/null )
  printf '%s\n' "$?" > "$DIFF_TMP/$s.rc"
}

# plain SIDE ARGS...: watch with no terminal, for what it says before it
# would draw.
plain() {
  local s=$1; shift
  diff_run timeout -k 2 30 env -i PATH="$bindir/$s:/usr/bin:/bin" TERM="$TERMV" \
    LC_ALL="$LOC" "${ENVX[@]}" watch "$@" < /dev/null \
    > "$DIFF_TMP/$s.out" 2> "$DIFF_TMP/$s.err"
  printf '%s\n' "$?" > "$DIFF_TMP/$s.rc"
}

same() {
  local f
  for f in out err rc; do
    cmp -s "$DIFF_TMP/ours.$f" "$DIFF_TMP/gnu.$f" || return 1
  done
}

show() {
  local s
  for s in ours gnu; do
    printf -- '--- %s: status %s\n' "$s" "$(cat "$DIFF_TMP/$s.rc")"
    printf 'out (%s bytes):\n' "$(wc -c < "$DIFF_TMP/$s.out")"
    od -An -c "$DIFF_TMP/$s.out" | head -16
    printf 'err:\n'; head -5 "$DIFF_TMP/$s.err"
  done
  if ! cmp -s "$DIFF_TMP/ours.out" "$DIFF_TMP/gnu.out"; then
    cmp "$DIFF_TMP/ours.out" "$DIFF_TMP/gnu.out" | head -1
  fi
}

judge() {
  local label=$1 want=$2 why=${3:-}
  if same; then
    if [ "$want" = differ ]; then
      xpass=$((xpass+1))
      printf 'XPASS %s -- expected to differ (%s) and did not\n' "$label" "$why"
    else
      pass=$((pass+1))
      [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
    fi
  elif [ "$want" = differ ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL %s (%s)\n' "$label" "$why"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n' "$label"
    show
  fi
  return 0
}

# screen LABEL SIZE STEP... -- ARGS...
screen() {
  local label=$1; shift
  side ours "$@"
  side gnu "$@"
  judge "$label [$TERMV, $LOC, $1]" same
}

# opts LABEL ARGS... / xopts WHY LABEL ARGS...
opts() {
  local label=$1; shift
  plain ours "$@"
  plain gnu "$@"
  judge "$label" same
}
xopts() {
  local why=$1 label=$2; shift 2
  plain ours "$@"
  plain gnu "$@"
  judge "$label" differ "$why"
}

# --- options, and refusals before the screen -------------------------------------
opts "--help" --help
opts "-h" -h
opts "-h after an interval" -n 1 -h
opts "no command" -t
opts "no command at all"
for args in '-Z' '--bogus' '-n' '-q' '--interval' '-n abc' '-n 1x' '-n -' \
            '-n .' '-q abc' '--equexit=' '--interval=' '-n 1,5 -h' '-x' '-d -h'; do
  # shellcheck disable=SC2086  # each is a list of arguments
  opts "$args" $args
done
opts "-n ''" -n ''
ENVX=(WATCH_INTERVAL=abc)
opts "WATCH_INTERVAL=abc" -t true
ENVX=(WATCH_INTERVAL=)
opts "WATCH_INTERVAL empty" -t true
ENVX=()
xopts "--version names SlateOS" "-v" -v
xopts "--version names SlateOS" "--version" --version

# --- runs that end by themselves --------------------------------------------------------
for TERMV in xterm-256color xterm vt100 linux screen dumb; do
  screen "two runs, -g" 24x80 -- -t -n 0.1 -g "$COUNTER"
  screen "three runs, -q 2" 24x80 -- -t -n 0.1 -q 2 'echo same'
done
TERMV=xterm-256color
for LOC in C C.UTF-8; do
  screen "the title" 24x80 -- -n 0.1 -g "$COUNTER"
  screen "the title, -n 2.25" 24x80 -- -n 2.25 -g -p "$COUNTER"
  screen "the title, cut" 24x40 -- -n 0.1 -g "$COUNTER; : a long command to cut short"
  screen "the title, no room" 24x30 -- -n 0.1 -g "$COUNTER"
  screen "the title, wide command" 24x50 -- -n 0.1 -g "$COUNTER; : 日本語のコマンドの名前"
  # A command with no width (`wcswidth` -1) is never cut, at any width.
  screen "the title, a tab in the command" 24x80 -- -n 0.1 -g "$COUNTER;	: tab"
  screen "the title, a control character" 24x60 -- -n 0.1 -g "$COUNTER; : $(printf '\001') to cut"
done
LOC=C
for size in 30x100 10x40 5x20 3x80 24x10; do
  screen "two runs, -g" "$size" -- -t -n 0.1 -g "$COUNTER"
done
TZV='America/New_York'
screen "the title in New York" 24x80 -- -n 0.1 -g "$COUNTER"
TZV=UTC
# A comma for the decimal point, in the title's `%.1f`. WSL has only C and
# C.UTF-8, so the locale is generated into the run's own directory; `localedef`
# exits 1 for a warning it survived, so what it made is what is looked at.
locdir=$DIFF_TMP/locales
mkdir -p "$locdir"
localedef -i de_DE -f UTF-8 "$locdir/de_DE.UTF-8" > /dev/null 2>&1
if [ -d "$locdir/de_DE.UTF-8" ]; then
  LOC=de_DE.UTF-8; ENVX=(LOCPATH="$locdir")
  screen "the title, a comma for the point" 24x80 -- -n 0.1 -g "$COUNTER"
  screen "the title, -n 2,25" 24x80 -- -n 2,25 -g -p "$COUNTER"
  opts "-n 1,5 -h, a comma for the point" -n 1,5 -h
  LOC=C; ENVX=()
else
  echo "watch-diff: could not generate de_DE.UTF-8; its cases are skipped" >&2
fi

# What the command writes, after the counter's line. A byte that starts no
# character takes what follows it into the character `my_getwc` is still
# looking for -- to the end of the output, when sixteen bytes do not come
# first -- so a counter after it would be read into it, the screens would
# never differ, and `-g` would wait for the timeout on both sides.
out() { # out LABEL PRINTF-FORMAT [OPTION...]
  local label=$1 fmt=$2; shift 2
  screen "$label" 24x80 -- -t -n 0.1 -g "$@" "$COUNTER; printf '$fmt'"
}
for LOC in C C.UTF-8; do
  out "plain lines" 'one\ntwo\nthree\n'
  out "no final newline" 'one\ntwo'
  out "blank lines" '\n\none\n\n\ntwo\n'
  out "tabs" 'a\tb\tc\t\td\n\tx\n'
  out "a long line, wrapped" '%0200d\n'
  out "a long line, cut" '%0200d\n' -w
  out "control bytes" 'a\001b\002c\177d\033e\n'
  out "a bell" 'ding\007dong\n'
  out "wide characters" '日本語 テキスト\n'
  out "combining" 'e\314\201 x\n'
  out "a wide character at the edge" '%079d日\n'
  # Octal escapes: the shell is dash, whose printf has no `\x`.
  out "bad UTF-8" 'a\200b\303c\377d\n'
  out "sixteen bytes and no character" 'a\200bcdefghijklmnopqrstuvwxyz\n'
  out "0xFF" 'a\377b\n'
  out "a NUL" 'a\000b\n'
  out "a NUL, then sixteen bytes" 'a\000bcdefghijklmnopqrstuvwxyz\n'
  out "CR in the middle" 'abc\rx\n'
  screen "more lines than fit" 24x80 -- -t -n 0.1 -g "$COUNTER >/dev/null; cat c; seq 1 30"
done
LOC=C
for seq in '1;31' '0' '' '7' '4;33;44' '38;5;196' '48;5;21' '38;5;9' '38;2;1;2;3' \
           '90;100' '22' '1;' ';1' '39;49' '3;23' '5;25;27' '99' '1;31;4;0;35'; do
  screen "SGR $seq, -c" 24x80 -- -t -c -n 0.1 -g "$COUNTER >/dev/null; printf 'a\\033[${seq}mb\\033[0mc\\n'; cat c"
done
screen "SGR, by default" 24x80 -- -t -n 0.1 -g "$COUNTER >/dev/null; printf 'a\\033[1;31mb\\n'; cat c"
screen "SGR, -C" 24x80 -- -t -C -n 0.1 -g "$COUNTER >/dev/null; printf 'a\\033[1;31mb\\n'; cat c"
screen "SGR, -c -C" 24x80 -- -t -c -C -n 0.1 -g "$COUNTER >/dev/null; printf 'a\\033[1;31mb\\n'; cat c"
screen "SGR, -C -c" 24x80 -- -t -C -c -n 0.1 -g "$COUNTER >/dev/null; printf 'a\\033[1;31mb\\n'; cat c"
screen "ESC ( B, -c" 24x80 -- -t -c -n 0.1 -g "$COUNTER >/dev/null; printf 'a\\033(Bb\\033Xc\\n'; cat c"
TERMV=vt100
screen "SGR on a terminal without colour, -c" 24x80 -- -t -c -n 0.1 -g "$COUNTER >/dev/null; printf 'a\\033[1;31mb\\n'; cat c"
TERMV=xterm-256color

# Differences, and the ways to end.
screen "-d" 24x80 -- -t -d -n 0.1 -g "$COUNTER; echo fixed"
screen "--differences=permanent" 24x80 -- -t --differences=permanent -n 0.1 -q 1 "echo steady"
screen "-d with -q" 24x80 -- -t -d -n 0.1 -q 3 "$COUNTER >/dev/null; echo steady"
screen "-x" 24x80 -- -t -x -n 0.1 -g sh -c "$COUNTER"
screen "-x, no such program" 24x80 'keys:q' -- -t -x -e -n 0.1 no-such-program
screen "-e" 24x80 'keys:k' -- -t -e -n 0.1 "$COUNTER; exit 3"
screen "-b" 24x80 -- -t -b -n 0.1 -g "$COUNTER; exit 1"
screen "-b, killed by a signal" 24x80 -- -t -b -n 0.1 -g "$COUNTER; kill -TERM \$\$"
screen "-n 0.05 is 0.1" 24x80 -- -t -n 0.05 -g "$COUNTER"
ENVX=(LINES=10 COLUMNS=40)
screen "LINES and COLUMNS" 24x80 -- -t -n 0.1 -g "$COUNTER; echo \$LINES \$COLUMNS"
ENVX=(COLUMNS=abc)
screen "COLUMNS=abc" 24x80 -- -t -n 0.1 -g "$COUNTER; echo \$COLUMNS"
ENVX=(LINES=0x14)
screen "LINES=0x14" 24x80 -- -t -n 0.1 -g "$COUNTER; echo \$LINES"
# `strtol`'s `long` is kept in an `int`: 2^32 + 10 is 10, 2^31 negative.
ENVX=(LINES=4294967306 COLUMNS=4294967336)
screen "LINES and COLUMNS past an int" 24x80 -- -t -n 0.1 -g "$COUNTER; echo \$LINES \$COLUMNS"
ENVX=(LINES=2147483648 COLUMNS=2147483648)
screen "LINES and COLUMNS an int makes negative" 24x80 -- -t -n 0.1 -g "$COUNTER; echo \$LINES \$COLUMNS"
ENVX=(COLUMNS=99999999999999999999)
screen "COLUMNS past a long" 24x80 -- -t -n 0.1 -g "$COUNTER; echo \$COLUMNS"
ENVX=(WATCH_INTERVAL=0.1)
screen "WATCH_INTERVAL" 24x80 -- -g "$COUNTER"
screen "WATCH_INTERVAL, then -n" 24x80 -- -n 0.3 -g "$COUNTER"
ENVX=(LINES=10)
screen "LINES=10, resized" 24x80 'resize:30x100' 'signal:INT' -- -t -n 1 'echo hello; echo $LINES $COLUMNS'
ENVX=()

# Standard error not the terminal: watch cannot measure it there, and keeps
# 24 by 80 whatever curses finds -- or what LINES and COLUMNS say, or for
# COLUMNS=abc a width of -1 and an empty screen, which never changes, so -q
# rather than -g ends it.
STDERR='file'
screen "standard error not the terminal" 30x100 -- -t -n 0.1 -g "$COUNTER; echo \$LINES \$COLUMNS"
screen "standard error not the terminal, resized" 30x100 'resize:20x60' 'signal:INT' -- -t -n 1 'echo hello; echo $LINES $COLUMNS'
ENVX=(LINES=10)
screen "standard error not the terminal, LINES=10" 30x100 -- -t -n 0.1 -g "$COUNTER; echo \$LINES \$COLUMNS"
ENVX=(COLUMNS=abc)
screen "standard error not the terminal, COLUMNS=abc" 24x80 -- -t -n 0.1 -q 2 "$COUNTER; echo \$COLUMNS"
ENVX=()
STDERR='tty'

# --- signals and resizes -------------------------------------------------------------
screen "SIGINT" 24x80 'signal:INT' -- -t -n 1 'echo hello'
screen "SIGTERM" 24x80 'signal:TERM' -- -t -n 1 'echo hello'
screen "SIGHUP" 24x80 'signal:HUP' -- -t -n 1 'echo hello'
screen "^C" 24x80 'keys:\x03' -- -t -n 1 'echo hello'
screen "resized, then SIGINT" 24x80 'resize:30x100' 'signal:INT' -- -t -n 1 'echo hello; echo world'
screen "shrunk, then SIGINT" 30x100 'resize:10x40' 'signal:INT' -- -n 1 'echo hello'
screen "resized, -r" 24x80 'resize:20x60' 'signal:INT' -- -t -r -n 1 "$COUNTER"
screen "suspended and back" 24x80 'keys:\x1a' 'signal:INT' -- -t -n 1 'echo hello'

echo "watch-diff: $pass case(s) agree, $fail differ, $xfail differ on purpose, $xpass expected to differ and did not"
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
