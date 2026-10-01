#!/usr/bin/env bash
# cksum-diff.sh — compare our `cksum` against GNU coreutils 9.4's, inside WSL.
#
# `cksum` is `src/digest.c` built with `HASH_ALGO_CKSUM`, so everything it
# shares with `md5sum` and the rest — the check-file formats, the escaping, the
# `--check` counters — is `scripts/digest-diff.sh`'s to test. This harness is
# for what only `cksum` has:
#
#   * **the default CRC** and its `%u %s` line, over inputs whose lengths need
#     one, two and three length bytes appended;
#   * **all eleven `-a` algorithms**, each in its own program's format — the
#     three legacy checksums as `sum`/POSIX print them, the rest tagged unless
#     `--untagged`;
#   * **`--base64` and `--raw`**, including base64's three padding widths
#     (BLAKE2b at 8, 16 and 24 bits) and `--raw`'s refusal of two files;
#   * **`-l`** — with BLAKE2b, and refused without it — and its parse errors;
#   * **the option rules** that differ from the other builds: tagged by
#     default, `--text mode is only supported with --untagged`, `--check`
#     refused for the legacy algorithms, `--base64` with `--raw`;
#   * **`--check` choosing the algorithm per line** from the tag when `-a` is
#     absent, accepting base64 digests, and upstream's kept quirks: a tag may
#     truncate any algorithm (`SHA256-128`), the byte after a tag is skipped
#     unread, and the warning names whichever algorithm the last tag chose.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `--version` names SlateOS, as
# everywhere here; and `--debug` with the CRC, where GNU's x86 build reports
# its PCLMUL implementation and ours, which has only the table CRC, says
# nothing — as GNU does when built without `USE_PCLMUL_CRC32`.
#
# Run `OURS=/usr/bin/cksum ./scripts/cksum-diff.sh` (with
# `MSYS_NO_PATHCONV=1 MSYS2_ENV_CONV_EXCL='*'` from Git Bash) to confirm the
# xfails are live: Ubuntu's own 9.4 prints GNU's `--help` and reports PCLMUL
# under `--debug`, so those cases should come back XPASS.
set -u

DIFF_PROG='cksum'
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0
TO_FULL=
INPUT=/dev/null

gnu=$bindir/gnu/cksum

fx=$DIFF_TMP/fx
mkdir -p "$fx/dir" || exit 1
cd "$fx" || exit 1
printf '' > empty
printf 'x' > one
printf 'hello\n' > hello
# 255 bytes append one length byte to the CRC, 256 two, 65536 three.
head -c 255 /dev/zero > z255
head -c 256 /dev/zero > z256
head -c 65536 /dev/zero > z65536
awk 'BEGIN { for (i = 0; i < 70000; i++) printf "%c", i % 251 }' > big
printf 'spaced\n' > 'a name'
printf 'dash\n' > ./-a
printf 'nl\n' > "$(printf 'we\nird')"

# --- check files, written by the reference -----------------------------------
# The reference writes them, so every `--check` case below is ours reading
# GNU's output: cross-verification. The round trip -- ours reading ours -- is
# the same bytes whenever the output cases above pass.
"$gnu" -a sha256 hello one > SUMS256
"$gnu" -a sha256 --untagged hello one > U256
"$gnu" -a md5 hello > M; "$gnu" -a sha1 one >> M; "$gnu" -a sha224 big >> M
"$gnu" -a sha384 empty >> M; "$gnu" -a sha512 hello >> M; "$gnu" -a sm3 one >> M
"$gnu" -a blake2b hello >> M; "$gnu" -a blake2b -l 128 big >> M
"$gnu" -a blake2b -l 8 one >> M
"$gnu" -a sha512 --base64 hello big > B64
"$gnu" -a sha1 --base64 --untagged hello > UB64
"$gnu" -a blake2b -l 16 --base64 one > B64L
"$gnu" -a blake2b --untagged hello > UB2
"$gnu" -a blake2b -l 256 --untagged one >> UB2
"$gnu" "$(printf 'we\nird')" -a md5 > ESC
sha256_hello=$("$gnu" -a sha256 --untagged hello | cut -d' ' -f1)
b2_hello=$("$gnu" -a blake2b --untagged hello | cut -d' ' -f1)
md5_hello=$("$gnu" -a md5 --untagged hello | cut -d' ' -f1)
md5_one=$("$gnu" -a md5 --untagged one | cut -d' ' -f1)
# A tag may truncate any algorithm: the first 128 bits of the SHA-256.
printf 'SHA256-128 (hello) = %s\n' "$(printf '%s' "$sha256_hello" | cut -c1-32)" > TRUNC
printf 'SHA256-128 (hello) = %s\n%s  one\n' \
  "$(printf '%s' "$sha256_hello" | cut -c1-32)" \
  "$("$gnu" -a sha256 --untagged one | cut -d' ' -f1 | cut -c1-32)" > TRUNC2
