#!/usr/bin/env bash
# Differential test: our `cfdisk` against Ubuntu's, util-linux 2.39.3's.
#
# cfdisk is a full-screen dialog: each case runs a side on a pseudo-terminal
# of its own (`scripts/curses-ptyrun.py`), types its keys there a step at a
# time -- each step once the program has drawn and gone quiet -- and
# compares everything the terminal was sent, byte for byte; what it wrote to
# standard error and its exit status; and the image it left. Each side works
# on its own copy of the case's starting image, in a directory of its own,
# under the same name, so every message names the same file. Nothing needs
# root: the devices are image files.
#
# The terminal is xterm-256color, 24 by 80, in C.UTF-8 unless a case says
# otherwise (`CF_TERM`, `CF_SIZE`, `CF_LOC`). Its keys are typed as that
# terminal sends them in keypad mode, which cfdisk turns on: the down arrow
# is `\eOB`, page down `\e[6~`.
#
# What is random is masked: a new table's identifier (a DOS label's
# `0x........`, a GPT's GUIDs), wherever cfdisk shows it and in the image.
# An image is compared by what Ubuntu's `sfdisk --dump` says of it and by
# the bytes of its first sector but the DOS disk identifier's four; a script
# the `u` command dumped, by its text.
set -u

DIFF_PROG='cfdisk'
DIFF_PKG='cfdisk'
DIFF_NEED='timeout sfdisk python3'
# Every case types its keys a step at a time, each after a pause for the
# program to go quiet, on both sides.
DIFF_TIMEOUT=${DIFF_TIMEOUT:-3600}
# To look at one case: CF_ONLY and CF_KEEP, in compare() below.
DIFF_FORWARD='CF_ONLY CF_KEEP'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0

fx=$DIFF_TMP/fx
mkdir -p "$fx"

# blank NAME MIB: an image of MIB MiB of zeros.
blank() { truncate -s "$2M" "$fx/$1"; }
# table NAME MIB SCRIPT: an image laid out by Ubuntu's sfdisk -- made once,
# and copied to both sides, so both start from the same bytes.
table() {
  blank "$1" "$2"
  printf '%b' "$3" | sfdisk -q --no-reread --no-tell-kernel "$fx/$1" >/dev/null 2>&1 \
    || { echo "cfdisk-diff: fixture $1 could not be made" >&2; exit 2; }
}

blank empty.img 64
table dos.img 64 'label: dos\nlabel-id: 0x12345678\n,8M,83\n,8M,82,*\n,16M,5\n,4M,83\n,,8e\n'
table gpt.img 64 'label: gpt\nlabel-id: 4A6F7B1C-1111-4222-8333-944455566677\nsize=8M, type=L, uuid=01234567-89AB-4CDE-8F01-23456789ABCD, name="first"\nsize=16M, type=S, uuid=11234567-89AB-4CDE-8F01-23456789ABCD\n'
table gaps.img 64 'label: dos\nlabel-id: 0x0a0b0c0d\n10240,4096,83\n2048,4096,83\n'
# In disk order, a partition between two free spaces, and a gap that is no
# whole number of MiB (6144 to 9999).
table odd.img 64 'label: dos\nlabel-id: 0x0d0d0d0d\n2048,4096,83\n10000,4096,83\n'
# A device its user may read and not write: each side's copy keeps the mode.
cp "$fx/dos.img" "$fx/ro.img" && chmod 444 "$fx/ro.img"
table full.img 16 'label: dos\nlabel-id: 0x01020304\n,4M\n,4M\n,4M\n,,\n'
# More partitions than the screen has lines for the table: it pages.
many='label: gpt\nlabel-id: 5A6F7B1C-1111-4222-8333-944455566677\n'
for i in $(seq 1 24); do
  many="${many}size=1M, type=L, uuid=$(printf '%08X' "$i")-89AB-4CDE-8F01-23456789ABCD\n"
