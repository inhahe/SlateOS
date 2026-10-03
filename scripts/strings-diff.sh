#!/usr/bin/env bash
# Differential test: our `strings` against GNU binutils `strings`.
#
# ## What makes this one worth measuring
#
# `strings` looks trivial -- print runs of printable bytes -- and almost every
# part of that sentence is a decision someone has to get right. What counts as
# printable, whether whitespace does, how long a run has to be, what offset to
# print it at and in which radix, and which parts of an object file are even
# looked at. An implementation can produce plausible output for a text file and
# be wrong about all five.
#
# The one that catches people: **by default `strings` does not scan the whole
# file.** For a recognised object file it scans only the loaded, initialised
# sections; `-a`/`--all` is what makes it read every byte. A fixture that is
# not an object file hides that entirely, so there is a real ELF among the
# fixtures below.
#
# ## The reference
#
# `/usr/bin/strings`, GNU Binutils 2.42. Not coreutils, so `DIFF_GNU_SOURCE`
# cannot supply it and §726's specific caveat does not apply; the general one
# does -- a green run certifies agreement with Ubuntu's binutils.
#
# ## Why `od -An -c`
#
# `-t` prints an offset and then whitespace before the string, and the amount
# of whitespace is part of the format. `-s`/`--output-separator` changes the
# terminator from a newline to an arbitrary string. Both are invisible to a
# comparison that reads lines.
#
# ## The fixtures are bytes, not text
#
# Every interesting case here is about a byte that is *not* text: a run that is
# exactly at the length threshold, a NUL in the middle, a high byte, a tab
# between two words, a UTF-16 string. They are written with `printf` escapes
# from this file rather than checked in, so what the harness compares is what
# this file says and not what some editor did to a fixture on the way in.
set -u

DIFF_PROG='strings'
# Every invocation bounded, both sides. `strings` on a large file is a scan, and
# the encodings that read two or four bytes at a time are the ones where an
# off-by-one in the step can fail to terminate.
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

fixtures=$DIFF_TMP/fixtures
mkdir -p "$fixtures"
cd "$fixtures" >/dev/null || exit 1

# --- fixtures ------------------------------------------------------------------
# Plain text, the base case.
printf 'hello world\nsecond line\n'                     > plain.txt

# Runs either side of the default threshold of 4: `abc` must not appear and
# `abcd` must.
printf 'abc\0abcd\0abcde\0'                             > lengths.bin

# A string with a tab and a space inside it. Whitespace is NOT printable to
# `strings` by default, so this is three runs, not one -- unless `-w`.
printf 'alpha\tbravo charlie\0'                          > spaced.bin

# High bytes, which are printable under `-e S` (8-bit) and not under `-e s`.
printf 'caf\351 latte\0plain\0'                          > high.bin

# UTF-16LE and UTF-16BE, for `-e l` and `-e b`.
printf 'w\0i\0d\0e\0s\0t\0r\0i\0n\0g\0\0\0'              > utf16le.bin
printf '\0w\0i\0d\0e\0s\0t\0r\0i\0n\0g\0\0'              > utf16be.bin

# A run at the very start and the very end, with no terminator -- the ends are
# where an off-by-one lives.
printf 'startrun\0middle\0endrun'                        > ends.bin

# Empty, and one byte.
printf ''                                                > empty.bin
printf 'x'                                               > onebyte.bin

# UTF-8 of every length -- 2, 3 and 4 bytes -- for `-U`, one string apiece.
printf 'abc\303\251def\0xyz\342\202\254uvw\0pq\360\237\230\200rs\0ab\364\217\277\277cd\n' > unicode.bin

# UTF-8 that is not: a lead with no continuation, a stray continuation, a
# three-byte sequence cut short, and a four-byte one cut short at a run's start.
printf 'abcd\303Xefgh\0abcd\251efgh\0wxyz\342\202Qrst\0\360\237\230abcd\0' > broken.bin

# Argument files for `@FILE`, which libiberty expands before anything is parsed.
printf -- '-n 6 -t x'        > opts1
printf -- "-f '-s' :"        > opts2
printf -- '@opts1 -w'        > nested
printf -- '  \n\t '          > blank
printf -- '-n'               > partial
printf -- '"plain.txt"'      > operand
printf -- '@self'            > self
mkdir -p atdir

# A real object file, so the default "loaded sections only" rule is exercised.
# `/bin/true` is read-only, tiny, and present on every host this runs on.
cp /bin/true elf.bin

run_side() {
  local side=$1; shift
  diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 PATH="$bindir/$side" strings "$@"
}

