#!/usr/bin/env bash
# basenc-diff.sh — compare our `base64`, `base32` and `basenc` against GNU's,
# inside WSL.
#
# ## What this is checking
#
# Upstream builds the three from one file, `src/basenc.c`, and so does this
# port (`userspace/coreutils/src/basenc.rs`); one harness covers the family.
# `base64` is not a coreutils bin yet (see the module's documentation), so
# its cases run as `basenc --base64` -- the same code, with `basenc`'s decode
# block size rather than `base64`'s. When the bin lands, it joins DIFF_BINS.
#
#   * **encoding**, every padding shape (one to five trailing bytes), binary
#     input, and input longer than one 30720-byte encode block, under `-w`
#     widths that end a line exactly, part-way and never -- and the widths that
#     are refused or overflow;
#   * **decoding**, where the interesting part is what upstream prints for
#     *invalid* input: the bytes decoded before the fault, partial quanta
#     included, then `invalid input`. So: padding mid-stream (`YQ==YQ==`), a
#     quantum cut short, garbage mid-quantum, a fault past the first 4096- or
#     8192-character decode block, input exactly one block long, and `-i`;
#   * every **basenc** encoding each way, including base64url's refusal of a
#     block holding `+` or `/`, base16's lower case, an odd number of bits, and
#     Z85's length rules and its overflow check;
#   * the **command line**: a missing or doubled encoding, an extra operand, a
#     missing file, a directory, and a write error.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `--version` names SlateOS.
set -u

export MSYS2_ARG_CONV_EXCL='*'

DIFF_PROG='basenc'
DIFF_GNU_SOURCE=9.4
DIFF_BINS="base32 basenc"
DIFF_NO_REF=1
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0
TO_FULL=
INPUT=/dev/null

fx=$DIFF_TMP/fx
mkdir -p "$fx/dir" || exit 1
cd "$fx" || exit 1
printf '' > empty
printf 'a' > b1
printf 'ab' > b2
printf 'abc' > b3
printf 'abcd' > b4
printf 'abcde' > b5
printf 'hello world\n' > hello
awk 'BEGIN { for (i = 0; i < 256; i++) printf "%c", i }' > bytes
awk 'BEGIN { for (i = 0; i < 100000; i++) printf "%c", (i * 7) % 251 }' > big
head -c 30720 big > exact