# The byte after the tag is skipped unread.
printf 'BLAKE2bX (hello) = %s\n' "$b2_hello" > B2X
printf 'BLAKE2b(hello) = %s\n' "$b2_hello" > B2OPENSSL
printf 'BLAKE2b-0x200 (hello) = %s\n' "$b2_hello" > B2HEX
# The warning names the algorithm the last tag chose; before any, the CRC.
printf 'SHA1 (hello) = zz\ngarbage\n' > WARNALGO
printf 'garbage\n' > GARBAGE
printf 'CRC (hello) = 1\n' > CRCLINE
printf 'BSD (hello) = 1\n' > BSDLINE
printf '%s  hello\n' "$md5_hello" > UNTAGGED_MD5
printf 'SHA256 (hello) = %s\n' "$(printf '%s' "$sha256_hello" | tr 'a-f' 'A-F')" > UPPER
printf 'SHA256 (gone) = %s\n' "$sha256_hello" > GONE
printf 'SHA256 (one) = %s\n' "$sha256_hello" > WRONG
# Layout latch across two files: reversed first, then standard.
printf '%s hello\n' "$md5_hello" > REV
printf '%s  one\n' "$md5_one" > STD

compare() {
  local side out err rc o_out g_out o_err g_err o_rc g_rc
  for side in ours gnu; do
    out=$DIFF_TMP/out-$side; err=$DIFF_TMP/err-$side
    if [ -n "$TO_FULL" ]; then
      ( timeout -k 2 30 env PATH="$bindir/$side" cksum "$@" ) <"$INPUT" >/dev/full 2>"$err"
    else
      ( timeout -k 2 30 env PATH="$bindir/$side" cksum "$@" ) <"$INPUT" >"$out" 2>"$err"
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
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_err" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_err" | tr '\n' '|')")
}

