#!/usr/bin/env bash
# Differential test: our `fdisk` against Ubuntu's, util-linux 2.39.3's.
#
# fdisk is a dialog: each case feeds a side its commands on standard input
# and compares what it printed (standard output and standard error), its
# status, and the image it left. Each side works on its own copy of the
# case's starting image, in a directory of its own, under the same name, so
# every message names the same file. Nothing needs root: the devices are
# image files.
#
# What is random is masked: a new table's identifier (a DOS label's
# `0x........`, a GPT's GUIDs), wherever fdisk prints it and in the image.
# An image is compared by what Ubuntu's `sfdisk --dump` says of it (its
# `label-id:` and every `uuid=` masked) and by the bytes of its first
# sector but the DOS disk identifier's four -- so a table written wrongly
# is a difference even where it reads back alike. A GPT's headers and
# entries hold GUIDs and checksums of them, and are compared through the
# dump only.
set -u

DIFF_PROG='fdisk'
DIFF_PKG='fdisk'
DIFF_NEED='timeout sfdisk python3'
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
    || { echo "fdisk-diff: fixture $1 could not be made" >&2; exit 2; }
}

blank empty.img 64
blank tiny.img 1
table dos.img 64 'label: dos\nlabel-id: 0x12345678\n,8M,83\n,8M,82,*\n,16M,5\n,4M,83\n,,8e\n'
table gpt.img 64 'label: gpt\nlabel-id: 4A6F7B1C-1111-4222-8333-944455566677\nsize=8M, type=L, uuid=01234567-89AB-4CDE-8F01-23456789ABCD, name="first"\nsize=16M, type=S, uuid=11234567-89AB-4CDE-8F01-23456789ABCD\n'
table bsd.img 64 'label: dos\nlabel-id: 0x0badcafe\n,32M,a5\n,,83\n'
table full.img 16 'label: dos\nlabel-id: 0x01020304\n,4M\n,4M\n,4M\n,,\n'
table hybrid.img 64 'label: gpt\n,8M,L\n'
printf 'label: dos\nunit: sectors\n,10M,83\n,,82,*\n' > "$fx/load.sfdisk"
printf 'label: dos\n,10M,nosuchtype\n,,83,x\n' > "$fx/bad.sfdisk"

# mask FILE...: a new table's random identifiers.
mask() {
  sed -E -i \
    -e 's/(disk identifier |Disk identifier: |label-id: )0x[0-9a-f]{8}/\1ID/g' \
    -e 's/[0-9A-F]{8}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{12}/GUID/g' "$@"
}

# The state a run left: the image as sfdisk reads it, and its first
# sector's bytes but the DOS disk identifier's.
state() {
  local img=$DIFF_TMP/run-$1/img
  [ -f "$img" ] || return 0
  echo '-- sfdisk --dump'
  sfdisk --dump "$img" 2>&1 | sed "s|$DIFF_TMP/run-$1/||"
  echo '-- first sector'
  python3 - "$img" <<'PY'
import sys
b = open(sys.argv[1], "rb").read(512)
print((b[:440] + b"\0\0\0\0" + b[444:]).hex())
PY
  # What `O` wrote.
  if [ -f "$DIFF_TMP/run-$1/script" ]; then
    echo '-- script'
    cat "$DIFF_TMP/run-$1/script"
  fi
}

# One side's run on its own copy of the image, from its own directory.
run_side() {
  local side=$1 src=$2 input=$3; shift 3
  local d=$DIFF_TMP/run-$side
  rm -rf "$d"; mkdir -p "$d"
  [ -n "$src" ] && cp "$fx/$src" "$d/img"
  printf '%b' "$input" > "$d/stdin"
  (cd "$d" && diff_run timeout -k 2 30 env LC_ALL=C.UTF-8 TERM=dumb PATH="$bindir/$side" \
    fdisk "$@" < "$d/stdin")
}

# compare FIXTURE INPUT ARGS...: `fdisk ARGS...` with INPUT (printf's %b)
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
  local label
  label=$(printf '[%s] fdisk %s <<< %q' "$src" "$*" "$input")
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

# d FIXTURE INPUT [ARGS...]: the dialog on `img`.
d() { local src=$1 input=$2; shift 2; compare "$src" "$input" "$@" img; }
# l FIXTURE ARGS...: a listing of `img`, no dialog.
l() { local src=$1; shift; compare "$src" "" "$@" img; }

# --- options, usage and refusals -----------------------------------------------
compare "" "" -h
compare "" "" --help
compare "" "" -V
compare "" "" --version
compare "" ""
compare "" "" --bogus
compare "" "" -b 1000 x
compare "" "" -b 4096x x
compare "" "" -c=bad x
compare "" "" -t atari x
compare "" "" -w sometimes x
compare "" "" -W sometimes x
compare "" "" -L=rainbow x
compare "" "" -s
compare "" "" nonexistent.img
compare "" "" -l nonexistent.img
compare "" "" a b

