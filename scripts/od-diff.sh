#!/usr/bin/env bash
# Differential test: our od against GNU od.
#
# `od`'s entire output is a layout — which column a datum lands in is decided
# by an integer-division rule spread over the whole line — so stdout is
# compared byte for byte through `od -An -c`. Trimming whitespace would erase
# the one thing this implementation had to get right: the padding.
#
# ## Why both sides run inside WSL
#
# `scripts/diff-wsl.sh` gives the reasons. The reference has to be glibc's, and
# for `od` the host's — MSYS2's, a Cygwin derivative linking `msys-2.0.dll`
# rather than glibc — is wrong three ways at once: its `getopt` words every
# option diagnostic differently (`unknown option -- x` against `invalid option
# -- 'x'`), its `long double` is not the x87 80-bit format `-t fL` is about,
# and its `isprint` is its own. See `known-issues.md` →
# `TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE`.
#
# This file already reached past MSYS2 to `wsl -e env LC_ALL=… od`, at the cost
# of a WSL process per case and a probe that the Windows cwd `wsl` inherited
# landed on the same bytes under `/mnt/...`. The subject moves in with it now.
# Every operand below is a regular file or `-`, so this harness has no argument
# whose *kind* the two platforms disagree about, and the tally is unchanged by
# the move.
#
# ## Why `LC_ALL=C` for the dumps and `C.UTF-8` for the diagnostics
#
# `od` asks `isprint` about individual *bytes* — for `-t c`, for `-a`, and for
# the `z` trailer — and it reads `localeconv()->decimal_point` to size a float
# column. Both are locale answers about a byte stream that has no encoding, so
# `C` is the only setting under which the question is well posed.
#
# A run that rejects its own arguments asks neither question: it formats no byte
# and sizes no column. What it does do is echo the offending text back through
# gnulib's `quote()`, and since §351 ours prints U+2018/U+2019 in every locale —
# which is what GNU prints under a UTF-8 locale and not what it prints under
# `C`. So those rows go through `run_diag` at `C.UTF-8`. This file used to be
# `C` throughout, and the header used to record that as sidestepping B-Q2; the
# answer to B-Q2 is what turned the sidestep into a wrong reference.
set -u

# Into WSL, build ours for Linux, find glibc's, and put both behind the one
# name `od` so `argv[0]` matches. See `scripts/diff-wsl.sh`.
DIFF_PROG=od
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

fixtures=$DIFF_TMP/fixtures
mkdir -p "$fixtures"
cd "$fixtures" >/dev/null || exit 1

# One invocation of one side. `$1` is `ours` or `gnu`; each is reached through
# a symlink named `od` in a directory that is the whole of `PATH` for that one
# invocation, so `argv[0]` is the bare word on both sides. The locale stays a
# per-case argument — the section above is entirely about why two of them are
# needed — and is spliced into the same `env` that narrows `PATH`. It replaced
# two references held as *strings* and word-split at every call site, which is
# why `compare` now takes a locale where it used to take a reference.
# `diff_run` keeps bash's own announcement of a child that died of a signal
# out of the stderr the caller captures; `diff-wsl.sh` says why.
run_side() {
  local side=$1 loc=$2; shift 2
  diff_run env "LC_ALL=$loc" PATH="$bindir/$side" od "$@"
}

# --- fixtures -----------------------------------------------------------------

# Every byte value, so `-t c`, `-a` and the `z` trailer are all exercised over
# the whole range rather than over the printable subset.
: > all256.bin
for i in $(seq 0 255); do printf "\\$(printf '%03o' "$i")" >> all256.bin; done

printf 'hello world\nsecond line\n'                       > text.txt
: > empty.bin
printf 'abcdefghijklmnopq'                                > odd17.bin      # 17 bytes
printf 'abcdefg'                                          > short7.bin
printf '\x01\x02\x03'                                     > three.bin
# Four identical 16-byte blocks then a different one: the `*` elision latch
# only shows up from the *second* repeat onwards.
for _ in 1 2 3 4; do printf 'AAAAAAAAAAAAAAAA' >> dup.bin; done
printf 'BBBBBBBBBBBBBBBB' >> dup.bin
for _ in 1 2; do printf 'AAAAAAAAAAAAAAAA' >> dup.bin; done

