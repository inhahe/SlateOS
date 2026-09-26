#!/usr/bin/env bash
# Differential test: our `lsmem` against util-linux 2.39.3's.
#
# What is compared: stdout, stderr and the exit status of each case, on
#
#   * WSL's own /sys: every output format, every column, the splits, the
#     summaries, and all of it on a terminal of many widths (inside
#     `script`'s pty), which is where libsmartcols' width arithmetic --
#     ported as the `smartcols` crate -- decides what is cut and what grows;
#   * memory trees built under $DIFF_TMP and read with --sysroot: states,
#     removability, nodes, zones and the attributes' malformed spellings;
#     gaps, `versionsort` order, block sizes without a newline, empty, too big
#     or not hex; and the trees lsmem refuses, with the messages it refuses
#     them with;
#   * the option parser and its refusals, and write errors (/dev/full, a
#     closed stdout) with small and large output.
#
# Two messages carry whatever errno holds (main.rs, "What is not
# upstream's"); in a UTF-8 locale upstream's includes what glibc's setlocale
# left while looking for locale files, so those cases run in the C locale,
# where the two agree.
set -u

DIFF_PROG='lsmem'
DIFF_PKG='lsmem'
DIFF_NEED='timeout script stty'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; broken=0

T=$DIFF_TMP/trees
mkdir -p "$T"

# ENVS: extra `env` words for the next cases. CLOC: run in the C locale with
# nothing else in the environment.
ENVS=()
CLOC=

# $1 = side, rest = argv.
run_side() {
  local side=$1; shift
  if [ -n "$CLOC" ]; then
    diff_run env -i PATH="$bindir/$side:/usr/bin:/bin" "${ENVS[@]}" \
      timeout -k 2 20 lsmem "$@"
  else
    diff_run env LC_ALL=C.UTF-8 "${ENVS[@]}" PATH="$bindir/$side:$PATH" \
      timeout -k 2 20 lsmem "$@"
  fi
}
# $1 = side, $2 = a bash script that runs `lsmem` from that side's PATH.
run_side_sh() {
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/$1:$PATH" timeout -k 2 20 bash -c "$2"
}
# $1 = side, $2 = columns, rest = argv: on a pty of that width.
run_side_pty() {
  local side=$1 cols=$2 cmd; shift 2
  cmd="stty cols $cols rows 60; lsmem$(printf ' %q' "$@")"
  diff_run env LC_ALL=C.UTF-8 PATH="$bindir/$side:$PATH" \
    timeout -k 2 20 script -qec "$cmd" /dev/null </dev/null
}

