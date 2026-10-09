#!/bin/bash
# Differential test: our slabtop against procps-ng 4.0.4's.
#
# ## The reference
#
# Ubuntu's /usr/bin/slabtop, procps 4.0.4-4ubuntu3.2; none of Debian's or
# Ubuntu's patches touch `src/slabtop.c` or `library/slabinfo.c`. It draws
# through Ubuntu's libncursesw 6.4, ours through the curses crate.
#
# ## How a run is captured
#
# `/proc/slabinfo` is pinned: each side runs in a user and mount namespace
# with a fixture bound over it (as `tload-diff.sh` pins `/proc/loadavg`), so
# both read the same caches. A run with no fixture reads the real file, which
# root alone may read -- and the namespace's root is not root to it -- so it
# is refused; a run whose file is to be missing has an empty directory bound
# over `/proc`.
#
# `-o` runs write to a file. The full-screen runs are on a pseudo-terminal
# of their own (`scripts/curses-ptyrun.py`), which types keys, resizes the
# terminal, sends signals and rewrites the fixture between frames, each once
# the program has drawn a frame and gone quiet. Each case compares what the
# terminal or the file was sent, byte for byte, with standard error and the
# exit status.
#
# ## Cases that differ on purpose
#
# `--version` names SlateOS.
#
# Run `OURS=/usr/bin/slabtop ./scripts/slabtop-diff.sh` to check that the
# harness still discriminates: the cases that differ on purpose should then
# be reported as no longer differing, and nothing else.
set -u

DIFF_PROG='slabtop'
DIFF_NEED='timeout python3 unshare'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

runner=$root/scripts/curses-ptyrun.py
pass=0; fail=0; xfail=0; xpass=0

if ! unshare -mUr sh -c "mount --bind /dev/null /proc/slabinfo" 2>/dev/null; then
  echo "slabtop-diff: cannot pin /proc/slabinfo in a namespace here; skipping"
  exit 0
fi
mkdir -p "$DIFF_TMP/emptydir"
fix=$DIFF_TMP/fixtures
mkdir -p "$fix"

HEAD='slabinfo - version: 2.1
# name            <active_objs> <num_objs> <objsize> <objperslab> <pagesperslab> : tunables <limit> <batchcount> <sharedfactor> : slabdata <active_slabs> <num_slabs> <sharedavail>'

# A cache line, as the kernel writes one.
cache() { # cache NAME ACTIVE OBJS SIZE PERSLAB PAGES ASLABS SLABS
  printf '%-17s %6s %6s %6s %4s %4s : tunables %4s %4s %4s : slabdata %6s %6s %6s\n' \
    "$1" "$2" "$3" "$4" "$5" "$6" 0 0 0 "$7" "$8" 0
}

{
  echo "$HEAD"
  cache ext4_groupinfo_4k 136 136 240 34 2 4 4
  cache fsverity_info 0 0 264 31 2 0 0
  cache ip6-frags 0 0 184 22 1 0 0
  cache PINGv6 26 26 1216 26 8 1 1
  cache RAWv6 286 286 1216 26 8 11 11
  cache UDPv6 72 72 1344 24 8 3 3
  cache TCPv6 39 39 2496 13 8 3 3
  cache kcopyd_job 0 0 3240 10 8 0 0
  cache dm_uevent 0 0 2888 11 8 0 0
  cache mqueue_inode_cache 34 34 960 34 8 1 1
  cache fuse_request 104 104 152 26 1 4 4
  cache ecryptfs_key_record_cache 0 0 576 28 4 0 0
  cache fat_inode_cache 21 21 776 21 4 1 1
  cache squashfs_inode_cache 9625 9625 704 23 4 419 419
  cache jbd2_journal_head 1938 2006 120 34 1 59 59
  cache ext4_inode_cache 52830 53480 1192 27 8 1982 1982
  cache dentry 141022 145656 192 21 1 6936 6936
  cache inode_cache 36400 37128 640 25 4 1486 1486
  cache kmalloc-8k 248 256 8192 4 8 64 64
  cache kmalloc-4k 1312 1344 4096 8 8 168 168
  cache kmalloc-2k 1712 1712 2048 16 8 107 107
  cache kmalloc-1k 3744 3744 1024 32 8 117 117
  cache kmalloc-512 6688 6688 512 32 4 209 209
  cache kmalloc-256 2688 2688 256 32 2 84 84
  cache kmalloc-192 5208 5208 192 21 1 248 248
  cache kmalloc-128 3296 3296 128 32 1 103 103
  cache kmalloc-96 5208 5208 96 42 1 124 124
  cache kmalloc-64 22592 22592 64 64 1 353 353
  cache kmalloc-32 25600 25600 32 128 1 200 200
  cache kmalloc-16 13312 13312 16 256 1 52 52
  cache kmalloc-8 12288 12288 8 512 1 24 24
  cache kmem_cache_node 1728 1728 64 64 1 27 27
  cache kmem_cache 1176 1176 256 32 2 37 37
  cache vm_area_struct 61087 61620 232 35 2 1761 1761
  cache anon_vma_chain 84264 87360 64 64 1 1365 1365
  cache radix_tree_node 26017 26180 584 28 4 935 935
} > "$fix/typical"