done
table many.img 64 "$many"
printf 'label: dos\nunit: sectors\n,10M,83\n,,82,*\n' > "$fx/load.sfdisk"

# mask FILE...: a new table's random identifiers.
mask() {
  sed -E -i \
    -e 's/(disk identifier |identifier: |label-id: )0x[0-9a-f]{8}/\1ID/g' \
    -e 's/[0-9A-F]{8}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{12}/GUID/g' "$@"
}

# The state a run left: the image as sfdisk reads it, its first sector's
# bytes but the DOS disk identifier's, and a dumped script.
state() {
  local d=$DIFF_TMP/run-$1 img=$DIFF_TMP/run-$1/img
  if [ -f "$img" ]; then
    echo '-- sfdisk --dump'
    sfdisk --dump "$img" 2>&1 | sed "s|$d/||"
    echo '-- first sector'
    python3 - "$img" <<'PY'
import sys
b = open(sys.argv[1], "rb").read(512)
print((b[:440] + b"\0\0\0\0" + b[444:]).hex())
PY
  fi
  if [ -f "$d/script" ]; then
    echo '-- script'
    sed "s|$d/||" "$d/script"
  fi
}

# One side's run on its own copy of the image, from its own directory, on a
# terminal: STEPS is the `|`-separated actions to do on it.
run_side() {
  local side=$1 src=$2 steps=$3; shift 3
  local d=$DIFF_TMP/run-$side size=${CF_SIZE:-24x80}
  local thens=() step
  rm -rf "$d"; mkdir -p "$d"
  [ -n "$src" ] && cp "$fx/$src" "$d/img"
  if [ -n "$steps" ]; then
    IFS='|' read -r -a parts <<< "$steps"
    for step in "${parts[@]}"; do
      thens+=(--then "$step")
    done
  fi
  (cd "$d" && env -i PATH="$bindir/$side" TERM="${CF_TERM:-xterm-256color}" \
    LC_ALL="${CF_LOC:-C.UTF-8}" HOME="$d" \
    /usr/bin/timeout -k 2 60 /usr/bin/python3 "$root/scripts/curses-ptyrun.py" "${thens[@]}" \
    "${size%x*}" "${size#*x}" "$d/out" "$d/err" cfdisk "$@" < /dev/null)
  local rc=$?
  [ "$rc" = 0 ] || echo "runner $rc" >> "$d/err"
}