compare() {
  local prog=$1; shift
  local side out err rc o_out g_out o_err g_err o_rc g_rc
  for side in ours gnu; do
    out=$DIFF_TMP/out-$side; err=$DIFF_TMP/err-$side
    if [ -n "$TO_FULL" ]; then
      ( timeout -k 2 30 env PATH="$bindir/$side" "$prog" "$@" ) <"$INPUT" >/dev/full 2>"$err"
    else
      ( timeout -k 2 30 env PATH="$bindir/$side" "$prog" "$@" ) <"$INPUT" >"$out" 2>"$err"
    fi
    rc=$?
    : >>"$out"
    if [ "$side" = ours ]; then
      o_rc=$rc; o_out=$(od -An -c <"$out"); o_err=$(cat "$err")
    else
      g_rc=$rc; g_out=$(od -An -c <"$out"); g_err=$(cat "$err")
    fi
    : >"$out"
  done
  if [ "$o_rc" = "$g_rc" ] && [ "$o_out" = "$g_out" ] && [ "$o_err" = "$g_err" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out{%s} err{%s}\n  gnu  (rc=%s): out{%s} err{%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ' | cut -c1-300)" "$(printf '%s' "$o_err" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ' | cut -c1-300)" "$(printf '%s' "$g_err" | tr '\n' '|')")
}

label_of() {
  printf '%s%s%s' "$*" "${INPUT:+ <$(basename "$INPUT")}" "${TO_FULL:+  [>/dev/full]}"
}

run_case() {
  local label; label=$(label_of "$@")
  compare "$@"
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  TO_FULL=; INPUT=/dev/null
  return 0
}

xfail_case() {
  local why=$1; shift
  local label; label=$(label_of "$@")
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$label" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$label" "$why"
  fi
  TO_FULL=; INPUT=/dev/null
  return 0
}

# Write the encoded forms once, from GNU, so each decode case below reads a
# file both sides share rather than one either side produced.
enc() { env PATH="$bindir/gnu" "$@"; }

# --- encoding --------------------------------------------------------------------------
for p in b64 base32; do
  if [ "$p" = b64 ]; then prog=basenc; pre=--base64; else prog=base32; pre=; fi
  for f in empty b1 b2 b3 b4 b5 hello bytes big exact; do
    run_case "$prog" ${pre:+"$pre"} "$f"
  done
  INPUT=$fx/hello; run_case "$prog" ${pre:+"$pre"}
  INPUT=$fx/hello; run_case "$prog" ${pre:+"$pre"} -
  for w in 0 1 3 4 8 76 77 99999999999999999999; do
    run_case "$prog" ${pre:+"$pre"} -w "$w" bytes
  done
  run_case "$prog" ${pre:+"$pre"} --wrap=5 hello
  run_case "$prog" ${pre:+"$pre"} -w -1 hello
  run_case "$prog" ${pre:+"$pre"} -w x hello
  run_case "$prog" ${pre:+"$pre"} -w 5x hello
  run_case "$prog" ${pre:+"$pre"} -w '' hello
  run_case "$prog" ${pre:+"$pre"} -w ' 5' hello
done
for e in base64 base64url base32 base32hex base16 base2msbf base2lsbf; do
  for f in empty b1 b2 b3 b5 bytes big; do
    run_case basenc --"$e" "$f"
  done
  run_case basenc --"$e" -w 10 bytes
done
run_case basenc --z85 empty
run_case basenc --z85 b4
run_case basenc --z85 bytes
run_case basenc --z85 b1
run_case basenc --z85 b5
run_case basenc --z85 exact
run_case basenc --z85 big

# --- decoding: the fixtures are GNU's own encodings -------------------------------------
enc basenc --base64 big > big.b64
enc basenc --base64 -w 0 big > big.b64w0
enc basenc --base64 bytes > bytes.b64
enc base32 big > big.b32
enc base32 bytes > bytes.b32
for e in base64url base32hex base16 base2msbf base2lsbf; do
  enc basenc --"$e" bytes > "bytes.$e"
  enc basenc --"$e" big > "big.$e"
done
enc basenc --z85 exact > exact.z85
# One fault, past the first decode block of each program.
{ head -c 5000 big.b64w0; printf '*'; tail -c +5001 big.b64w0; } > late-fault.b64
awk 'BEGIN { for (i = 0; i < 4096; i++) printf "A" }' > block.b64
awk 'BEGIN { for (i = 0; i < 8192; i++) printf "A" }' > block.b32
printf 'YWJj\r\nZGVm\r\n' > crlf.b64
printf 'YWJj ZGVm\n' > spaced.b64
printf 'YWJj\n\n\nZGVm\n' > blanklines.b64
printf 'Y\nW\nJ\nj\n' > onecharlines.b64

for f in big.b64 big.b64w0 bytes.b64 late-fault.b64 block.b64 crlf.b64 spaced.b64 blanklines.b64 onecharlines.b64; do
  run_case basenc --base64 -d "$f"
  run_case basenc --base64 -d -i "$f"
done
for f in big.b32 bytes.b32 block.b32; do
  run_case base32 -d "$f"
  run_case base32 -d -i "$f"
done
for s in 'YQ==' 'YQ==YQ==' 'YQ=x' 'YQ' 'YQ=' 'Y' '=YQ=' 'YWJj=' 'YW Jj' 'YWJ*j' '****' 'YQ==\n' 'YWJjZA==ZQ=='; do
  printf "$s" > s.b64
  run_case basenc --base64 -d s.b64
  run_case basenc --base64 -d -i s.b64
  run_case basenc --base64url -d s.b64
done
for s in 'MFRGG===' 'MFRGG' 'MFRGG===MFRGG===' 'MF======' 'MFRA====' 'MF=RGG==' 'mfrgg===' 'MFRGG==x' 'MFRGGZDF'; do
  printf "$s" > s.b32
  run_case base32 -d s.b32
  run_case base32 -d -i s.b32
  run_case basenc --base32hex -d s.b32
done
for e in base64url base32hex base16 base2msbf base2lsbf; do
  run_case basenc --"$e" -d "bytes.$e"
  run_case basenc --"$e" -d "big.$e"
done
printf 'a+b/' > plusslash
run_case basenc --base64url -d plusslash
run_case basenc --base64url -d -i plusslash
printf '48656c6c6f' > lower16; run_case basenc --base16 -d lower16
printf '48656C6C6F' > upper16; run_case basenc --base16 -d upper16
printf '48656C6C6' > odd16; run_case basenc --base16 -d odd16
printf '0100100' > bits7; run_case basenc --base2msbf -d bits7; run_case basenc --base2lsbf -d bits7
printf '01001000\n01101001' > bits16; run_case basenc --base2msbf -d bits16; run_case basenc --base2lsbf -d bits16
printf '0120' > bad2; run_case basenc --base2msbf -d bad2
run_case basenc --z85 -d exact.z85
printf 'HelloWorld' > hw.z85; run_case basenc --z85 -d hw.z85
printf 'Hello' > h5.z85; run_case basenc --z85 -d h5.z85
printf 'Hell' > h4.z85; run_case basenc --z85 -d h4.z85
printf '%%%%%%%%%%' > over.z85; run_case basenc --z85 -d over.z85
printf 'Hel~o' > tilde.z85; run_case basenc --z85 -d tilde.z85; run_case basenc --z85 -d -i tilde.z85

# --- the command line -------------------------------------------------------------------
run_case basenc hello
run_case basenc --base64 --base32 hello
run_case basenc --z85 --base16 b4
run_case basenc --base64 hello b1
run_case base32 nosuch
run_case basenc --base64 dir
run_case basenc --base64 -d dir
run_case basenc --base16 nosuch
run_case base32 -x
run_case base32 --nope
run_case basenc --base65
run_case basenc --base hello
run_case basenc --base64 --dec bytes.b64
TO_FULL=1; run_case base32 hello
TO_FULL=1; run_case basenc --base16 bytes
for p in base32 basenc; do
  xfail_case 'our --help omits the GNU ancillary block' "$p" --help
  xfail_case 'our --version names SlateOS' "$p" --version
done

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)\n' "$xpass"
  exit 1
fi
printf '\n'
[ "$fail" -eq 0 ]