label_of() {
  printf 'cksum %s%s%s' "$*" "${INPUT:+ <$(basename "$INPUT")}" "${TO_FULL:+  [>/dev/full]}"
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

# --- the default CRC -----------------------------------------------------------
for f in empty one hello z255 z256 z65536 big; do
  run_case "$f"
  INPUT=$fx/$f; run_case
done
INPUT=$fx/hello; run_case -
INPUT=$fx/hello; run_case - one
run_case hello big empty
run_case 'a name'
run_case -- -a
run_case ./-a
run_case "$(printf 'we\nird')"
run_case -z hello one
run_case --raw hello
INPUT=$fx/hello; run_case --raw
run_case --untagged hello
run_case --tag hello
run_case --base64 hello
run_case -a crc hello

# --- every algorithm, in every output form ---------------------------------------
for a in sysv bsd crc md5 sha1 sha224 sha256 sha384 sha512 blake2b sm3; do
  run_case -a "$a" hello
  INPUT=$fx/hello; run_case -a "$a"
  run_case -a "$a" hello big empty
  run_case -a "$a" --untagged hello
  run_case -a "$a" --untagged -b hello
  run_case -a "$a" --base64 hello
  run_case -a "$a" --base64 --untagged big
  run_case -a "$a" --raw hello
  run_case -a "$a" -z hello one
  run_case -a "$a" "$(printf 'we\nird')"
  run_case -a "$a" -z "$(printf 'we\nird')"
  run_case --algorithm="$a" one
done

# --- -l --------------------------------------------------------------------------
for l in 8 16 24 128 256 504 512 0; do
  run_case -a blake2b -l "$l" hello
  run_case -a blake2b -l "$l" --untagged hello
  run_case -a blake2b -l "$l" --base64 hello
done
run_case -a blake2b --length=256 hello
run_case -a blake2b -l 7 hello
run_case -a blake2b -l 520 hello
run_case -a blake2b -l 1024 hello
run_case -a blake2b -l x hello
run_case -a blake2b -l '' hello
run_case -a blake2b -l -8 hello
run_case -a blake2b -l 0x10 hello
run_case -a blake2b -l ' 16' hello
run_case -a blake2b -l '16 ' hello
run_case -a blake2b -l 99999999999999999999 hello
run_case -l 256 hello                       # the CRC: -l refused
run_case -a sha256 -l 256 hello
run_case -a sha256 -l 0 hello               # 0 is "not given"
run_case -l 7 -a nope hello                 # -l fails first
run_case -a nope -l 7 hello                 # -a fails first

# --- the option rules ----------------------------------------------------------
run_case -a nope hello
run_case -a sha hello                       # no abbreviations
run_case -a SHA1 hello
run_case -a '' hello
run_case --algorithm=blake2 hello
run_case -a
run_case --base64 --raw hello
run_case --raw --base64 hello
run_case --raw hello one
run_case -a sha256 --raw hello one
run_case -t hello
run_case --untagged -t hello
run_case -t --untagged hello
run_case --tag -t hello
run_case -t --tag hello
run_case -c -a crc SUMS256
run_case -c -a bsd SUMS256
run_case -c -a sysv SUMS256
run_case --tag -c SUMS256                   # --tag also sets binary
run_case -z -c SUMS256
run_case -b -c SUMS256
run_case --ignore-missing hello
run_case --status hello
run_case --warn hello
run_case --quiet hello
run_case --strict hello
run_case --b hello                          # ambiguous: base64, binary
run_case --u hello
run_case --r hello
run_case --a=md5 hello
run_case --l=8 -a blake2b hello
run_case --s hello
run_case --=x
run_case -Z
run_case -a sha256 --debug hello
xfail_case 'GNU reports its PCLMUL CRC; ours has only the table CRC' --debug hello
xfail_case 'our --help omits the GNU ancillary block' --help
xfail_case 'our --version names SlateOS' --version

# --- --check -------------------------------------------------------------------
run_case -c SUMS256
run_case -c M
run_case -c --quiet M
run_case -c --status M
run_case -c B64
run_case -c B64L
run_case -a sha1 -c UB64
run_case -c UB64                            # untagged needs -a
run_case -a sha256 -c U256
run_case -c U256
run_case -a sha256 -c SUMS256
run_case -a md5 -c SUMS256                  # the wrong -a: misformatted
run_case -a md5 -c -w SUMS256
run_case -a blake2b -c UB2
run_case -c ESC
run_case -c TRUNC
run_case -a sha256 -c TRUNC2                # the width sticks for the next line
run_case -a sha256 -c -w TRUNC2
run_case -a blake2b -c B2X
run_case -c B2X                             # without -a the tag must be exact
run_case -c B2OPENSSL
run_case -c B2HEX
run_case -c -w WARNALGO
run_case -c -w GARBAGE
run_case -c GARBAGE
run_case -c -w CRCLINE
run_case -c -w BSDLINE
run_case -c -w UNTAGGED_MD5
run_case -a md5 -c UNTAGGED_MD5
run_case -c UPPER
run_case -c GONE
run_case -c --ignore-missing GONE
run_case -c WRONG
run_case -c --strict -w WARNALGO
run_case -a md5 -c REV STD                  # the layout latch spans files
run_case -a md5 -c STD REV
INPUT=$fx/SUMS256; run_case -c
INPUT=$fx/SUMS256; run_case -c -
run_case -c nosuch
run_case -c dir

# --- failures and a write error ---------------------------------------------------
run_case nosuch
run_case -a sha256 nosuch hello
run_case dir
TO_FULL=1; run_case hello
TO_FULL=1; run_case -a sha256 hello
TO_FULL=1; run_case -a sha256 --raw hello

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)\n' "$xpass"
  exit 1
fi
printf '\n'
[ "$fail" -eq 0 ]