# compare FIXTURE STEPS ARGS...: `cfdisk ARGS...` with the STEPS done on its
# terminal, on a copy of FIXTURE as `img` ("" for none).
compare() {
  local src=$1 steps=$2; shift 2
  local o=$DIFF_TMP/run-ours g=$DIFF_TMP/run-gnu
  local o_all=$DIFF_TMP/o.all g_all=$DIFF_TMP/g.all
  # CF_ONLY=STRING: only the cases whose fixture, steps and arguments,
  # written as `FIXTURE STEPS ARGS`, contain STRING -- to look at one.
  case "$src $steps $*" in *"${CF_ONLY:-}"*) ;; *) return 0 ;; esac
  run_side ours "$src" "$steps" "$@"
  { cat "$o/out"; echo; echo '-- stderr'; cat "$o/err"; state ours; } > "$o_all"
  run_side gnu "$src" "$steps" "$@"
  { cat "$g/out"; echo; echo '-- stderr'; cat "$g/err"; state gnu; } > "$g_all"
  # A run that never reached the terminal is no run: both sides empty would
  # otherwise agree.
  if ! [ -f "$o/out" ] || ! [ -f "$g/out" ] || ! [ -s "$g/err" ]; then
    echo "cfdisk-diff: a run produced nothing: $src $steps $*" >&2
    exit 2
  fi
  # Every case ends by quitting: Ubuntu's cfdisk still running at the time
  # limit, or a step the runner refused, is a case that tests nothing past
  # the point it stuck -- and ours, stuck at the same point, would agree.
  # Ours alone stuck is a difference, and is compared as one.
  if grep -q '^runner ' "$g/err"; then
    echo "cfdisk-diff: Ubuntu's cfdisk did not finish ($(grep '^runner ' "$g/err")): $src $steps $*" >&2
    exit 2
  fi
  mask "$o_all" "$g_all"
  # CF_KEEP=DIR: the last case's two transcripts kept, as DIR/gnu.all and
  # DIR/ours.all, to read with less than `od -c`.
  if [ -n "${CF_KEEP:-}" ]; then
    mkdir -p "$CF_KEEP" && cp "$g_all" "$CF_KEEP/gnu.all" && cp "$o_all" "$CF_KEEP/ours.all"
  fi
  local label
  label=$(printf '[%s] cfdisk %s <<< %q (%s, %s, %s)' "$src" "$*" "$steps" \
    "${CF_TERM:-xterm-256color}" "${CF_LOC:-C.UTF-8}" "${CF_SIZE:-24x80}")
  if cmp -s "$o_all" "$g_all"; then
    pass=$((pass + 1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail + 1))
    printf 'DIFF %s\n' "$label"
    if [ -n "${VERBOSE:-}" ]; then
      cmp "$g_all" "$o_all" | head -1 | sed 's/^/  /'
      diff -u --label gnu --label ours <(od -c "$g_all") <(od -c "$o_all") | sed -n '3,40p' | sed 's/^/  /'
    fi
    if [ -n "${DIFF_FAIL_FAST:-}" ]; then exit 1; fi
  fi
  return 0
}

# c FIXTURE STEPS [ARGS...]: the dialog on `img`.
c() { local src=$1 steps=$2; shift 2; compare "$src" "$steps" "$@" img; }

# Keys, as xterm sends them in keypad mode.
DOWN='keys:\eOB'
UP='keys:\eOA'
RIGHT='keys:\eOC'
LEFT='keys:\eOD'
ENTER='keys:\r'
QUIT='keys:q'

# --- options, usage and refusals -----------------------------------------------
compare "" "" -h
compare "" "" --help
compare "" "" -V
compare "" "" --version
compare "" "" --bogus
compare "" "" -Lrainbow img
compare "" "" nonexistent.img
compare "" "" -r nonexistent.img
compare "" "" a b

# --- looking: moving about, the extra information, help, redrawing ---------------
c dos.img "$QUIT"
c dos.img "$DOWN|$DOWN|$DOWN|$UP|$QUIT"
c dos.img "keys:\eOF|keys:\eOH|keys:j|keys:k|$QUIT"
c dos.img "keys:x|$DOWN|keys:x|$DOWN|$QUIT"
c dos.img "keys:h|keys: |$QUIT"
c dos.img "$RIGHT|$RIGHT|$LEFT|keys:\t|keys:\e[Z|$QUIT"
c dos.img "keys:\x0c|$QUIT"
c dos.img "$ENTER|$ENTER" -r
c gpt.img "$DOWN|$DOWN|$QUIT"
c gpt.img "keys:x|$QUIT"
c many.img "keys:\e[6~|keys:\e[6~|keys:\e[5~|$DOWN|keys:\eOF|$QUIT"
# Down a line at a time to the first page's last row: its arrows to the
# next page are drawn with the cursor's reverse video.
c many.img "keys:\eOB\eOB\eOB\eOB\eOB\eOB\eOB\eOB\eOB\eOB\eOB\eOB\eOB\eOB|$QUIT"
c gaps.img "$DOWN|$DOWN|$QUIT"
c full.img "$DOWN|$DOWN|$DOWN|$QUIT"