# --- listings -------------------------------------------------------------------
for f in empty.img dos.img gpt.img bsd.img full.img tiny.img; do
  l "$f" -l
  l "$f" -x
done
l dos.img -l --bytes
l dos.img -l -u=cylinders
l dos.img -l -o Device,Start,Size
l dos.img -l -o +Id
l dos.img -l -o Device,Bogus
l gpt.img -l -o +UUID,Name
l gpt.img -l -t dos
l dos.img -l -t gpt
l dos.img -l -C 100 -H 16 -S 63
l dos.img -s
compare "" "" -l img img2

# --- the dialog: help, printing, units, leaving ------------------------------------
d dos.img 'm\nq\n'
d gpt.img 'm\nq\n'
d empty.img 'q\n'
d empty.img ''
d dos.img 'p\nq\n'
d dos.img 'x\nm\np\nr\nq\n'
d gpt.img 'x\nm\np\nr\nq\n'
d dos.img 'z\nq\n'
d dos.img '\n\nq\n'
d dos.img 'u\np\nu\np\nq\n'
d dos.img 'F\nv\nq\n'
d gpt.img 'F\nv\nq\n'
d dos.img 'i\n1\ni\n5\nq\n'
d gpt.img 'i\n1\nq\n'
d dos.img 'l\nq\n'
d gpt.img 'l\nq\n'
d dos.img 'x\nd\nD\nq\n'
d gpt.img 'x\nD\nq\n'

# --- new tables -----------------------------------------------------------------
d empty.img 'p\nw\n'
d empty.img 'o\np\nw\n'
d empty.img 'g\np\nw\n'
d dos.img 'g\nw\n'
d gpt.img 'o\nw\n'
d empty.img 's\n\n\n\np\nw\n'
d empty.img 's\n16\n63\n\np\nw\n'
d empty.img 'G\np\nw\n'
d empty.img 'n\np\nq\n' -n

# --- MBR partitions -------------------------------------------------------------
d empty.img 'o\nn\np\n1\n\n+10M\nn\np\n2\n\n+5M\nn\ne\n3\n\n\nn\n\n+2M\nn\n\n\np\nw\n'
d empty.img 'o\nn\n\n\n\n\np\nw\n'
d empty.img 'o\nn\np\n1\n4096\n+1G\n+1M\nw\n'
d empty.img 'o\nn\np\n1\n100\n-1M\nw\n'
d empty.img 'o\nn\np\n5\n\n\nw\n'
d full.img 'n\nq\n'
d dos.img 't\n1\n7\np\nw\n'
d dos.img 't\n2\nL\nlinux\nw\n'
d dos.img 't\n1\nnosuch\n83\nw\n'
d dos.img 'a\n1\na\n2\np\nw\n'
d dos.img 'd\n2\nd\n5\np\nw\n'
d dos.img 'd\n9\n3\nw\n'
d dos.img 'c\nc\nq\n'
d dos.img 'x\ni\n0xdeadbeef\nr\np\nw\n'
d dos.img 'x\nb\n1\n4096\nr\nw\n'
d dos.img 'x\nF\nr\nq\n'
d dos.img 'x\nf\nr\nw\n'

# --- GPT partitions -------------------------------------------------------------
d empty.img 'g\nn\n1\n\n+5M\nn\n\n\n+10M\nn\n\n\n\nt\n2\n19\np\nw\n'
d gpt.img 'n\n\n\n+1M\nt\n3\nL\nq\n'
d gpt.img 't\n1\n0FC63DAF-8483-4772-8E79-3D69D8477DE4\nw\n'
d gpt.img 't\n2\n20\nw\n'
d gpt.img 'x\nn\n1\nboot\nu\n2\n00000000-1111-4222-8333-444455556666\nA\n1\nB\n2\nR\n1\nS\n2\n2\nr\np\nw\n'
d gpt.img 'x\nl\n256\nr\nw\n'
d gpt.img 'x\ni\n12345678-1234-4234-8234-123456789ABC\nr\nw\n'
d gpt.img 'M\np\nr\nq\n'
d hybrid.img 'M\nn\np\n2\n\n\nw\n'

# --- nested BSD ---------------------------------------------------------------
d bsd.img 'b\np\nr\nq\n'
d bsd.img 'b\nm\nq\n'

# --- answers out of range, and the end of the input -----------------------------
d empty.img 'o\nn\np\n9\n1\n\n99999999999\n\nw\n'
d empty.img 'o\nn\np\n1\n'
d empty.img 'o\nn\np\n1\n\n'
d dos.img 'd\n'
d dos.img 't\n1\n'

# --- sfdisk scripts -------------------------------------------------------------
d dos.img 'O\nscript\nq\n'
d empty.img 'I\nnosuchfile\nq\n'
d empty.img 'I\n../fx/load.sfdisk\np\nw\n'
d empty.img 'I\n../fx/bad.sfdisk\np\nq\n'

printf '\nfdisk-diff: %d passed, %d differed\n' "$pass" "$fail"
[ "$fail" = 0 ]