# Floats with known bit patterns: +0, -0, 1.0, -1.0, inf, -inf, nan, denormal,
# and two ordinary values. Little-endian doubles.
printf '\x00\x00\x00\x00\x00\x00\x00\x00' >  f64.bin   # +0
printf '\x00\x00\x00\x00\x00\x00\x00\x80' >> f64.bin   # -0
printf '\x00\x00\x00\x00\x00\x00\xf0\x3f' >> f64.bin   # 1.0
printf '\x00\x00\x00\x00\x00\x00\xf0\xbf' >> f64.bin   # -1.0
printf '\x00\x00\x00\x00\x00\x00\xf0\x7f' >> f64.bin   # +inf
printf '\x00\x00\x00\x00\x00\x00\xf0\xff' >> f64.bin   # -inf
printf '\x01\x00\x00\x00\x00\x00\xf8\x7f' >> f64.bin   # nan
printf '\x01\x00\x00\x00\x00\x00\x00\x00' >> f64.bin   # smallest denormal
printf '\x18\x2d\x44\x54\xfb\x21\x09\x40' >> f64.bin   # pi
printf '\x9b\x91\x04\x8b\x0a\xbf\x05\x40' >> f64.bin   # e

# The same idea for x87 80-bit long doubles, each padded to 16 bytes as the
# ABI requires. `-t fL` reads only the first ten.
pad6() { printf '\x00\x00\x00\x00\x00\x00'; }
{ printf '\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00'; pad6; } >  f80.bin  # +0
{ printf '\x00\x00\x00\x00\x00\x00\x00\x80\xff\x3f'; pad6; } >> f80.bin  # 1.0
{ printf '\x00\x00\x00\x00\x00\x00\x00\x80\xff\xbf'; pad6; } >> f80.bin  # -1.0
{ printf '\x00\x00\x00\x00\x00\x00\x00\x80\xff\x7f'; pad6; } >> f80.bin  # +inf
{ printf '\x00\x00\x00\x00\x00\x00\x00\xc0\xff\x7f'; pad6; } >> f80.bin  # nan
{ printf '\x35\xc2\x68\x21\xa2\xda\x0f\xc9\x00\x40'; pad6; } >> f80.bin  # pi
{ printf '\x00\x00\x00\x00\x00\x00\x00\x80\x01\x00'; pad6; } >> f80.bin  # LDBL_MIN
{ printf '\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00'; pad6; } >> f80.bin  # denormal

# NUL-terminated printable runs, with a short one and a non-printable byte in
# the middle, for `-S`.
printf 'alpha\x00hi\x00beta gamma\x00ab\x01cd\x00delta\x00' > strings.bin
printf 'no terminator here'                                >> strings.bin

# --- comparison ---------------------------------------------------------------

compare() {
  local o_out g_out o_err g_err o_rc g_rc stdin=$1 loc=$2; shift 2
  o_err=$(mktemp); g_err=$(mktemp)
  # stdout through a file, not a pipe: in `x=$(od | od)` the recorded status is
  # the outer od's, and `PIPESTATUS` is set in the substitution's subshell
  # where it cannot be read. See the same note in cat-diff.sh.
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  if [ "$stdin" = "-" ]; then
    run_side ours "$loc" "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
    run_side gnu  "$loc" "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  else
    run_side ours "$loc" "$@" <"$stdin" >"$o_bin" 2>"$o_err"; o_rc=$?
    run_side gnu  "$loc" "$@" <"$stdin" >"$g_bin" 2>"$g_err"; g_rc=$?
  fi
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  rm -f "$o_bin" "$g_bin"

  # stderr is compared in full, not merely for emptiness: the whole point of
  # the getopt module is that the sentences match, so a harness that only asked
  # "did it complain?" would pass on every wording this exists to fix.
  #
  # It is compared with `cmp` on the files rather than `[ "$a" = "$b" ]` on two
  # command substitutions, because `od -A ''` embeds the *byte* it rejected in
  # its message and that byte is NUL — which a command substitution silently
  # drops (with a warning), taking the one character under test with it.
  local msg_same=no
  cmp -s "$o_err" "$g_err" && msg_same=yes

  # The text forms exist only for the failure report, where the NUL is noise.
  local o_msg g_msg
  o_msg=$(tr -d '\000' < "$o_err"); g_msg=$(tr -d '\000' < "$g_err")

  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$msg_same" = yes ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ' | cut -c1-400)" \
    "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ' | cut -c1-400)" \
    "$(printf '%s' "$g_msg" | tr '\n' '|')")
  rm -f "$o_err" "$g_err"
}