# Ties in every sorted column, to show the order a tie keeps.
{
  echo "$HEAD"
  cache zeta 10 20 64 8 1 2 3
  cache alpha 10 20 64 8 1 2 3
  cache mid 30 20 128 16 2 2 3
  cache Beta 10 20 64 8 1 2 3
  cache _under 5 40 32 4 4 1 1
  cache alpha 99 20 64 8 1 2 3
} > "$fix/ties"

# Numbers past what an `unsigned int` sums: every total wraps.
{
  echo "$HEAD"
  cache huge-a 4294967295 4294967295 4294967295 4294967295 4294967295 4294967295 4294967295
  cache huge-b 4294967295 4294967295 4294967295 1 2 4294967295 4294967295
  cache negative -1 -2 -3 -4 -5 -6 -7
  cache more-active 300 100 4096 4 1 1 1
} > "$fix/wrapping"

# The 2.1 statistics form, with columns after `slabdata` the library ignores.
{
  echo 'slabinfo - version: 2.1 (statistics)'
  echo '# name <active_objs> <num_objs> ... : globalstat ... : cpustat ...'
  echo 'kmalloc-64 640 640 64 64 1 : tunables 120 60 8 : slabdata 10 10 0 : globalstat 1 2 3 4 5 6 7 8 : cpustat 1 2 3 4'
  echo 'size-128 100 120 128 30 1 : tunables 120 60 8 : slabdata 4 4 0 : globalstat 1 2 3 4 5 6 7 8 : cpustat 1 2 3 4'
} > "$fix/statistics"

# Only the headings: no caches at all.
printf '%s\n' "$HEAD" > "$fix/empty"
printf 'slabinfo - version: 1.1\n' > "$fix/version1"
printf 'slabinfo - version: 2\n' > "$fix/version2"
: > "$fix/nothing"
{ echo "$HEAD"; cache ok 1 1 8 1 1 1 1; echo 'broken 1 2 3'; } > "$fix/broken"
{ echo "$HEAD"; cache ok 1 1 8 1 1 1 1; echo; } > "$fix/blank"
{
  echo "$HEAD"
  cache "n$(printf '%0130d' 0)" 1 1 8 1 1 1 1
} > "$fix/longname"
{
  echo "$HEAD"
  printf '%s' 'cache 1 2 3 4 5 : tunables 0 0 0 : slabdata 1 2 3'
  printf '%02100d\n' 0
  cache after 1 1 8 1 1 1 1
} > "$fix/longline"
{
  echo "$HEAD"
  printf 'tabbed\t5\t6\t7\t8\t1 : tunables 0 0 0 : slabdata 1 1 0\n'
  printf 'odd\001name 5 6 7 8 1 : tunables 0 0 0 : slabdata 1 1 0\n'
  printf 'caf\303\251 5 6 7 8 1 : tunables 0 0 0 : slabdata 1 1 0\n'
} > "$fix/odd"

# Mid-run, the caches change: some grow, one goes, one comes.
{
  echo "$HEAD"
  cache dentry 141500 145656 192 21 1 6936 6936
  cache inode_cache 36400 37128 640 25 4 1486 1486
  cache kmalloc-64 22592 22656 64 64 1 354 354
  cache newcomer 12 64 64 64 1 1 1
  cache vm_area_struct 61000 61620 232 35 2 1761 1761
  cache anon_vma_chain 90000 90000 64 64 1 1407 1407
  cache radix_tree_node 26017 26180 584 28 4 935 935
} > "$fix/changed"

SLAB=typical
TERMV=xterm-256color
LOC=C

# The slabinfo source of a run: `real`, `noproc`, or a fresh copy of the
# fixture -- fresh for each side, since a run may rewrite it.
current=$DIFF_TMP/current
where() {
  case $SLAB in
    real|noproc) printf '%s' "$SLAB" ;;
    *) cp "$fix/$SLAB" "$current" && printf '%s' "$current" ;;
  esac
}

# The namespace a side runs in, then COMMAND...: the fixture bound over
# /proc/slabinfo (or /proc emptied, or nothing done), then COMMAND with only
# PATH, TERM and LC_ALL in its environment, the side's binary first on PATH.
in_ns() {
  local s=$1; shift
  diff_run timeout -k 2 30 unshare -mUr sh -c '
    src=$1 empty=$2 dir=$3 term=$4 loc=$5
    shift 5
    case $src in
      noproc) mount --bind "$empty" /proc || exit 125 ;;
      real) ;;
      *) mount --bind "$src" /proc/slabinfo || exit 125 ;;
    esac
    exec env -i PATH="$dir:/usr/bin:/bin" TERM="$term" LC_ALL="$loc" "$@"' _ \
    "$(where)" "$DIFF_TMP/emptydir" "$bindir/$s" "$TERMV" "$LOC" "$@"
}

