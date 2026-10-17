#!/usr/bin/env bash
# Differential test: our `sfdisk` against Ubuntu's, util-linux 2.39.3's.
#
# Each case feeds a side a script (or nothing) on standard input and
# compares what it printed (standard output and standard error), its
# status, and the image it left -- each side on its own copy of the case's
# starting image, in a directory of its own, under the same name, so every
# message names the same file. HOME is that directory, for the backup
# files, which are compared too. Nothing needs root: the devices are image
# files.
#
# What is random is masked: a new table's identifier (a DOS label's
# `0x........`, a GPT's GUIDs), wherever sfdisk prints it and in the image.
# An image is compared by what Ubuntu's `sfdisk --dump` says of it and by
# its first sector's bytes but the DOS disk identifier's four; a case that
# moves partition data compares the whole image's checksum as well.
set -u

DIFF_PROG='sfdisk'
DIFF_PKG='sfdisk'
DIFF_NEED='timeout python3'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0

fx=$DIFF_TMP/fx
mkdir -p "$fx"

blank() { truncate -s "$2M" "$fx/$1"; }
# table NAME MIB SCRIPT: an image laid out by Ubuntu's sfdisk, once, and
# copied to both sides.
table() {
  blank "$1" "$2"
  printf '%b' "$3" | "$gnu_real" -q --no-reread --no-tell-kernel "$fx/$1" >/dev/null 2>&1 \
    || { echo "sfdisk-diff: fixture $1 could not be made" >&2; exit 2; }
}

blank empty.img 64
table dos.img 64 'label: dos\nlabel-id: 0x12345678\n,8M,83\n,8M,82,*\n,16M,5\n,4M,83\n,,8e\n'
table gpt.img 64 'label: gpt\nlabel-id: 4A6F7B1C-1111-4222-8333-944455566677\nsize=8M, type=L, uuid=01234567-89AB-4CDE-8F01-23456789ABCD, name="first"\nsize=16M, type=S, uuid=11234567-89AB-4CDE-8F01-23456789ABCD\n'
table full.img 16 'label: dos\nlabel-id: 0x01020304\n,4M\n,4M\n,4M\n,,\n'
table gaps.img 64 'label: dos\nlabel-id: 0x0a0b0c0d\n10240,4096,83\n2048,4096,83\n'
# A partition with data in it, to move: 1 MiB of a pattern at its start.
table data.img 32 'label: dos\nlabel-id: 0x0d0a7a00\n4096,4096,83\n'
python3 - "$fx/data.img" <<'PY'
import sys
with open(sys.argv[1], "r+b") as f:
    f.seek(4096 * 512)
    f.write(bytes(range(256)) * 4096)
PY

mask() {
  sed -E -i \
    -e 's/(disk identifier |Disk identifier: |label-id: |"id": ")0x[0-9a-f]{8}/\1ID/g' \
    -e 's/[0-9A-F]{8}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{12}/GUID/g' "$@"
}

