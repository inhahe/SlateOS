#!/bin/bash
# Differential test: the curses crate against Ubuntu's libncursesw 6.4.
#
# Both are driven through the same scripts of curses calls, and every
# terminal stream, return value or exit status on which they disagree is
# reported.
#
# ## The reference
#
# Ubuntu's ncurses 6.4+20240113-1ubuntu2.2: `libncursesw.so.6`, through
# `scripts/curses-probe.c`, built here against the installed `curses.h`. The
# subject is `userspace/curses/examples/curses-probe.rs`, which reads the
# same scripts the same way; `curses-probe.c` describes the script language.
#
# ## What is compared
#
# For each case, under each terminal type that this machine's terminfo
# database has and in each locale the case names: everything written to
# standard output -- the bytes a terminal would be sent -- byte for byte;
# what every call returned, as each probe prints it on standard error; and
# the exit status. Standard output is a file, not a terminal, so neither side
# has terminal modes to set (they fail alike, which the return values
# show), and the speed is 0, so nothing is padded with time.
#
# The environment is emptied for each run (`env -i`) and only what a case
# names is set, so that neither side sees `LINES`, `COLUMNS`,
# `NCURSES_NO_PADDING` or anything else from the shell that ran this.
#
# ## Cases that differ on purpose
#
# None.
set -u

DIFF_PROG='curses-probe'
DIFF_PKG='curses'
DIFF_EXAMPLES=curses-probe
DIFF_NO_REF=1
DIFF_NO_BINDIR=1
DIFF_NEED='cc cmp od infocmp'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

KEEP=0
ONLY=''
while [ $# -gt 0 ]; do
  case "$1" in
    --keep) KEEP=1; shift ;;
    --only) ONLY=$2; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
if [ "$KEEP" = 1 ]; then
  diff_cleanup() { echo "working files in $DIFF_TMP"; }
fi

if ! [ -f /usr/include/curses.h ]; then
  echo "curses-diff.sh: libncurses-dev is not installed; skipped"
  exit 0
fi
ref=$DIFF_TMP/ref
if ! cc -O2 -Wall -o "$ref" "$root/scripts/curses-probe.c" -lncursesw; then
  echo "could not build the reference probe" >&2
  exit 1
fi

# The terminal types every case runs under, those the database has.
TERMS=''
for t in xterm-256color xterm xterm-color xterm-mono vt100 vt220 vt52 linux \
  screen screen-256color tmux-256color rxvt rxvt-unicode ansi dumb sun cons25 \
  putty-256color st-256color alacritty konsole gnome; do
  if infocmp "$t" > /dev/null 2>&1; then
    TERMS="$TERMS $t"
  fi
done

pass=0
fail=0
failed=''
work=$DIFF_TMP/work
mkdir -p "$work"

# run SIDE PROG TERM LOCALE MODE ENV... -- SCRIPT: one probe run, its output
# in $work/SIDE.{out,err}. MODE `file` writes to a file; `pty` to a
# pseudo-terminal of the size an environment entry `PTY=ROWSxCOLS` gives
# (24x80 without one), which is not passed on to the probe.
run() {
  local side=$1 prog=$2 term=$3 loc=$4 mode=$5
  shift 5
  local envs=() size=24x80
  while [ "$1" != -- ]; do
    case "$1" in
      PTY=*) size=${1#PTY=} ;;
      *) envs+=("$1") ;;
    esac
    shift
  done
  shift
  if [ "$mode" = pty ]; then
    env -i PATH=/usr/bin:/bin TERM="$term" LC_ALL="$loc" "${envs[@]}" \
      timeout 30 python3 "$root/scripts/curses-ptyrun.py" "${size%x*}" "${size#*x}" \
      "$work/$side.out" "$work/$side.err" "$prog" "$1" < /dev/null
    local rc=$?
    [ "$rc" = 0 ] || echo "runner $rc" >> "$work/$side.err"
  else
    env -i PATH=/usr/bin:/bin TERM="$term" LC_ALL="$loc" "${envs[@]}" \
      "$prog" "$1" < /dev/null > "$work/$side.out" 2> "$work/$side.err"
    echo "exit $?" >> "$work/$side.err"
  fi
}