judge() {
  local o_rc=$1 g_rc=$2
  if [ "$o_rc" = 127 ] || [ "$g_rc" = 127 ] || [ "$o_rc" = 124 ] || [ "$g_rc" = 124 ]; then
    AGREED=broken
  elif cmp -s "$DIFF_TMP/o.out" "$DIFF_TMP/g.out" \
     && cmp -s "$DIFF_TMP/o.err" "$DIFF_TMP/g.err" && [ "$o_rc" = "$g_rc" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out %q err %q\n  gnu  (rc=%s): out %q err %q' \
    "$o_rc" "$(head -c 3000 "$DIFF_TMP/o.out")" "$(cat "$DIFF_TMP/o.err")" \
    "$g_rc" "$(head -c 3000 "$DIFF_TMP/g.out")" "$(cat "$DIFF_TMP/g.err")")
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

# case_ ARGV...
case_() {
  local o_rc g_rc
  run_side ours "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side gnu "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "${CLOC:+[C locale] }${ENVS[*]:+${ENVS[*]} }lsmem $(printf '%q ' "$@")"
}
# sh_case SCRIPT: a bash script calling `lsmem`, run for each side.
sh_case() {
  local o_rc g_rc
  run_side_sh ours "$1" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side_sh gnu "$1" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "bash -c $(printf '%q' "$1")"
}
# pty_case COLS ARGV...
pty_case() {
  local cols=$1 o_rc g_rc; shift
  run_side_pty ours "$cols" "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side_pty gnu "$cols" "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  judge "$o_rc" "$g_rc"
  report "[pty ${cols} cols] lsmem $(printf '%q ' "$@")"
}

# tree NAME [BLOCKSIZE]: a memory directory under $T/NAME, with
# block_size_bytes holding BLOCKSIZE byte for byte when given. M names it.
tree() {
  M=$T/$1/sys/devices/system/memory
  mkdir -p "$M"
  if [ $# -ge 2 ]; then printf '%b' "$2" > "$M/block_size_bytes"; fi
}
# blk N STATE REMOVABLE ZONES NODE: memoryN in the current tree, each
# attribute written with a newline after it; `-` leaves that one out.
blk() {
  local b=$M/memory$1
  mkdir -p "$b"
  if [ "$2" != - ]; then printf '%s\n' "$2" > "$b/state"; fi
  if [ "$3" != - ]; then printf '%s\n' "$3" > "$b/removable"; fi
  if [ "$4" != - ]; then printf '%s\n' "$4" > "$b/valid_zones"; fi
  if [ "$5" != - ]; then mkdir -p "$b/node$5"; fi
  return 0
}
# on NAME ARGV...: a case reading tree NAME.
on() { local name=$1; shift; case_ --sysroot "$T/$name" "$@"; }

# --- options and their refusals -----------------------------------------------
case_ -h
case_ --help
case_ -V
case_ --version
case_ --vers
case_ -Z
case_ --bogus
case_ --s
case_ --su
case_ --sum
case_ --summ=only
case_ --summary=only
case_ --summary=never
case_ --summary=always
case_ --summary=bogus
case_ --summary=
case_ --summary only
case_ --json=1
case_ -o
case_ --output
case_ -S
case_ -s
case_ foo
case_ -J foo
case_ -- -J
case_ -J -r
case_ -J -P
case_ -P -r
case_ -r -J
case_ -J -J
case_ -S NODE -a
case_ -a -S NODE
case_ -a -a
case_ -J --summary=only
case_ -r --summary
case_ -P --summary=never
case_ -J --summary=always
case_ -r --summary=always -b
case_ -o FOO
case_ -o FOO,SIZE
case_ -o RANGE,FOO,SIZE
case_ -o RANGE,,SIZE
case_ -o ,
case_ -o +
case_ -o ''
case_ -o SIZE,
case_ -o range,size
case_ -o +NODE,ZONES
case_ -o +node -o +zones
case_ -o RANGE,RANGE,RANGE
case_ --output-all -o +RANGE,SIZE,STATE,REMOVABLE,BLOCK,NODE,ZONES
case_ --output-all -o +RANGE,SIZE,STATE,REMOVABLE,BLOCK,NODE,ZONES,RANGE
case_ -o RANGE,SIZE,STATE,REMOVABLE,BLOCK,NODE,ZONES,RANGE,SIZE,STATE,REMOVABLE,BLOCK,NODE,ZONES
case_ -o RANGE,SIZE,STATE,REMOVABLE,BLOCK,NODE,ZONES,RANGE,SIZE,STATE,REMOVABLE,BLOCK,NODE,ZONES,RANGE
case_ -S BOGUS
case_ -S ''
case_ -S none
case_ -S NONE
case_ -S none,NODE
case_ -S RANGE,SIZE,STATE,REMOVABLE,BLOCK,NODE,ZONES
case_ -S RANGE,SIZE,STATE,REMOVABLE,BLOCK,NODE,ZONES,RANGE
case_ -S +NODE
case_ --summary=only -o BOGUS
case_ -b --summary=only -S BOGUS

# --- WSL's own /sys ------------------------------------------------------------
for args in '' '-a' '-b' '-n' '-r' '-P' '-J' '-J -b' '-J -a' '-P -b' '-r -n' \
            '-J -n' '--output-all' '--output-all -b' '--output-all -J' \
            '--output-all -P' '--output-all -r' '-o +NODE,ZONES' \
            '-o ZONES,NODE,BLOCK,REMOVABLE,STATE,SIZE,RANGE' '-o RANGE' \
            '-o SIZE -b' '-o NODE' '-o BLOCK,BLOCK' '-S none' '-S NODE' \
            '-S ZONES' '-S STATE -o RANGE' '-o RANGE -S none' '-a -o BLOCK' \
            '-a -b -r' '--summary' '--summary=only -b' '--summary=never' \
            '--summary=always -J' '-a -J --summary=always'; do
  # Word-split on purpose: each entry is an argument list.
  # shellcheck disable=SC2086
  case_ $args
done

# --- on a terminal, at widths from too narrow to wide -----------------------------
for cols in 1 10 20 30 38 39 40 45 50 55 60 64 65 70 75 80 100 132 250; do
  pty_case "$cols"
  pty_case "$cols" --output-all
  pty_case "$cols" -o RANGE
  pty_case "$cols" -o RANGE,NODE,ZONES
  pty_case "$cols" -o ZONES,RANGE
done
for cols in 20 40 60 80; do
  pty_case "$cols" -a
  pty_case "$cols" -r --output-all
  pty_case "$cols" -J -o RANGE,SIZE
  pty_case "$cols" -P
  pty_case "$cols" -n -o STATE,RANGE
  pty_case "$cols" --summary=only
done

# --- memory trees ---------------------------------------------------------------
# States, removability, nodes and zones, all mixed.
tree basic '8000000\n'
blk 0 online 1 DMA32 0
blk 1 online 1 DMA32 0
blk 2 online 1 Normal 0
blk 3 offline 1 Normal 0
blk 4 online 0 Normal 1
blk 5 online 0 Normal 1
blk 6 going-offline 0 'Normal Movable' 1
blk 7 - 0 'Normal Movable' 1
blk 8 online 1 'Normal Movable' 1
blk 9 online 1 'Movable Normal' 1
for args in '' '-a' '-b' '-n' '-r' '-P' '-J' '-J -b' '-J -a' '--output-all' \
            '--output-all -J' '--output-all -P' '--output-all -r -b' \
            '-o +NODE,ZONES' '-o NODE,ZONES' '-S none' '-S NODE' '-S ZONES' \
            '-S STATE' '-S REMOVABLE' '-S STATE,REMOVABLE -o +NODE' \
            '-S none -o +NODE,ZONES' '--summary' '--summary=only -b' \
            '--summary=always -P'; do
  # shellcheck disable=SC2086
  on basic $args
done
pty_case 50 --sysroot "$T/basic" --output-all
pty_case 80 --sysroot "$T/basic" --output-all
pty_case 120 --sysroot "$T/basic" --output-all -a

# Gaps between blocks.
tree gaps '8000000\n'
for n in 0 2 3 10 11 12 40; do blk "$n" online 1 Normal 0; done
on gaps
on gaps -a
on gaps -S none
on gaps --summary=only

# versionsort: numbers compare as numbers, leading zeros as fractions -- and
# memory01 and memory1 are both block 1.
tree order '8000000\n'
for n in 9 10 1 01 001 010 100 0 00 2; do blk "$n" online 1 Normal 0; done
on order
on order -a
on order -a -o BLOCK,RANGE

# No valid_zones anywhere, and none on memory0 alone: have_zones is decided by
# memory0.
tree nozones '8000000\n'
for n in 0 1 2; do blk "$n" online 1 - 0; done
on nozones --output-all
on nozones -J -o ZONES,NODE
tree zones_not_on_0 '8000000\n'
blk 0 online 1 - 0
blk 1 online 1 Normal 0
blk 2 online 1 Movable 0
on zones_not_on_0 --output-all
on zones_not_on_0 -S ZONES -o +ZONES

# Nodes are decided by the first block in versionsort order.
tree node_not_on_first '8000000\n'
blk 0 online 1 Normal -
blk 1 online 1 Normal 0
blk 2 online 1 Normal 1
on node_not_on_first --output-all
tree node_on_some '8000000\n'
blk 0 online 1 Normal 0
blk 1 online 1 Normal -
blk 2 online 1 Normal 1
blk 3 online 1 Normal 99999999999999999999
blk 4 online 1 Normal 4294967298
blk 5 online 1 Normal x
on node_on_some --output-all
on node_on_some -J -o NODE,BLOCK -a

# The attributes' odd spellings.
tree odd '8000000\n'
blk 0 online 2 Normal 0
blk 1 online ' 1' normal 0
blk 2 online abc NORMAL 0
blk 3 online 4294967297 'Normal  Movable' 0
blk 4 online -1 "$(printf 'Normal\tMovable')" 0
blk 5 Online 1 Unknown 0
blk 6 'online ' 1 Foo 0
blk 7 online 1 'DMA DMA32 Normal Highmem Movable Device None Unknown Normal' 0
blk 8 ' online' +1 ' Normal' 0
blk 9 online 1 '' 0
blk 10 online '' None 0
blk 11 going-offline 1 Device 0
blk 12 - 1 Highmem 0
blk 13 - 1 Normal 0
blk 14 - - - 0
# Contents a one-line `blk` cannot write: two newlines (one is stripped), a
# NUL (the string ends there), nothing at all, and no newline.
printf 'online\n\n' > "$M/memory7/state"
printf 'online\0junk\n' > "$M/memory13/state"
: > "$M/memory12/state"
printf 'offline' > "$M/memory14/state"
printf 'Normal' > "$M/memory14/valid_zones"
printf '1' > "$M/memory14/removable"
on odd -a --output-all
on odd -a -J --output-all
on odd --output-all
on odd -o +ZONES -S ZONES

# Block sizes.
for v in '8000000' '8000000\n' '0x8000000\n' ' 8000000\n' '+8000000\n' \
         '8000000\n\n' '800 0000\n' 'zz\n' '-1\n' '0\n' '1\n' \
         '8000000000000000\n' 'ffffffffffffffff\n' '10000000000000000\n' \
         'fffffffffffffffffff\n' '\n' '' '8000000\0\n'; do
  # Named for the bytes in the file: `size_` is the empty one, `size_0a` a
  # lone newline.
  name=size_$(printf '%b' "$v" | od -An -tx1 | tr -d ' \n')
  tree "$name" "$v"
  blk 0 online 1 Normal 0
  blk 1 offline 1 Normal 0
  on "$name"
  on "$name" -b --output-all
done
# An empty size and a lone newline report errno: that of the valid_zones
# probe, or -- with memory0/valid_zones present -- the table's terminal query.
CLOC=1
on size_ -b
on size_0a -b
on size_ -b --summary=only
on size_0a -b --summary=only
tree size_empty_nozones ''
blk 0 online 1 - 0
on size_empty_nozones
on size_empty_nozones --summary=only
CLOC=

tree size_dir
mkdir -p "$M/block_size_bytes"
blk 0 online 1 Normal 0
on size_dir
tree size_missing
blk 0 online 1 Normal 0
on size_missing
on size_missing --summary=only

# Block numbers beyond 64 bits, and sizes past 2^63.
tree huge '8000000000000000\n'
blk 99999999999999999999 online 1 Normal 0
blk 3 online 1 Normal 0
on huge -a --output-all
on huge -a -b --output-all
on huge -a -b --summary

# Trees lsmem refuses. No blocks: errno is whatever it was.
tree empty '8000000\n'
CLOC=1
on empty
on empty --summary=only
on empty -J
case_ --sysroot "$T/empty/"
ENVS=(COLUMNS=80); on empty
ENVS=(COLUMNS=99999999999999999999); on empty
ENVS=(LINES=x); on empty
ENVS=()
CLOC=
tree only_others '8000000\n'
mkdir -p "$M/memory" "$M/memoryX" "$M/memory1a" "$M/xmemory1"
CLOC=1; on only_others; CLOC=
tree file_block '8000000\n'
: > "$M/memory0"
on file_block
on file_block --summary=only
tree file_block_later '8000000\n'
blk 0 online 1 Normal 0
: > "$M/memory1"
on file_block_later
tree unreadable_block '8000000\n'
blk 0 online 1 Normal 0
blk 1 online 1 Normal 0
chmod 000 "$M/memory1"
on unreadable_block
tree unreadable_first '8000000\n'
blk 0 online 1 Normal 0
chmod 000 "$M/memory0"
on unreadable_first
case_ --sysroot "$T/nowhere"
case_ --sysroot "$T/nowhere" --summary=only
: > "$T/a_file"
case_ --sysroot "$T/a_file"
mkdir -p "$T/memory_is_a_file/sys/devices/system"
: > "$T/memory_is_a_file/sys/devices/system/memory"
case_ --sysroot "$T/memory_is_a_file"
tree unlistable '8000000\n'
blk 0 online 1 Normal 0
chmod 311 "$M"
on unlistable
chmod 755 "$M"
tree unsearchable '8000000\n'
blk 0 online 1 Normal 0
chmod 644 "$M"
on unsearchable
chmod 755 "$M"
case_ --sysroot "$(printf '/%.0s' $(seq 1 4080))"
case_ --sysroot "$T/$(printf 'x%.0s' $(seq 1 4100))"
case_ --sysroot ''
sh_case "cd '$T' && lsmem --sysroot basic -o +NODE"
sh_case "cd '$T' && lsmem --sysroot empty"

# --- write errors ---------------------------------------------------------------
sh_case 'lsmem >/dev/full'
sh_case 'lsmem -a >/dev/full'
sh_case 'lsmem --summary=only >/dev/full'
sh_case 'lsmem -h >/dev/full'
sh_case 'lsmem -V >/dev/full'
sh_case 'lsmem >&-'
sh_case 'lsmem -a >&-'
sh_case 'lsmem -a -J >&-'
sh_case 'lsmem -h >&-'
sh_case 'lsmem -Z >&-'

printf '%d passed, %d differed, %d broken\n' "$pass" "$fail" "$broken"
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ]