state() {
  local d=$DIFF_TMP/run-$1 img=$DIFF_TMP/run-$1/img
  if [ -f "$img" ]; then
    echo '-- sfdisk --dump'
    "$gnu_real" --dump "$img" 2>&1 | sed "s|$d/||"
    echo '-- first sector'
    python3 - "$img" <<'PY'
import sys
b = open(sys.argv[1], "rb").read(512)
print((b[:440] + b"\0\0\0\0" + b[444:]).hex())
PY
    if [ -n "${WHOLE:-}" ]; then
      echo '-- image'
      sha256sum < "$img"
    fi
  fi
  for f in "$d"/*.bak "$d"/*.move; do
    [ -f "$f" ] || continue
    echo "-- $(basename "$f")"
    od -An -tx1 "$f" | sha256sum
  done
}

run_side() {
  local side=$1 src=$2 input=$3; shift 3
  local d=$DIFF_TMP/run-$side
  rm -rf "$d"; mkdir -p "$d"
  [ -n "$src" ] && cp "$fx/$src" "$d/img"
  printf '%b' "$input" > "$d/stdin"
  (cd "$d" && diff_run timeout -k 2 30 env LC_ALL=C.UTF-8 TERM=dumb HOME="$d" PATH="$bindir/$side" \
    sfdisk "$@" < "$d/stdin")
}

# compare FIXTURE INPUT ARGS...: `sfdisk ARGS...` with INPUT (printf's %b)
# on its standard input, on a copy of FIXTURE as `img` ("" for none).
compare() {
  local src=$1 input=$2; shift 2
  local o_out=$DIFF_TMP/o.out g_out=$DIFF_TMP/g.out o_err=$DIFF_TMP/o.err g_err=$DIFF_TMP/g.err
  local o_rc g_rc
  run_side ours "$src" "$input" "$@" >"$o_out" 2>"$o_err"; o_rc=$?
  state ours >>"$o_out"
  run_side gnu "$src" "$input" "$@" >"$g_out" 2>"$g_err"; g_rc=$?
  state gnu >>"$g_out"
  mask "$o_out" "$g_out" "$o_err" "$g_err"
  # Each side's directory is its HOME, where its backup files go.
  sed -i "s|$DIFF_TMP/run-ours/|RUN/|g; s|$DIFF_TMP/run-gnu/|RUN/|g" "$o_out" "$g_out" "$o_err" "$g_err"
  local label
  label=$(printf '[%s] sfdisk %s <<< %q' "$src" "$*" "$input")
  if cmp -s "$o_out" "$g_out" && cmp -s "$o_err" "$g_err" && [ "$o_rc" = "$g_rc" ]; then
    pass=$((pass + 1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail + 1))
    printf 'DIFF %s\n  ours rc=%s, gnu rc=%s\n' "$label" "$o_rc" "$g_rc"
    diff -u --label gnu.out --label ours.out "$g_out" "$o_out" | sed -n '3,30p' | sed 's/^/  out /'
    diff -u --label gnu.err --label ours.err "$g_err" "$o_err" | sed -n '3,12p' | sed 's/^/  err /'
    if [ -n "${DIFF_FAIL_FAST:-}" ]; then exit 1; fi
  fi
  return 0
}

# s FIXTURE SCRIPT [ARGS...]: a script for `img`.
s() { local src=$1 input=$2; shift 2; compare "$src" "$input" "$@" img; }
# c FIXTURE ARGS...: a command on the image, no script.
c() { local src=$1; shift; compare "$src" "" "$@"; }

# --- options, usage and refusals -----------------------------------------------
compare "" "" -h
compare "" "" --help
compare "" "" -v
compare "" "" --version
compare "" ""
compare "" "" --bogus
compare "" "" -u X img
compare "" "" -u S -s nonexistent
compare "" "" -F -d img
compare "" "" -F -J img
compare "" "" -s -u S img
compare "" "" --move-data img
compare "" "" -N 0x img
compare "" "" -w sometimes img
compare "" "" -W sometimes img
compare "" "" --color=rainbow -l img
compare "" "" -L -l nonexistent
compare "" "" -G nonexistent
compare "" "" --change-id nonexistent 1
compare "" "" --print-id nonexistent 1
compare "" "" -c nonexistent 1
compare "" "" nonexistent

# --- types ---------------------------------------------------------------------
compare "" "" -T
compare "" "" -T -X gpt
compare "" "" -T -X sun
compare "" "" -T -X sgi
compare "" "" -T -X bsd
compare "" "" -T -X atari

# --- listings -------------------------------------------------------------------
for f in empty.img dos.img gpt.img full.img gaps.img; do
  c "$f" -l img
  c "$f" -F img
  c "$f" -d img
  c "$f" -J img
  c "$f" -g img
  c "$f" -V img
  c "$f" -l -V img
done
c dos.img -l --bytes img
c dos.img -l -o Device,Start,Size img
c dos.img -l -o +Id img
c dos.img -l -o Device,Bogus img
c dos.img -s img
c dos.img -l img img
c dos.img -l img nonexistent
c dos.img -Y dos -l img

# --- printing and changing a partition --------------------------------------------
c dos.img --part-type img 1
c dos.img --part-type img 1 82
c dos.img --part-type img 2 L
c dos.img --part-type img 1 nosuch
c dos.img --part-type img 9
c dos.img --part-type img 0
c dos.img --part-type img
c dos.img --part-type img 1 82 x
c gpt.img --part-type img 1
c gpt.img --part-type img 1 S
c gpt.img --part-uuid img 1
c gpt.img --part-uuid img 1 21234567-89AB-4CDE-8F01-23456789ABCD
c gpt.img --part-uuid img 1 not-a-uuid
c gpt.img --part-label img 1
c gpt.img --part-label img 2
c gpt.img --part-label img 1 renamed
c gpt.img --part-attrs img 1
c gpt.img --part-attrs img 1 RequiredPartition,LegacyBIOSBootable
c dos.img --part-uuid img 1
c dos.img --part-attrs img 1
c dos.img --disk-id img
c dos.img --disk-id img 0xdeadbeef
c dos.img --disk-id img nonsense
c gpt.img --disk-id img
c gpt.img --disk-id img 12345678-1234-4234-8234-123456789ABC
c dos.img --disk-id img 1 2
c dos.img --delete img 2
c dos.img --delete img
c dos.img --delete img 9
c dos.img -A img
c dos.img -A img 1
c dos.img -A img -
c dos.img -A img 9
c gpt.img -A img
c gpt.img -A img 1
c gaps.img -r img
c dos.img -r img
c gpt.img --relocate gpt-bak-mini img
c gpt.img --relocate gpt-bak-std img
c gpt.img --relocate other img
c gpt.img --relocate gpt-bak-std
c dos.img -B img
c dos.img -B -O backup img
c empty.img -B img

# --- scripts --------------------------------------------------------------------
s empty.img 'label: dos\n,10M,83\n,,82\n'
s empty.img 'label: gpt\n,10M,L\n,,S\n'
s empty.img ',10M\n,,\n'
s empty.img ',10M\n,,\n' -X gpt
s empty.img ',10M\n,,\n' -q
s empty.img ',10M\n,,\n' -n
s empty.img 'label: dos\n'
s empty.img 'label: gpt\nfirst-lba: 4096\ntable-length: 32\n,1M\n'
s empty.img 'colour: blue\n,1M\n'
s empty.img 'label: atari\n'
s empty.img 'unit: cylinders\n'
s empty.img ',1M\nbogus line here\n,1M\n'
s empty.img ',1M\nprint\nhelp\n,1M\n'
s empty.img ',1M\nabort\n'
s empty.img ',1M\nwrite\n,1M\n'
s empty.img ',1M\nquit\n'
s empty.img ''
s empty.img '\n# a comment\n\n'
s empty.img 'start=2048, size=4096, type=83, bootable\nstart=8192, size=1M, Id=82\n'
s empty.img '/dev/sdz1 : start=2048, size=4096\n/dev/sdz5 : start=10000, size=100\n'
s empty.img ',,0\n'
s empty.img 'start=0 size=0\n,1M\n'
s empty.img ',99999M\n'
s full.img ',1M\n'
s dos.img ',1M\n' -a
s dos.img ',2M\n' -N 1
s dos.img ',2M,82\n' -N 1 --no-reread
s dos.img ',2M\n' img 2
s dos.img ',2M\n' -N 9
s empty.img ',2M\n' -N 1
s gpt.img ',4M\n' -a -X gpt
s dos.img 'label: gpt\n,1M\n' -w always
s dos.img 'label: gpt\n,1M\n' -w never
s dos.img 'label: dos\n,1M\n' -b
s dos.img 'label: dos\n,1M\n' -O backup
s dos.img 'label: dos\n,1M\n' --no-tell-kernel
WHOLE=1 s data.img '8192,\n' -N 1 --move-data
WHOLE=1 s data.img '6144,\n' -N 1 --move-data=moved
WHOLE=1 s data.img '8192,\n' -N 1 --move-data -n

printf '\nsfdisk-diff: %d passed, %d differed\n' "$pass" "$fail"
[ "$fail" = 0 ]