# check NAME TERM LOCALE MODE ENV... -- SCRIPT: one comparison.
check() {
  local name=$1 term=$2 loc=$3 mode=$4
  shift 4
  run ref "$ref" "$term" "$loc" "$mode" "$@"
  run ours "$OURS" "$term" "$loc" "$mode" "$@"
  if cmp -s "$work/ref.out" "$work/ours.out" && cmp -s "$work/ref.err" "$work/ours.err"; then
    pass=$((pass + 1))
    return
  fi
  fail=$((fail + 1))
  failed="$failed $name/$term/$loc/$mode"
  if [ "${VERBOSE:-0}" = 1 ]; then
    echo "--- $name under $term, $loc, $mode"
    if ! cmp -s "$work/ref.err" "$work/ours.err"; then
      diff -u "$work/ref.err" "$work/ours.err" | head -30
    fi
    if ! cmp -s "$work/ref.out" "$work/ours.out"; then
      cmp "$work/ref.out" "$work/ours.out" | head -1
      echo "reference:"
      od -c "$work/ref.out" | head -24
      echo "ours:"
      od -c "$work/ours.out" | head -24
    fi
  fi
}

# scenario NAME LOCALES [ENV...]: the script on standard input, run under
# every terminal type in each of LOCALES (space-separated), writing both to
# a file and to a terminal -- or only to a terminal when the first ENV is
# `pty-only`.
scenario() {
  local name=$1 locales=$2 modes='file pty'
  shift 2
  if [ "${1:-}" = pty-only ]; then
    modes=pty
    shift
  fi
  if [ -n "$ONLY" ] && [ "$ONLY" != "$name" ]; then
    cat > /dev/null
    return
  fi
  local script=$DIFF_TMP/$name.script
  cat > "$script"
  local t loc mode
  for t in $TERMS; do
    for loc in $locales; do
      for mode in $modes; do
        check "$name" "$t" "$loc" "$mode" "$@" -- "$script"
      done
    done
  done
}

scenario hello 'C C.UTF-8' <<'EOF'
use_env 0
initscr
lines
mvaddstr 0 0 hello
refresh
getyx
endwin
isendwin
EOF

scenario attributes 'C' <<'EOF'
use_env 0
initscr
attron 200000
addstr bold
attroff 200000
addstr  plain
attron 40000
mvaddstr 1 0 reverse
attrset 20000
addstr  underline
attrset 10000
addstr  standout
attrset 100000
addstr  dim
attrset 80000
addstr  blink
attrset 800000
addstr  invis
attrset 80000000
addstr  italic
attrset 240000
mvaddstr 2 3 bold+reverse
standout
mvaddstr 3 0 standout()
standend
addstr  standend
refresh
erase
mvaddstr 5 5 second
attron 200000
mvaddstr 6 6 frame
refresh
endwin
EOF

scenario movement 'C' <<'EOF'
use_env 0
initscr
mvaddstr 0 0 a
mvaddstr 0 79 b
mvaddstr 1 0 c
mvaddstr 1 40 d
mvaddstr 10 10 e
mvaddstr 10 11 f
mvaddstr 10 30 g
mvaddstr 11 30 h
mvaddstr 12 2 i
mvaddstr 22 70 j
mvaddstr 23 0 k
mvaddstr 5 5 l
move 20 5
refresh
mvaddstr 15 15 m
move 0 0
refresh
mvaddstr 3 77 xyz
move 23 78
refresh
endwin
EOF