# --- changing: delete, bootable, type, resize, sort, new -------------------------
c dos.img "keys:d|keys:W|keys:yes|$ENTER|$QUIT"
c dos.img "keys:b|$DOWN|keys:b|keys:W|keys:yes\r|$QUIT"
c dos.img "keys:\e[3~|$QUIT"
c dos.img "keys:t|$DOWN|$DOWN|$ENTER|keys:W|keys:yes\r|$QUIT"
c dos.img "keys:t|keys:\e[6~|keys:\e[5~|keys:q|$QUIT"
# A type chosen with Ctrl-J and with the keypad's Enter, as with Return.
c dos.img "keys:t|$DOWN|keys:\n|$QUIT"
c dos.img "keys:t|$DOWN|keys:\eOM|$QUIT"
c gpt.img "keys:t|keys:\e[6~|keys:\e[6~|$DOWN|$ENTER|keys:W|keys:yes\r|$QUIT"
c gpt.img "keys:r|keys:\x7f\x7f\x7f\x7f\x7f\x7f4M\r|keys:W|keys:yes\r|$QUIT"
c gpt.img "keys:r|keys:\e|$QUIT"
c gaps.img "keys:s|keys:W|keys:yes\r|$QUIT"
# gaps.img lists its partitions out of disk order, so its gap -- the free
# space between them -- is the third line. The size dialog stays open after
# a size it refuses, until Escape; an empty one it takes for 0 (a C
# `continue` that reaches its do-while's condition), and goes on to the
# primary-or-extended menu, which only Enter leaves.
c gaps.img "$DOWN|$DOWN|keys:n|keys:\r|$ENTER|keys:W|keys:yes\r|$QUIT"
c gaps.img "$DOWN|$DOWN|keys:n|keys:\x7f\x7f\x7f\x7f\x7f\x7f1M\r|keys:\eOC|keys:\r|$QUIT"
c gaps.img "$DOWN|$DOWN|keys:n|keys:abc\r|keys:\e|$QUIT"
c gaps.img "$DOWN|$DOWN|keys:n|keys:\x7f\x7f\x7f\x7f\x7f\x7f\r|$ENTER|$QUIT"
c gaps.img "$DOWN|$DOWN|keys:n|keys:\x7f\x7f\x7f\x7f\x7f\x7f999G\r|keys:\e|$QUIT"
c gaps.img "$DOWN|$DOWN|keys:n|keys:\eOD\x7f9\r|keys:\e|$QUIT"
c gaps.img "$DOWN|$DOWN|keys:n|keys:\x7f\x7f\x7f\x7f\x7f\x7f2048S\r|$ENTER|keys:W|keys:yes\r|$QUIT"
# A size in sectors past the free space: refused, the prompt comes back
# without its S, and Enter there means bytes.
c gaps.img "$DOWN|$DOWN|keys:n|keys:\x7f\x7f\x7f\x7f\x7f\x7f999999999S\r|$ENTER|keys:\e|$QUIT"
# A size with a suffix that rounds past the free space, by less than the
# suffix's unit, is all of it (2M in 1.88 MiB).
c odd.img "$DOWN|keys:n|keys:\x7f\x7f\x7f\x7f\x7f\x7f\x7f2M\r|$ENTER|$QUIT"
# Deleting the partition between two free spaces leaves two lines fewer,
# and the cursor past the last: it moves to the last.
c odd.img "$DOWN|$DOWN|keys:d|$QUIT"
# A dialog's text edited: Left over several characters, Home, End; and
# Left and Backspace over characters of more than one byte (é, €).
c gpt.img "keys:u|keys:abcd\eOD\eODX\eOHY\eOFZ\r|$QUIT"
c gpt.img "keys:u|keys:a\xc3\xa9\xe2\x82\xac\eODX\eOD\eOD\x7fY\r|$QUIT"
# Right and Delete in a dialog's text, over characters of one byte and of
# more.
c gpt.img "keys:u|keys:abcd\eOH\eOC\e[3~X\r|$QUIT"
c gpt.img "keys:u|keys:a\xc3\xa9\xe2\x82\xacb\eOH\eOC\e[3~X\r|$QUIT"
# A device that cannot be written is opened read-only, and says so; W then
# does nothing.
c ro.img "keys:W|$QUIT"
# A dialog's text ended by Ctrl-J or the keypad's Enter, as by Return.
c gpt.img "keys:u|keys:abc\n|$QUIT"
c gpt.img "keys:u|keys:abd\eOM|$QUIT"
# A size in sectors above the 1 MiB grain, written: aligned like any other,
# since upstream marks the free space, not the new partition, as sized
# explicitly.
c gaps.img "$DOWN|$DOWN|keys:n|keys:\x7f\x7f\x7f\x7f\x7f\x7f3000S\r|$ENTER|keys:W|keys:yes\r|$QUIT"
# The primary-or-extended menu takes only Enter: x there is nothing, so
# Down then picks extended.
c odd.img "$DOWN|keys:n|keys:\r|keys:x|$DOWN|$ENTER|keys:W|keys:yes\r|$QUIT"
# The smallest size is one sector: 512 bytes is taken, 100 refused.
c odd.img "$DOWN|keys:n|keys:\x7f\x7f\x7f\x7f\x7f\x7f\x7f512\r|$ENTER|$QUIT"
c odd.img "$DOWN|keys:n|keys:\x7f\x7f\x7f\x7f\x7f\x7f\x7f100\r|keys:\e|$QUIT"
# The type menu's last page, reached with End: no arrow for a page after it.
c dos.img "keys:t|keys:\eOF|keys:\e|$QUIT"
# Home in the paged type menu goes to the first item, not a page up.
c dos.img "keys:t|keys:\eOF|keys:\eOH|$ENTER|$QUIT"
# A size over the free space by exactly its suffix's unit is refused, not
# taken for all of it: 3M in gaps.img's gap of exactly 2 MiB.
c gaps.img "$DOWN|$DOWN|keys:n|keys:\x7f\x7f\x7f\x7f\x7f\x7f3M\r|keys:\e|$QUIT"
# The box of the current partition's details needs three free lines under
# the table: dos.img's leaves two, three and four on these screens.
for s in 17x80 18x80 19x80; do
  CF_SIZE=$s c dos.img "$QUIT"