report() {
  local label="$1"; shift
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

# A case whose operands name files, or which needs no input at all.
run_case() { compare - C "$@"; report "od $*"; }

# A case that gets as far as a diagnostic and no further, referenced under
# `C.UTF-8` rather than the `C` everything else here needs. The header's reason
# for `C` is about `isprint` and `localeconv`, and a run that rejects its own
# arguments reaches neither — it never formats a byte and never sizes a float
# column. What it does reach is §351, which made our `quote()` print
# U+2018/U+2019 in every locale; GNU prints those under a UTF-8 locale and ASCII
# under `C`, so `C` is the setting in which the reference would now be wrong.
run_diag() { compare - C.UTF-8 "$@"; report "od $* [C.UTF-8]"; }

# A case fed through stdin from a fixture file.
run_in() {
  local file="$1"; shift
  compare "$file" C "$@"
  report "od $* < $file"
}

xfail_case() {
  local reason="$1"; shift
  compare - C "$@"
  if [ "$AGREED" = no ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL od %s  (%s)\n' "$*" "$reason"
  else
    xpass=$((xpass+1))
    printf 'XPASS od %s\n  now agrees with GNU, so this reason is stale: %s\n' "$*" "$reason"
  fi
  return 0
}

# `--help` up to, but not including, GNU's five-line referral tail.
#
# The tail names the GNU project, its bug address and its manual, so it must
# differ; xfailing the whole of `--help` on that account would leave the other
# 73 lines — every option's spelling, every indent, the SIZE and BYTES
# paragraphs — unchecked, which is precisely the part a hand-written help text
# gets wrong.
#
# It is the one check here that necessarily reports a difference under
# `OURS=/usr/bin/od`: the tail is stripped from the reference side only, so
# pointing `OURS` at the reference compares a text with its tail against the
# same text without one. That is a property of the check, not a failure of it.
help_body() {
  local ours gnu
  ours=$(run_side ours C --help 2>&1)
  gnu=$(run_side gnu C --help 2>&1 | sed '/^GNU coreutils online help:/,$d' | sed -e :a -e '/^$/{$d;N;ba' -e '}')
  if [ "$ours" = "$gnu" ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   od --help (body)\n'
  else
    fail=$((fail+1))
    printf 'DIFF od --help (body)\n%s\n' "$(diff <(printf '%s\n' "$gnu") <(printf '%s\n' "$ours") | head -20)"
  fi
  return 0
}

# Two of our own invocations compared against each other: the reference cannot
# arbitrate an abbreviation, but the abbreviation must still resolve to the
# same option, which is the whole point of the getopt module.
selfsame() {
  local a="$1" b="$2" x y xr yr
  # shellcheck disable=SC2086  # both are option words by construction
  x=$(run_side ours C $a three.bin 2>&1); xr=$?
  # shellcheck disable=SC2086
  y=$(run_side ours C $b three.bin 2>&1); yr=$?
  if [ "$x" = "$y" ] && [ "$xr" = "$yr" ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   od %s == od %s\n' "$a" "$b"
  else
    fail=$((fail+1))
    printf 'DIFF od %s != od %s\n  %s (rc=%s)\n  %s (rc=%s)\n' \
      "$a" "$b" "$(printf '%s' "$x" | tr '\n' '|')" "$xr" \
      "$(printf '%s' "$y" | tr '\n' '|')" "$yr"
  fi
  return 0
}

# --- the default format -------------------------------------------------------
run_case all256.bin
run_case text.txt
run_case empty.bin
run_case odd17.bin
run_case short7.bin
run_case three.bin
run_in text.txt
run_in empty.bin
run_case -
run_case text.txt text.txt
run_case three.bin odd17.bin short7.bin
run_case text.txt - < /dev/null

# --- address radices ----------------------------------------------------------
for radix in d o x n; do
  run_case -A $radix all256.bin
  run_case --address-radix=$radix odd17.bin
done
run_case -Ax -tx1 all256.bin
run_case -An -c text.txt

# --- integer formats ----------------------------------------------------------
for t in a c d1 d2 d4 d8 dC dS dI dL o1 o2 o4 o8 u1 u2 u4 u8 x1 x2 x4 x8; do
  run_case -t $t all256.bin
  run_case -t $t odd17.bin
done
run_case -t d all256.bin
run_case -t o all256.bin
run_case -t u all256.bin
run_case -t x all256.bin

# --- floating point -----------------------------------------------------------
run_case -t f4 f64.bin
run_case -t f8 f64.bin
run_case -t fF f64.bin
run_case -t fD f64.bin
run_case -t f f64.bin
run_case -t fL f80.bin
run_case -t f16 f80.bin
run_case -t fD all256.bin
run_case -t fF all256.bin
run_case -t fL all256.bin
run_case -A n -t fD f64.bin
# A partial trailing datum: the tail is zero-padded to a whole float.
run_case -t fD odd17.bin
run_case -t fF short7.bin

# --- several formats at once --------------------------------------------------
run_case -t x1 -t c all256.bin
run_case -t o1 -t x1 -t u1 -t d1 odd17.bin
run_case -t x1c all256.bin
run_case -t c -t fD f64.bin
run_case -t d8 -t c odd17.bin
run_case -bc text.txt
run_case -x -c all256.bin
run_case -t a -t c all256.bin
# Formats of different sizes together: the line length is their lcm.
run_case -t d1 -t d2 -t d4 -t d8 all256.bin
run_case -t x1 -t f16 f80.bin

# --- the z trailer ------------------------------------------------------------
run_case -t x1z all256.bin
run_case -t x2z odd17.bin
run_case -t x1z -t c all256.bin
run_case -t cz text.txt
run_case -t x4z short7.bin
run_case -t x1z empty.bin

# --- traditional single-letter formats ----------------------------------------
for f in a b c d f i l o s x X H I L O B D F e h; do
  run_case -$f all256.bin
done
run_case -abcdfilosx three.bin

# --- duplicate-line elision ---------------------------------------------------
run_case dup.bin
run_case -v dup.bin
run_case --output-duplicates dup.bin
run_case -t x1 dup.bin
run_case -w8 dup.bin
run_case -t c dup.bin

# --- widths -------------------------------------------------------------------
for w in 1 2 4 8 16 32 64 3 5 7 9 100; do
  run_case -w$w -t x1 all256.bin
done
run_case -w -t x1 all256.bin
run_case --width -t x1 all256.bin
run_case --width=8 -t x1 all256.bin
run_case -w8 -t x4 all256.bin       # 8 is a multiple of 4: accepted
run_case -w6 -t x4 all256.bin       # 6 is not: warning, falls back to lcm
run_case -w9 -t x2 all256.bin
run_case -w2 -t fD f64.bin
run_case -w16 -t x1z all256.bin

# --- skip and limit -----------------------------------------------------------
for j in 0 1 7 16 17 255 256 257; do
  run_case -j $j all256.bin
done
run_case -j 1b all256.bin
run_case -j 0x10 all256.bin
run_case --skip-bytes=32 all256.bin
for n in 0 1 7 16 17 255 256 300; do
  run_case -N $n all256.bin
done
run_case --read-bytes=48 all256.bin
run_case -j 16 -N 32 all256.bin
run_case -j 250 -N 32 all256.bin
run_case -j 300 all256.bin
run_case -j 16 -N 32 -t x1z all256.bin
run_case -N 5 three.bin all256.bin
run_case -j 2 three.bin all256.bin
run_in all256.bin -j 32 -N 16
run_in all256.bin -N 8

# --- --endian -----------------------------------------------------------------
for t in x2 x4 x8 d2 d4 d8 o2 fF fD fL; do
  run_case --endian=big -t $t all256.bin
  run_case --endian=little -t $t all256.bin
done
run_case --endian=big -t x1 all256.bin
run_case --endian=b -t x4 all256.bin
run_case --endian=l -t x4 all256.bin
run_case --endian=big -t f16 f80.bin

# --- strings ------------------------------------------------------------------
run_case -S 3 strings.bin
run_case -S 1 strings.bin
run_case -S 5 strings.bin
run_case -S 20 strings.bin
run_case --strings strings.bin
run_case --strings=4 strings.bin
run_case -S 3 all256.bin
run_case -S 3 -A x strings.bin
run_case -S 3 -A n strings.bin
run_case -S 3 -N 12 strings.bin
run_case -S 3 -j 6 strings.bin
run_case -S 3 empty.bin
run_in strings.bin -S 3

# --- the traditional operand grammar ------------------------------------------
run_case all256.bin +16
run_case all256.bin +020
run_case all256.bin +0x10
run_case all256.bin 16
run_case all256.bin +1b
run_case -c all256.bin +16
run_case --traditional all256.bin
run_case --traditional all256.bin 16
run_case --traditional all256.bin 16 32
run_case --traditional -c all256.bin 020 040
run_case --traditional all256.bin +16 +32
run_case --traditional -A x all256.bin 16 32
run_case --traditional -A n all256.bin 16 32
# A modern option disables the offset reading, so `+16` becomes a file name.
run_case -t x1 all256.bin +16
run_case -j 0 all256.bin +16

# --- diagnostics --------------------------------------------------------------
run_case -Q
run_case --bogus
run_case -A
run_case -A q all256.bin
run_case -A '' all256.bin
run_case -t
run_diag -t q all256.bin
run_diag -t d3 all256.bin
run_diag -t d16 all256.bin
run_diag -t f2 all256.bin
run_diag -t f32 all256.bin
run_diag -t 'd99999999999999999999' all256.bin
run_case -t '' all256.bin
run_diag -t q -t w all256.bin
run_case -j
run_diag -j x all256.bin
run_diag -j 1Q all256.bin
run_diag -N x all256.bin
run_case -j 18446744073709551615 -N 18446744073709551615 all256.bin
run_diag -S x all256.bin
run_case -S 3 -t x1 all256.bin
run_diag --endian=middle all256.bin
run_diag --endian all256.bin
run_diag --endian=
run_diag --traditional all256.bin 16 32 48
run_diag --traditional text.txt all256.bin
run_case nosuchfile
run_case nosuchfile text.txt
run_case text.txt nosuchfile

# --- option abbreviations resolve, and long and short agree -------------------
selfsame "--format=x1" "-tx1"
selfsame "--format=x1" "-t x1"
selfsame "--addr=x" "-Ax"
selfsame "--skip=1" "-j1"
selfsame "--read=2" "-N2"
selfsame "--out" "-v"
selfsame "--wid=8 -tx1" "-w8 -tx1"
selfsame "--str=3" "-S3"
selfsame "--trad" "--traditional"
selfsame "-t x1z" "-tx1z"

# --- the help text, minus the attribution tail --------------------------------
help_body

# --- deliberate divergences ---------------------------------------------------
# GNU 9.4 writes `if (s_err != LONGINT_OK || w_tmp <= 0) xstrtol_fatal (s_err,
# …)`, so a well-formed non-positive width reaches xstrtol_fatal with
# LONGINT_OK and it `abort()`s (SIGABRT, exit 134). We print the diagnostic
# upstream evidently meant, and that later releases produce.
xfail_case "GNU 9.4 aborts on -w0; we diagnose it" -w0 -t x1 all256.bin
xfail_case "GNU 9.4 aborts on a negative width; we diagnose it" -w-4 -t x1 all256.bin

# The last five lines of GNU's --help, and the whole of --version, name the GNU
# project, its bug address and its manual. Everything above that tail is
# compared byte for byte; only the attribution differs, and it must.
xfail_case "--help tail names the GNU project" --help
xfail_case "--version banner names GNU coreutils 9.4" --version

# --- summary ------------------------------------------------------------------
printf '\n%d passed, %d differed' "$pass" "$fail"
[ "$xfail" -gt 0 ] && printf ', %d differ on purpose' "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ' "$xpass"
printf '\n'
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