scenario scrolling 'C' <<'EOF'
use_env 0
initscr
mvaddstr 0 0 line 00 the quick brown fox jumps over the lazy dog
mvaddstr 1 0 line 01 the quick brown fox jumps over the lazy dog
mvaddstr 2 0 line 02 the quick brown fox jumps over the lazy dog
mvaddstr 3 0 line 03 the quick brown fox jumps over the lazy dog
mvaddstr 4 0 line 04 the quick brown fox jumps over the lazy dog
mvaddstr 5 0 line 05 the quick brown fox jumps over the lazy dog
mvaddstr 6 0 line 06 the quick brown fox jumps over the lazy dog
mvaddstr 7 0 line 07 the quick brown fox jumps over the lazy dog
mvaddstr 8 0 line 08 the quick brown fox jumps over the lazy dog
mvaddstr 9 0 line 09 the quick brown fox jumps over the lazy dog
mvaddstr 10 0 line 10 the quick brown fox jumps over the lazy dog
mvaddstr 11 0 line 11 the quick brown fox jumps over the lazy dog
mvaddstr 12 0 line 12 the quick brown fox jumps over the lazy dog
mvaddstr 13 0 line 13 the quick brown fox jumps over the lazy dog
mvaddstr 14 0 line 14 the quick brown fox jumps over the lazy dog
mvaddstr 15 0 line 15 the quick brown fox jumps over the lazy dog
mvaddstr 16 0 line 16 the quick brown fox jumps over the lazy dog
mvaddstr 17 0 line 17 the quick brown fox jumps over the lazy dog
mvaddstr 18 0 line 18 the quick brown fox jumps over the lazy dog
mvaddstr 19 0 line 19 the quick brown fox jumps over the lazy dog
mvaddstr 20 0 line 20 the quick brown fox jumps over the lazy dog
mvaddstr 21 0 line 21 the quick brown fox jumps over the lazy dog
mvaddstr 22 0 line 22 the quick brown fox jumps over the lazy dog
mvaddstr 23 0 line 23 the quick brown fox jumps over the lazy dog
refresh
mvaddstr 0 0 line 01 the quick brown fox jumps over the lazy dog
mvaddstr 1 0 line 02 the quick brown fox jumps over the lazy dog
mvaddstr 2 0 line 03 the quick brown fox jumps over the lazy dog
mvaddstr 3 0 line 04 the quick brown fox jumps over the lazy dog
mvaddstr 4 0 line 05 the quick brown fox jumps over the lazy dog
mvaddstr 5 0 line 06 the quick brown fox jumps over the lazy dog
mvaddstr 6 0 line 07 the quick brown fox jumps over the lazy dog
mvaddstr 7 0 line 08 the quick brown fox jumps over the lazy dog
mvaddstr 8 0 line 09 the quick brown fox jumps over the lazy dog
mvaddstr 9 0 line 10 the quick brown fox jumps over the lazy dog
mvaddstr 10 0 line 11 the quick brown fox jumps over the lazy dog
mvaddstr 11 0 line 12 the quick brown fox jumps over the lazy dog
mvaddstr 12 0 line 13 the quick brown fox jumps over the lazy dog
mvaddstr 13 0 line 14 the quick brown fox jumps over the lazy dog
mvaddstr 14 0 line 15 the quick brown fox jumps over the lazy dog
mvaddstr 15 0 line 16 the quick brown fox jumps over the lazy dog
mvaddstr 16 0 line 17 the quick brown fox jumps over the lazy dog
mvaddstr 17 0 line 18 the quick brown fox jumps over the lazy dog
mvaddstr 18 0 line 19 the quick brown fox jumps over the lazy dog
mvaddstr 19 0 line 20 the quick brown fox jumps over the lazy dog
mvaddstr 20 0 line 21 the quick brown fox jumps over the lazy dog
mvaddstr 21 0 line 22 the quick brown fox jumps over the lazy dog
mvaddstr 22 0 line 23 the quick brown fox jumps over the lazy dog
mvaddstr 23 0 line 24 a new line at the bottom
refresh
mvaddstr 3 0 line 02 the quick brown fox jumps over the lazy dog
mvaddstr 4 0 line 03 the quick brown fox jumps over the lazy dog
mvaddstr 5 0 line 04 the quick brown fox jumps over the lazy dog
mvaddstr 6 0 line 05 the quick brown fox jumps over the lazy dog
mvaddstr 7 0 line 06 the quick brown fox jumps over the lazy dog
mvaddstr 8 0 line 07 the quick brown fox jumps over the lazy dog
mvaddstr 9 0 line 08 the quick brown fox jumps over the lazy dog
mvaddstr 10 0 line 09 the quick brown fox jumps over the lazy dog
mvaddstr 2 0 inserted line
refresh
endwin
EOF

scenario colours 'C' <<'EOF'
use_env 0
initscr
has_colors
can_change_color
start_color
colors
use_default_colors
init_pair 1 1 -1
init_pair 2 2 4
init_pair 3 -1 3
init_pair 4 7 0
attrset 100
mvaddstr 0 0 red on default
attrset 200
mvaddstr 1 0 green on blue
attrset 300
mvaddstr 2 0 default on yellow
attrset 200400
mvaddstr 3 0 bold white on black
attrset 0
mvaddstr 4 0 plain
refresh
init_pair 1 3 -1
refresh
init_color 8 333 333 333
init_color 9 1000 333 333
init_pair 5 8 9
attrset 500
mvaddstr 5 0 redefined
refresh
endwin
EOF

scenario colours-assumed 'C' <<'EOF'
use_env 0
initscr
start_color
init_pair 1 1 0
init_pair 2 0 7
attrset 100
mvaddstr 0 0 red on black
attrset 200
mvaddstr 1 0 black on white
refresh
assume_default_colors 2 -1
attrset 0
mvaddstr 2 0 pair zero now
refresh
endwin
EOF