done

# --- writing, or not -------------------------------------------------------------
c dos.img "keys:d|keys:W|keys:no\r|$QUIT"
c dos.img "keys:d|keys:W|keys:\e|$QUIT"
c dos.img "keys:W|$QUIT" -r
c dos.img "keys:u|keys:script\r|$QUIT"

# --- a disk with no table: the label menu ----------------------------------------
c empty.img "$ENTER|$QUIT"
c empty.img "$DOWN|$ENTER|keys:n|$ENTER|$ENTER|keys:W|keys:yes\r|$QUIT"
c empty.img "$DOWN|$DOWN|$DOWN|$UP|$ENTER|$QUIT"
c empty.img "keys:q"
c empty.img "keys:l|keys:../fx/load.sfdisk\r|keys:W|keys:yes\r|$QUIT"
c empty.img "keys:l|keys:nosuchfile\r|keys:q"
c dos.img "$ENTER|$QUIT" -z

# --- the window resized, and other terminals and locales -------------------------
c dos.img "resize:20x60|keys:x|$QUIT"
c dos.img "keys:t|resize:16x50|$DOWN|keys:q|$QUIT"
c dos.img "resize:40x120|$DOWN|$QUIT"
CF_SIZE=12x40 c dos.img "$DOWN|keys:h|keys: |$QUIT"
CF_LOC=C c dos.img "keys:x|$DOWN|$QUIT"
CF_TERM=linux c dos.img 'keys:\e[B|keys:\e[B|keys:t|keys:\e[B|keys:q|keys:q'
CF_TERM=vt100 c gpt.img 'keys:\eOB|keys:x|keys:q'

printf '\ncfdisk-diff: %d passed, %d differed\n' "$pass" "$fail"
[ "$fail" = 0 ]