compare() {
  local stdin=$1; shift
  local o_out g_out o_err g_err o_rc g_rc
  o_err=$(mktemp); g_err=$(mktemp)
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  if [ "$stdin" = "-" ]; then
    run_side ours "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
    run_side gnu  "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  elif [ "$stdin" = "<&-" ]; then
    # Standard input closed.
    run_side ours "$@" <&- >"$o_bin" 2>"$o_err"; o_rc=$?
    run_side gnu  "$@" <&- >"$g_bin" 2>"$g_err"; g_rc=$?
  elif [ "${stdin#<}" != "$stdin" ]; then
    # Standard input redirected from a path -- a directory, whose reads fail.
    run_side ours "$@" <"${stdin#<}" >"$o_bin" 2>"$o_err"; o_rc=$?
    run_side gnu  "$@" <"${stdin#<}" >"$g_bin" 2>"$g_err"; g_rc=$?
  else
    printf '%b' "$stdin" | run_side ours "$@" >"$o_bin" 2>"$o_err"; o_rc=$?
    printf '%b' "$stdin" | run_side gnu  "$@" >"$g_bin" 2>"$g_err"; g_rc=$?
  fi
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  rm -f "$o_bin" "$g_bin" "$o_err" "$g_err"
  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
}

report() {
  local label="$1"
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

run_case()  { compare - "$@"; report "strings $*"; }
run_stdin() { local i="$1"; shift; compare "$i" "$@"; report "printf '$i' | strings $*"; }

xfail_case() {
  local why=$1; shift
  compare - "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS strings %s -- expected to differ (%s) and did not\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail strings %s (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- the default ------------------------------------------------------------------
run_case plain.txt
run_case lengths.bin
run_case spaced.bin
run_case high.bin
run_case ends.bin
run_case empty.bin
run_case onebyte.bin

# --- the length threshold ------------------------------------------------------------
run_case -n 1 lengths.bin
run_case -n 3 lengths.bin
run_case -n 4 lengths.bin
run_case -n 5 lengths.bin
run_case -n 6 lengths.bin
run_case -n4 lengths.bin
run_case -4 lengths.bin
run_case -1 lengths.bin
# The digit shorthand is the whole word that holds it, read after every other
# option: `-12` is twelve, `-5 -n 3` is five either way round, and `-a5` is
# refused as an integer.
run_case -10 plain.txt
run_case -12 plain.txt
run_case -5 -n 3 lengths.bin
run_case -n 3 -5 lengths.bin
run_case -1 -5 lengths.bin
run_case -010 plain.txt
run_case -a5 plain.txt
run_case --bytes=3 lengths.bin
run_case -n 0 lengths.bin
run_case -n -1 lengths.bin
run_case -n notanumber lengths.bin
xfail_case "our usage text, not the GNU project's -- it names the options this build refuses where GNU lists its supported targets" -n

# --- offsets and radixes -----------------------------------------------------------------
run_case -t d plain.txt
run_case -t o plain.txt
run_case -t x plain.txt
run_case -td plain.txt
run_case --radix=x plain.txt
xfail_case "our usage text, not the GNU project's -- it names the options this build refuses where GNU lists its supported targets" -t q plain.txt
xfail_case "our usage text, not the GNU project's -- it names the options this build refuses where GNU lists its supported targets" -t
run_case -o plain.txt
run_case -t d ends.bin
run_case -t x high.bin

# --- whitespace ----------------------------------------------------------------------------
run_case -w spaced.bin
run_case --include-all-whitespace spaced.bin
run_case -w plain.txt
run_case -w -t d spaced.bin

# --- encodings ------------------------------------------------------------------------------
run_case -e s high.bin
run_case -e S high.bin
run_case -e l utf16le.bin
run_case -e b utf16be.bin
run_case -e B utf16be.bin
run_case -e L utf16le.bin
run_case --encoding=S high.bin
xfail_case "our usage text, not the GNU project's -- it names the options this build refuses where GNU lists its supported targets" -e q high.bin
xfail_case "our usage text, not the GNU project's -- it names the options this build refuses where GNU lists its supported targets" -e
run_case -e l plain.txt

# --- the whole file, or only its loaded sections ---------------------------------------------------
run_case elf.bin
run_case -a elf.bin
run_case --all elf.bin
run_case -d elf.bin
run_case --data elf.bin
# `-d` with an offset, which is the half of it that is easy to get wrong.
# Each section is scanned as its own stream, so a naive implementation reports
# the offset WITHIN the section and every number is wrong by the section's
# start. These cases are the only thing that would catch that: `-d elf.bin`
# alone prints the same strings either way.
run_case -d -t x elf.bin
run_case -d -t d elf.bin
run_case -d -n 8 -t x elf.bin
run_case -a -n 8 elf.bin

# --- naming the file in the output --------------------------------------------------------------------
run_case -f plain.txt
run_case --print-file-name plain.txt
run_case -f plain.txt lengths.bin
run_case -f -t d plain.txt

# --- several files, and stdin ----------------------------------------------------------------------------
run_case plain.txt lengths.bin
run_case plain.txt nosuch.bin lengths.bin
run_stdin 'hello world\0second\0'
run_stdin 'abc\0abcd\0' -n 4
run_stdin ''
# Standard input that cannot be read: binutils reads it with `getc` and never
# asks `ferror`, so a failed read is simply the end of the input -- nothing is
# printed and the status is 0, for a closed descriptor and a directory alike.
# Ours reported the directory as `Is a directory` until 2026-10-03.
compare '<&-'; report "strings <&-"
compare '<.'; report "strings < dir"
compare '<&-' -a; report "strings -a <&-"
compare '<.' -n 2; report "strings -n 2 < dir"
xfail_case "our usage text, not the GNU project's -- it names the options this build refuses where GNU lists its supported targets" -

# --- the output separator ---------------------------------------------------------------------------------
run_case -s : plain.txt
run_case --output-separator=: plain.txt
run_case -s '' plain.txt
xfail_case "our usage text, not the GNU project's -- it names the options this build refuses where GNU lists its supported targets" -s

# NINE OF THESE ARE xfail AND THE REASON IS ONE LINE OF OUR OWN HELP TEXT.
#
# Every case here prints the usage, and this build's usage carries a line GNU's
# does not: `This build reads ELF64 objects only, for --data, so --target is
# refused`, where GNU prints `supported targets: elf64-x86-64 ...`. We cannot print that
# list without claiming support we do not have, so the two can never match and
# `--help`/`--version` are already xfail for exactly this.
#
# THE PART THAT CAN MATCH IS STILL ASSERTED, elsewhere: the diagnostic sentence
# for each of these is unit-tested in `strings.rs` (13 assertions on
# `e.sentence`). What is given up here is only the comparison of our usage block
# against theirs, which was never going to hold.
#
# Verified before reclassifying rather than assumed: the message lines matched
# byte for byte once the spurious `Try '... --help'` referral was removed.
# --- refusals -----------------------------------------------------------------------------------------------
run_case nosuch.bin
run_case .
xfail_case "our usage text, not the GNU project's -- it names the options this build refuses where GNU lists its supported targets" -Q plain.txt
xfail_case "our usage text, not the GNU project's -- it names the options this build refuses where GNU lists its supported targets" --nosuchoption plain.txt
xfail_case "GNU accepts an unknown --target on a non-object file and prints its strings; this build refuses --target outright" -T nosucharch plain.txt

# --- -U/--unicode -------------------------------------------------------------------------------------------------
# Every mode, both spellings. Any mode but `d` forces `-e S`; `l` prints only a
# character's first byte (upstream's `%.1s`); a four-byte character's escape is
# upstream's arithmetic, not its code point; `h` is `e` off a terminal.
run_case -U d unicode.bin
run_case -U l unicode.bin
run_case -U e unicode.bin
run_case -U x unicode.bin
run_case -U h unicode.bin
run_case -U i unicode.bin
run_case --unicode=default unicode.bin
run_case --unicode=locale unicode.bin
run_case --unicode=escape unicode.bin
run_case --unicode=hex unicode.bin
run_case --unicode=highlight unicode.bin
run_case --unicode=invalid unicode.bin
run_case -U e -n 2 unicode.bin
run_case -U e -n 7 unicode.bin
run_case -U e -t x unicode.bin
run_case -U x -t d -f unicode.bin
run_case -U e -e l unicode.bin
run_case -U e -w spaced.bin
run_case -U e high.bin
run_case -U e broken.bin
run_case -U x -t x broken.bin
run_case -U i broken.bin
run_case -U l broken.bin
run_case -U e plain.txt unicode.bin
run_stdin 'abc\xc3\xa9def\0' -U e
run_stdin 'abc\xc3\xa9def\0' -U x -t d
# `-d` reads an object's sections from memory: upstream's buffer path.
run_case -U e -d elf.bin
run_case -U x -d -t x elf.bin
run_case -U i -d elf.bin
run_case -U e -d unicode.bin
# The help advertises `show`; the parser refuses it, and `s`, without the usage.
run_case -U s unicode.bin
run_case -U show unicode.bin
run_case -U '' unicode.bin
run_case --unicode= unicode.bin

# --- @FILE ---------------------------------------------------------------------------------------------------------
# Each readable `@FILE` becomes the words in it, nested files too, before any
# parsing -- after `--` as well. One that cannot be read stays an operand, a
# directory or a file that names itself is refused before anything else.
run_case @opts1 plain.txt
run_case @opts2 plain.txt
run_case @nested spaced.bin
run_case @blank plain.txt
run_case @operand
run_case -- @opts1 plain.txt
run_case @partial plain.txt
run_case @nosuchfile plain.txt
run_case @atdir plain.txt
run_case @self plain.txt

# --- the two whose text is ours ----------------------------------------------------------------------------------
xfail_case "our help text, not the GNU project's" --help
xfail_case "our version string, not the GNU project's" --version

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ -n "${DIFF_SKIPPED:-}" ] && printf 'skipped:%s\n' "$DIFF_SKIPPED"
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