scenario wide 'C.UTF-8' <<'EOF'
use_env 0
initscr
mvaddnwstr 0 0 -1 日本語のテキスト
mvaddnwstr 1 0 -1 e\xcc\x81 combining acute
mvaddnwstr 2 78 -1 中
mvaddnwstr 3 79 -1 中
mvaddnwstr 4 0 -1 ┌──┐ box │
refresh
mvaddnwstr 0 1 -1 x
mvaddnwstr 0 4 -1 yz
refresh
in_wch
move 0 0
in_wch
endwin
EOF

scenario bytes 'C C.UTF-8' <<'EOF'
use_env 0
initscr
mvaddstr 0 0 tab\there
mvaddstr 1 0 ctl\x01\x02\x1f\x7f
mvaddstr 2 0 hi\x80\x9f\xa0\xff
mvaddstr 3 0 utf8 caf\xc3\xa9 \xe4\xb8\xad
mvaddstr 4 0 new\nline
mvaddstr 6 70 wraps across the end of the line
mvaddstr 9 0 bell\x07
refresh
inch
in_wch
getyx
endwin
EOF

scenario line-drawing 'C C.UTF-8' <<'EOF'
use_env 0
initscr
mvaddch 0 0 40006c
addch 400071
addch 400071
addch 40006b
mvaddch 1 0 400078
mvaddch 1 3 400078
mvaddch 2 0 40006d
addch 400071
addch 400071
addch 40006a
mvaddch 3 0 400060
addch 400061
addch 400066
addch 400067
addch 40007e
addch 40002c
addch 40002b
addch 40002e
addch 40002d
addch 400068
addch 400069
addch 400030
addch 400070
addch 400072
addch 400079
addch 40007a
addch 40007b
addch 40007c
addch 40007d
refresh
endwin
EOF

scenario clearing 'C' <<'EOF'
use_env 0
initscr
mvaddstr 0 0 aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
mvaddstr 1 0 bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
mvaddstr 2 0 cccccccccccccccccccccccccccccccccccccccc
mvaddstr 3 0 dddddddddddddddddddddddddddddddddddddddd
refresh
move 0 20
clrtoeol
move 2 10
clrtobot
refresh
clear
mvaddstr 10 10 after clear
refresh
endwin
EOF

scenario resize 'C' <<'EOF'
use_env 0
initscr
mvaddstr 0 0 top
mvaddstr 23 0 bottom
refresh
is_term_resized 24 80
is_term_resized 30 100
resizeterm 30 100
lines
getmaxyx
mvaddstr 29 90 corner
refresh
resizeterm 10 40
lines
mvaddstr 9 0 small
refresh
resize_term 12 50
lines
refresh
resizeterm 0 10
endwin
EOF

scenario environment-size 'C' LINES=30 COLUMNS=100 <<'EOF'
initscr
lines
mvaddstr 29 99 x
refresh
endwin
EOF

scenario cursor 'C' <<'EOF'
use_env 0
initscr
curs_set 0
mvaddstr 1 1 hidden
refresh
curs_set 2
refresh
curs_set 1
refresh
endwin
EOF

scenario cursor-at-exit 'C' <<'EOF'
use_env 0
initscr
curs_set 0
mvaddstr 1 1 hidden
refresh
endwin
mvaddstr 2 2 back
refresh
curs_set 1
curs_set 1
endwin
EOF

scenario attributes-at-exit 'C' <<'EOF'
use_env 0
initscr
start_color
use_default_colors
init_pair 1 2 -1
attrset 200100
mvaddstr 23 70 green bold
refresh
endwin
EOF