# once SIDE ARGS...: slabtop writing to a file.
once_side() {
  local s=$1; shift
  in_ns "$s" slabtop "$@" < /dev/null > "$DIFF_TMP/$s.out" 2> "$DIFF_TMP/$s.err"
  printf '%s\n' "$?" > "$DIFF_TMP/$s.rc"
}

# screen_side SIDE SIZE STEP... -- ARGS...: slabtop on a terminal of SIZE.
screen_side() {
  local s=$1 size=$2; shift 2
  local -a steps=()
  while [ "$1" != -- ]; do steps+=(--then "$1"); shift; done
  shift
  in_ns "$s" python3 "$runner" "${steps[@]}" "${size%x*}" "${size#*x}" \
    "$DIFF_TMP/$s.out" "$DIFF_TMP/$s.err" slabtop "$@" < /dev/null
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

# once LABEL ARGS... / xonce WHY LABEL ARGS...
once() {
  local label=$1; shift
  once_side ours "$@"
  once_side gnu "$@"
  judge "$label [$SLAB]" same
}
xonce() {
  local why=$1 label=$2; shift 2
  once_side ours "$@"
  once_side gnu "$@"
  judge "$label [$SLAB]" differ "$why"
}

# screen LABEL SIZE STEP... -- ARGS...
screen() {
  local label=$1; shift
  screen_side ours "$@"
  screen_side gnu "$@"
  judge "$label [$SLAB, $TERMV, $LOC, $1]" same
}

# --- the report, -o, for every fixture and every order ---------------------------
for SLAB in typical ties wrapping statistics empty odd longline changed; do
  once "-o" -o
  for key in a b c l v n o p s u x N C ''; do
    once "-o -s '$key'" -o -s "$key"
  done
done
for SLAB in version1 version2 nothing broken blank longname real noproc; do
  once "-o, a file that does not read" -o
done
SLAB=typical
for LOC in C C.UTF-8 POSIX no_SUCH.locale; do
  once "-o in $LOC" -o
  once "-o -s n in $LOC" -o -s n
done
LOC=C

# --- options --------------------------------------------------------------------------
once "--once --sort=c" --once --sort=c
once "--once --sort c" --once --sort c
once "--on --so=n (abbreviated)" --on --so=n
once "-os n (grouped)" -os n
once "-osn" -osn
for args in '-d 1 -o' '-o -d 1' '-d 0' '-d -3' '-d abc' '-d 1x' '-d 99999999999999999999' \
            '-d' '-s' '--delay' '--sort' '--delay=' '-x' '--bogus' '--once=yes' \
            'operand' 'operand -o' '-o operand' '-h' '--help' '-h -o' '-o -h' \
            '-d 1 -d 2 -o' '-- -o' '-o --'; do
  # shellcheck disable=SC2086  # each is a list of arguments
  once "$args" $args
done
once "-d ''" -d ''
xonce "--version names SlateOS" "-V" -V
xonce "--version names SlateOS" "--version" --version
xonce "--version names SlateOS" "operand -V" operand -V

# --- the screen ----------------------------------------------------------------------
for TERMV in xterm-256color xterm vt100 linux screen dumb; do
  for LOC in C C.UTF-8; do
    screen "one frame, q" 24x80 'keys:q' --
  done
done
TERMV=xterm-256color
LOC=C
for size in 30x100 11x40 10x80 5x20 50x200 24x60; do
  screen "one frame, q" "$size" 'keys:q' --
done
for key in a b c l v n o p s u x Q; do
  screen "sort by '$key', then q" 24x80 "keys:$key" 'keys:q' --
done
screen "-s n, q" 24x80 'keys:q' -- -s n
screen "-d 1, q" 24x80 'keys:q' -- -d 1
screen "keys typed together" 24x80 'keys:ncq' --
screen "interrupted" 24x80 'keys:\x03' --
screen "SIGINT" 24x80 'signal:INT' --
screen "SIGTERM: curses ends it" 24x80 'signal:TERM' --
screen "SIGHUP" 24x80 'signal:HUP' --
screen "suspended and back" 24x80 'keys:\x1a' 'keys:q' --
screen "grown" 24x80 'resize:30x100' 'keys:q' --
screen "shrunk" 30x100 'resize:20x70' 'keys:q' --
screen "made too small" 24x80 'resize:8x40' 'keys:q' --
screen "resized twice" 24x80 'resize:40x120' 'resize:12x50' 'keys:q' --
for SLAB in typical ties; do
  screen "the caches change" 24x80 "copy:$fix/changed:$current;keys:o" 'keys:q' --
done
for SLAB in wrapping empty odd; do
  screen "one frame, q" 24x80 'keys:q' --
done
for SLAB in broken real noproc; do
  screen "a file that does not read" 24x80 'keys:q' --
done
SLAB=typical
TERMV=no-such-terminal
screen "no such terminal" 24x80 'keys:q' --
TERMV=xterm-256color

echo "slabtop-diff: $pass case(s) agree, $fail differ, $xfail differ on purpose, $xpass expected to differ and did not"
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