scenario slabtop-like 'C' <<'EOF'
initscr
resizeterm 24 80
move 0 0
printw  Active / Total Objects (% used)    : 1234 / 5678 (21.7%)\n
printw  Active / Total Slabs (% used)      : 12 / 34 (35.3%)\n
printw  Active / Total Caches (% used)     : 98 / 140 (70.0%)\n
printw  Active / Total Size (% used)       : 4096.00K / 8192.00K (50.0%)\n
printw  Minimum / Average / Maximum Object : 0.01K / 0.19K / 8.00K\n\n
attron 40000
printw   OBJS ACTIVE  USE OBJ SIZE  SLABS OBJ/SLAB CACHE SIZE NAME                   \n
attroff 40000
printw   1234   1000  81%    0.19K     56       21      224K kmalloc-192            \n
printw    999    900  90%    0.06K     15       64       60K kmalloc-64             \n
printw     88     80  90%    1.00K     22        4      352K inode_cache            \n
refresh
move 0 0
printw  Active / Total Objects (% used)    : 1240 / 5678 (21.8%)\n
printw  Active / Total Slabs (% used)      : 12 / 34 (35.3%)\n
printw  Active / Total Caches (% used)     : 98 / 140 (70.0%)\n
printw  Active / Total Size (% used)       : 4100.00K / 8192.00K (50.0%)\n
printw  Minimum / Average / Maximum Object : 0.01K / 0.19K / 8.00K\n\n
attron 40000
printw   OBJS ACTIVE  USE OBJ SIZE  SLABS OBJ/SLAB CACHE SIZE NAME                   \n
attroff 40000
printw    999    950  95%    0.06K     15       64       60K kmalloc-64             \n
printw   1234   1000  81%    0.19K     56       21      224K kmalloc-192            \n
printw     88     80  90%    1.00K     22        4      352K inode_cache            \n
refresh
endwin
EOF

scenario watch-like 'C' <<'EOF'
initscr
nonl
noecho
cbreak
mvaddstr 0 0 Every 2.0s: date
mvaddstr 0 58 host: Thu Oct  9 12:00:00 2026
mvaddstr 2 0 first output line
mvaddstr 3 0 second output line
refresh
mvaddstr 0 58 host: Thu Oct  9 12:00:02 2026
move 2 0
addstr first output line
move 3 0
standout
addstr S
standend
addstr econd changed line
refresh
resizeterm 24 80
clear
mvaddstr 0 0 Every 2.0s: date
refresh
endwin
EOF

scenario endwin-and-back 'C' <<'EOF'
use_env 0
initscr
mvaddstr 2 2 before
refresh
endwin
isendwin
mvaddstr 3 3 after
refresh
isendwin
endwin
endwin
EOF

scenario bell 'C' <<'EOF'
use_env 0
initscr
beep
refresh
endwin
EOF

scenario modes 'C' <<'EOF'
use_env 0
initscr
nonl
noecho
cbreak
nocbreak
nl
echo
mvaddstr 0 0 modes
refresh
endwin
EOF

scenario edges 'C' <<'EOF'
use_env 0
initscr
move 24 0
move -1 0
mvaddstr 23 75 overflow the corner
addnstr 0 zero
addnstr 3 three
mvaddnstr 5 5 2 two
printw printed %s
addch 0
refresh
endwin
EOF

scenario init-and-end 'C' <<'EOF'
initscr
endwin
EOF

scenario window-size 'C' pty-only PTY=30x100 <<'EOF'
initscr
lines
mvaddstr 29 99 x
refresh
endwin
EOF

scenario window-changes 'C' pty-only <<'EOF'
initscr
mvaddstr 0 0 before
refresh
winch 30 100
refresh
lines
getmaxyx
mvaddstr 29 0 after growing
refresh
winch 10 40
refresh
lines
winch 10 40
refresh
endwin
EOF

scenario suspend-and-resume 'C C.UTF-8' pty-only <<'EOF'
initscr
start_color
init_pair 1 1 0
attrset 100
mvaddstr 3 3 before suspending
curs_set 0
refresh
suspend
isendwin
mvaddstr 4 4 after
refresh
endwin
EOF

scenario suspend-while-ended 'C' pty-only <<'EOF'
initscr
mvaddstr 1 1 x
refresh
endwin
suspend
isendwin
endwin
EOF

scenario interrupt 'C' pty-only <<'EOF'
initscr
mvaddstr 1 1 interrupted
refresh
interrupt
refresh
endwin
EOF

scenario terminate 'C' pty-only <<'EOF'
initscr
attron 40000
mvaddstr 1 1 terminated
refresh
terminate
EOF

scenario terminate-before-refresh 'C' pty-only <<'EOF'
initscr
mvaddstr 1 1 never shown
terminate
EOF

# Terminal types the database has no entry for, and none at all.
printf 'initscr\nrefresh\nendwin\n' > "$DIFF_TMP/missing.script"
for t in no-such-terminal ''; do
  for mode in file pty; do
    check missing "$t" C "$mode" -- "$DIFF_TMP/missing.script"
  done
done

if [ -n "$failed" ]; then
  echo "differ:$failed"
fi
echo "curses-diff: $pass case(s) agree, $fail differ (terminals:$TERMS)"
[ "$fail" = 0 ]
