#!/usr/bin/env bash
# Differential test: our `time` against GNU Time 1.9's `/usr/bin/time`.
#
# ## Which `time` this is
#
# Not the shell's. `time` is a *keyword* in bash, ksh and zsh, so `command -v
# time` answers with the keyword and `time foo` in a script never reaches a
# binary at all. The two are not near-relatives either — they share no option,
# no format string and no output shape:
#
#     $ time true            # bash's keyword
#
#     real    0m0.000s
#     user    0m0.000s
#     sys     0m0.000s
#
#     $ /usr/bin/time true   # GNU Time 1.9
#     0.00user 0.00system 0:00.00elapsed 0%CPU (0avgtext+0avgdata 1152maxresident)k
#     0inputs+0outputs (0major+72minor)pagefaults 0swaps
#
# Our shipped `time` imitated the *keyword* — a blank line and one `real\t0m0.000s`
# — which is the one shape that cannot be reached through a `PATH` lookup, since
# any shell that could look it up would have used its own keyword first. This
# harness therefore compares against `/usr/bin/time`, and `DIFF_REF` names it by
# path for the same reason `echo-diff.sh` does: `command -v` would find the
# builtin.
#
# ## Why most cases are compared with the digits masked
#
# The whole output of this program is measurements, and no two runs of it agree:
# elapsed time, CPU percentage, maximum resident set size and page-fault counts
# all move. A byte-for-byte comparison of `0.00user 0.00system 0:00.00elapsed
# 0%CPU (0avgtext+0avgdata 1152maxresident)k` would fail against *itself* on the
# next run.
#
# So `NORM=1` replaces every run of digits with `N` on both sides before
# comparing, which leaves exactly what this port can get wrong — the format
# engine's structure, the punctuation, the field order, the units, and which
# `%` sequence expands to which field — and drops only the value the kernel
# happened to report. `OURS=/usr/bin/time` still discriminates: run it and every
# xfail should turn into an XPASS and nothing else should move.
#
# The cases where a *number* is the answer are run without it, and they are the
# ones a masked comparison would hide:
#
#   * `-f x=%x` — the child's exit status, and nothing else on the line.
#   * `-f Z=%Z` — the page size.
#   * `-f ""` with a failing child — `Command exited with non-zero status 1`,
#     alone, because an empty format contributes no digits of its own.
#   * every diagnostic, every option error, and every exit status.
#
# ## What is deliberately reproduced and looks like a bug
#
# Three things, all measured against the real binary:
#
#   * A format string ending in a bare `%` prints `?` and returns **without the
#     closing newline** — `summarize`'s `case '\0'` is a `return`, not a
#     `break`.
#   * `-p` suppresses `Command exited with non-zero status N`, and `-f 'real
#     %e\nuser %U\nsys %S'` does not, even though the two produce identical
#     output. Upstream compares the format *pointer* against `posix_format`.
#   * `-h` is not an option. The help text says `-h,  --help`, the switch has a
#     `case 'h'`, and the getopt string is `"+af:o:pqvV"` — so only the long
#     form reaches it and `-h` is `invalid option -- 'h'`.
#
# A fourth is *not* reproduced, and is an out-of-bounds read rather than a
# quirk: a format string ending in a bare backslash makes upstream print `?\`,
# the format's own NUL byte, and then whatever bytes follow it in memory —
# measured, `time -f 'ab\' true` prints `ab?\<NUL>true`, having walked into the
# adjacent `argv` string. Ours stops at the end of the format. See the case
# below, which is an xfail for that reason.
#
# Run `OURS=/usr/bin/time ./scripts/time-diff.sh` to confirm the harness still
# discriminates.
#
# SC2016 is disabled for the file rather than case by case: the twenty-odd
# snippets below are single-quoted *on purpose*, so that `$F`, `$$` and `$1` are
# expanded by the `sh` the snippet runs under and not by this one. Quoting them
# the way SC2016 suggests would substitute this harness's values and destroy the
# case. SC2209 is the same misreading one level up -- `time` here is a string,
# not a command being run.
# shellcheck disable=SC2016,SC2209
set -u

DIFF_PROG='time'
# Our binary is `time_cmd`, because `src/bin/time.rs` would collide with Rust's
# own `std::time` in the doc tooling and reads badly beside it; the symlink
# `diff-wsl.sh` builds is named `time`, so `argv[0]` is the bare word on both
# sides and the `time: ` prefix on every diagnostic matches.
DIFF_BINS=time_cmd
# `command -v time` under bash answers `time` — the keyword. See the header.
DIFF_REF="/usr/bin/time /bin/time"
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

work=$DIFF_TMP/work
mkdir -p "$work"

case_no=0

# --- knobs, reset after every case -------------------------------------------

# Mask digit runs before comparing. See the header.
NORM=
# Also print the contents of the file named by `$F` in the snippet, so that a
# `-o FILE` case is compared on what landed in the file rather than only on what
# did not land on stderr.
SHOWF=
reset_knobs() { NORM=; SHOWF=; }

# --- running one side --------------------------------------------------------

# Direct exec: stdin from /dev/null, stdout and stderr to separate files. The
# arguments arrive as this function's arguments, so a byte that is not valid
# UTF-8 and a word containing a space both reach the program untouched.
#
# `$bindir/$side` is *prepended* to `PATH` rather than replacing it, because the
# commands being timed (`sh`, `true`, `printf`) have to be findable. Both sides
# then see the identical `PATH`, which matters more here than its contents: a
# name that is not found resolves to `ENOENT` (127) or `EACCES` (126) depending
# on what is *on* the path, and the two sides must be asking the same question.
run_direct() {
  local side=$1 out=$2 err=$3 rcf=$4; shift 4
  ( PATH="$bindir/$side:$PATH" timeout -k 2 30 \
      time "$@" </dev/null >"$out" 2>"$err" )
  echo $? >"$rcf"
  return 0
}

# A shell snippet, for the cases whose subject is a descriptor or an output
# file. `$F` is a scratch path unique to this side and case, so the two sides
# never write to the same file; the snippet's own status goes to descriptor 9,
# which it cannot close.
run_snippet() {
  local side=$1 out=$2 err=$3 rcf=$4 snippet=$5 f=$6
  rm -f "$f"
  ( PATH="$bindir/$side:$PATH" F="$f" timeout -k 2 30 \
      sh -c "$snippet"'; echo $? >&9' </dev/null ) \
    >"$out" 2>"$err" 9>"$rcf"
  if [ -n "$SHOWF" ]; then
    { echo "--- F ---"; cat "$f" 2>&1; } >>"$out"
  fi
  return 0
}

# --- comparing the two sides -------------------------------------------------

# Every run of digits becomes `N`, when `NORM` is set. Applied to both sides
# identically, so it can hide a difference but can never invent an agreement
# between two shapes that differ in anything but a number.
#
# A NUL is spelled out rather than passed through, because `judge` holds each
# side in a shell variable and command substitution *discards* NUL bytes with a
# warning -- which would silently equate a side that emits one with a side that
# does not. Upstream emits one for `-f 'ab\'`, so this is not hypothetical.
canon() {
  if [ -n "$NORM" ]; then
    sed -E 's/\x00/[NUL]/g; s/[0-9]+/N/g' "$1"
  else
    sed -E 's/\x00/[NUL]/g' "$1"
  fi
}

judge() {
  local o_out=$1 g_out=$2 o_err=$3 g_err=$4 o_rc=$5 g_rc=$6 label=$7
  local o_o g_o o_e g_e o_r g_r
  o_o=$(canon "$o_out"); g_o=$(canon "$g_out")
  o_e=$(canon "$o_err"); g_e=$(canon "$g_err")
  o_r=$(cat "$o_rc");    g_r=$(cat "$g_rc")

  if [ "$o_o" = "$g_o" ] && [ "$o_e" = "$g_e" ] && [ "$o_r" = "$g_r" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours: rc=%s out{%s} err{%s}\n  gnu : rc=%s out{%s} err{%s}' \
    "$o_r" "$(printf '%s' "$o_o" | tr '\n' '|')" "$(printf '%s' "$o_e" | tr '\n' '|')" \
    "$g_r" "$(printf '%s' "$g_o" | tr '\n' '|')" "$(printf '%s' "$g_e" | tr '\n' '|')")
  LABEL=$label
}

compare_direct() {
  case_no=$((case_no+1))
  local o_out=$work/oo$case_no g_out=$work/go$case_no
  local o_err=$work/oe$case_no g_err=$work/ge$case_no
  local o_rc=$work/or$case_no  g_rc=$work/gr$case_no
  run_direct ours "$o_out" "$o_err" "$o_rc" "$@"
  run_direct gnu  "$g_out" "$g_err" "$g_rc" "$@"
  judge "$o_out" "$g_out" "$o_err" "$g_err" "$o_rc" "$g_rc" "time $*"
  reset_knobs
}

compare_snippet() {
  case_no=$((case_no+1))
  local o_out=$work/oo$case_no g_out=$work/go$case_no
  local o_err=$work/oe$case_no g_err=$work/ge$case_no
  local o_rc=$work/or$case_no  g_rc=$work/gr$case_no
  run_snippet ours "$o_out" "$o_err" "$o_rc" "$1" "$work/of$case_no"
  run_snippet gnu  "$g_out" "$g_err" "$g_rc" "$1" "$work/gf$case_no"
  # The scratch path is in the output when `SHOWF` is set, and differs between
  # the sides by construction. Normalise it to the same word before comparing.
  sed -i "s#$work/of$case_no#F#g" "$o_out"
  sed -i "s#$work/gf$case_no#F#g" "$g_out"
  judge "$o_out" "$g_out" "$o_err" "$g_err" "$o_rc" "$g_rc" "[sh] $1"
  reset_knobs
}

report() {
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$LABEL"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$LABEL" "$REPORT"
  fi
  return 0
}

run_case() { compare_direct "$@"; report; }
sh_case()  { compare_snippet "$1"; report; }

# `run_case` with the digits masked, which is what nearly every case that
# actually runs a command needs. Spelled as its own verb rather than as a knob
# set on the line above, because a knob that is forgotten yields a case that
# fails for a reason unrelated to the program.
norm_case() { NORM=1; compare_direct "$@"; report; }
norm_sh()   { NORM=1; compare_snippet "$1"; report; }

# A case expected to differ, with the reason. Counted apart so that one which
# starts agreeing is reported too: a stale xfail is a claim nobody rechecked.
xfail_case() {
  local why="$1"; shift
  compare_direct "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$LABEL" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$LABEL" "$why"
  fi
  return 0
}

echo "time-diff:"
echo "  ours: $OURS"
echo "  gnu:  $gnu_real"

# =============================================================================
# 1. The default format
# =============================================================================
# Two lines, and every field in them is a measurement, so these are the cases
# `NORM` exists for. What is being compared is the shape: the order of the
# fields, the words glued to them, the parentheses and the trailing `k`.

norm_case true
norm_case /bin/true
norm_case sh -c 'exit 0'
norm_case printf 'x'
norm_case echo hello world

# =============================================================================
# 2. The child's status is the program's status
# =============================================================================
# `time` must pass the command's status through untouched, and add the
# `Command exited with non-zero status N` line above the summary while doing
# it. 128+signal for a death by signal, which is what a shell reports.

norm_case false
norm_case sh -c 'exit 42'
norm_case sh -c 'exit 255'
norm_case sh -c 'exit 1'
norm_case sh -c 'kill -TERM $$'
norm_case sh -c 'kill -INT $$'
norm_case sh -c 'kill -KILL $$'
norm_case sh -c 'kill -ABRT $$'

# The message on its own, with an empty format so that nothing else on the line
# carries a digit and the status can be compared unmasked.
run_case -f '' false
run_case -f '' sh -c 'exit 42'
run_case -f '' sh -c 'exit 255'
run_case -f '' true
run_case -f '' sh -c 'kill -TERM $$'
run_case -f '' sh -c 'kill -KILL $$'

# `-q` drops the abnormal-termination line and nothing else.
run_case -q -f '' false
run_case -q -f '' sh -c 'kill -TERM $$'
run_case --quiet -f '' false
norm_case -q false

# =============================================================================
# 3. A command that cannot be run
# =============================================================================
# The child reports and `_exit`s, so the *parent* still prints a full summary
# and the status is the child's 126 or 127. A port that returned early on a
# failed spawn would print nothing here and be wrong twice over.
#
# Which of 126 and 127 comes out is glibc's `execvp` rule — the most specific
# errno of the whole `PATH` walk, so a single `EACCES` anywhere beats the
# trailing `ENOENT`. Both sides walk the identical `PATH`, so they agree
# whatever that path happens to contain.

norm_case nosuchcommandanywhere
norm_case /nope/nope
norm_case /etc
norm_case /etc/hostname
norm_case ''
norm_case -

# The same, with an empty format, so the status line is compared exactly.
run_case -f '' /nope/nope
run_case -f '' /etc
run_case -f '' ''

# =============================================================================
# 4. Missing operand, and options getopt itself rejects
# =============================================================================
# All 125: `EXIT_CANCELED`, everything this program gets wrong before it has a
# child to blame.

run_case
run_case -x true
run_case -xy true
run_case --nope true
run_case -h true
run_case -f
run_case -o
run_case --format
run_case --output-file
run_case --help=x
run_case --version=x
run_case --=x
run_case -a
run_case -p
run_case -v

# =============================================================================
# 5. `+` in the short-option string
# =============================================================================
# Option parsing stops at the first operand, so everything after the command
# name belongs to the command. The option most likely to follow one is
# `--help`, which is exactly the one that must not be intercepted.

norm_case echo -p
norm_case printf '[%s]' -f
norm_case sh -c 'echo "$1"' sh --version
norm_case sh -c 'echo "$1"' sh -v
run_case -f '' sh -c 'echo "$1" >&2' sh --help

# =============================================================================
# 6. `--` and what follows it
# =============================================================================

run_case --
norm_case -- true
norm_case -- echo -p
run_case -f '' -- false
norm_case -- -

# =============================================================================
# 7. The format engine: `%` sequences whose value is fixed
# =============================================================================
# Compared unmasked, because here the digits are the answer.

run_case -f 'x=%x' true
run_case -f 'x=%x' false
run_case -f 'x=%x' sh -c 'exit 42'
run_case -f 'x=%x' sh -c 'exit 255'
run_case -f 'x=%x' sh -c 'kill -KILL $$'      # WEXITSTATUS of a signal: 0
run_case -f 'Z=%Z' true                       # page size
run_case -f 'C=[%C]' true
run_case -f 'C=[%C]' echo hi there
run_case -f 'C=[%C]' sh -c 'exit 0'
run_case -f 'C=[%C]' printf '%s' 'a b' c
run_case -f 'lit%%eral' true
run_case -f '%%%%' true
run_case -f 'a%Qb' true                       # unknown: `?` then the letter
run_case -f '%q' true
run_case -f '%' true                          # `?`, and NO closing newline
run_case -f 'abc%' true
run_case -f 'a\tb\nc\\d' true
run_case -f '\q' true                         # unknown escape: `?\q`
run_case -f 'a\qb' true
run_case -f '' true
run_case -f 'no sequences at all' true

# =============================================================================
# 8. The format engine: `%` sequences whose value is a measurement
# =============================================================================
# Masked. What is checked is that each letter expands to *something of the right
# shape* in the right place — `%E` is `M:SS.CC` and `%e` is seconds with two
# decimals, and a port that swapped them would still print two plausible numbers.

norm_case -f 'U=%U S=%S' true
norm_case -f 'E=%E' true
norm_case -f 'e=%e' true
norm_case -f 'E=%E e=%e' true
norm_case -f 'P=%P' true
norm_case -f 'M=%M' true
norm_case -f 'X=%X D=%D K=%K t=%t p=%p' true
norm_case -f 'I=%I O=%O' true
norm_case -f 'F=%F R=%R' true
norm_case -f 'W=%W' true
norm_case -f 'c=%c w=%w' true
norm_case -f 'k=%k' true
norm_case -f 'r=%r s=%s' true
norm_case -f 'every=%U%S%E%e%P%M%X%D%K%t%p%I%O%F%R%W%c%w%k%r%s%Z%x' true

# An hour is the branch in `%E` that switches to `h:mm:ss`; it cannot be reached
# in a test that finishes, so only the sub-hour form is exercised here.

# =============================================================================
# 9. `-p`, and why it is not the same as its own format string
# =============================================================================
# `-p` is `real %e\nuser %U\nsys %S`, and it also suppresses the
# abnormal-termination line — because upstream compares the format *pointer*
# against `posix_format`, not its text. Spelling the same format out with `-f`
# therefore keeps the line. Both are checked, because a port that implemented
# `-p` as a format assignment would pass the first and fail the second.

norm_case -p true
norm_case -p false
norm_case -p sh -c 'kill -TERM $$'
norm_case --portability true
norm_case --portability false
norm_case -f 'real %e\nuser %U\nsys %S' false
norm_case -p -q false
norm_case -p -f 'e=%e' false                  # `-f` after `-p` wins: line returns
norm_case -f 'e=%e' -p false                  # `-p` after `-f` wins: line goes

# =============================================================================
# 10. `-v`
# =============================================================================
# Twenty-three lines built by concatenating `longstats`, each one a tab, a
# label, and a field. Masked, but the labels are not digits and neither is the
# order.

norm_case -v true
norm_case -v false
norm_case --verbose true
norm_case -v sh -c 'kill -TERM $$'
norm_case -v -q false
norm_case -v -p false                         # `-v` wins: it is applied last
norm_case -p -v false
norm_case -v -f 'e=%e' false                  # ...over `-f` too

# =============================================================================
# 11. The `TIME` environment variable
# =============================================================================
# Read before the options, so `-f` overrides it and `-p` and `-v` override it.

norm_sh 'TIME="T=%e" time true'
sh_case 'TIME="lit%%eral" time true'
sh_case 'TIME="x=%x" time false'
norm_sh 'TIME="T=%e" time -f "F=%e" true'
norm_sh 'TIME="T=%e" time -p true'
norm_sh 'TIME="T=%e" time -v true'
sh_case 'TIME="" time true'
norm_sh 'TIME="%U %S" time true'

# =============================================================================
# 12. `-o FILE` and `-a`
# =============================================================================
# The summary goes to the file and stderr stays clean. `-a` appends; without it
# the file is truncated. A failure to *open* the file is fatal and 125; a
# failure to *write* it is not noticed at all, because `main` never checks the
# `fflush`.

SHOWF=1; norm_sh 'time -o "$F" true'
SHOWF=1; norm_sh 'time --output-file="$F" true'
SHOWF=1; norm_sh 'time --output="$F" true'
SHOWF=1; sh_case 'time -o "$F" -f "x=%x" false'
SHOWF=1; sh_case 'time -o "$F" -f "x=%x" true; time -o "$F" -f "x=%x" false'
SHOWF=1; sh_case 'time -o "$F" -f "x=%x" true; time -a -o "$F" -f "x=%x" false'
SHOWF=1; sh_case 'time -a -o "$F" -f "x=%x" true; time -a -o "$F" -f "x=%x" false'
SHOWF=1; sh_case 'time --append -o "$F" -f "x=%x" true; time --append -o "$F" -f "x=%x" false'
run_case -o /nodir/nofile true
run_case -o '' true
run_case -o /etc true                          # a directory
norm_case -o /dev/full true                    # write failure is not noticed
norm_case -a -o /dev/full true

# `-a` with no `-o` is accepted and does nothing.
norm_case -a true
run_case -a -f '' false

# =============================================================================
# 13. Long options and their abbreviations
# =============================================================================
# The table is `append format help output-file portability quiet verbose
# version`, in that order, which is what an ambiguous prefix's candidate list
# is built from.

norm_case --append true
run_case --format='x=%x' false
run_case --format 'x=%x' false
norm_case --qu false
norm_case --por true
norm_case --verb true
run_case --f 'x=%x' true                       # unambiguous: only `--format`
norm_case --a true                             # only `--append`
run_case --o /nodir/nofile true                # only `--output-file`
norm_case --p true                             # only `--portability`
run_case --v true                              # ambiguous: verbose, version
# `--he` and `--ver` resolve to `--help` and `--version`, whose text differs on
# purpose; they are in section 16 with the rest of that family.

# =============================================================================
# 14. Bytes that are not valid UTF-8
# =============================================================================
# The finding that brought this program up for conversion: it read `argv` and
# the environment as `String`, so every one of these panicked before doing
# anything at all. On this OS a byte over 0x7f is a legal filename character.

norm_case "$(printf 'caf\351')"
run_case -f '' "$(printf 'caf\351')"
norm_case "$(printf '\377\376')"
run_case -f 'C=[%C]' printf '[%s]' "$(printf 'na\377me')"
run_case -f 'C=[%C]' printf '[%s]' "$(printf '\351')" "$(printf '\200\201')"
run_case -f "$(printf 'f\351=%%x')" false
run_case -f "$(printf '\377')" true
run_case -o "$(printf 'no\351dir/x')" true
norm_case "$(printf 'ca\351fe')" arg1 "$(printf '\200')"
sh_case 'TIME="$(printf "t\351=%%x")" time false'
SHOWF=1; sh_case 'time -o "$F" -f "$(printf "b\351=%%x")" false'

# =============================================================================
# 15. Descriptors closed or full
# =============================================================================
# Where a Rust port is wrong by default: the runtime reopens a closed standard
# descriptor on `/dev/null` before `main` and then swallows `EBADF`.
#
# GNU Time does **not** register gnulib's `close_stdout`, so the rules here are
# not `nice`'s. Measured: `time --help >&-` exits **0** in silence, because
# nothing checks the write; but a `ferror` on the summary stream is checked
# inside `summarize`, and that one exits **1** — not 125, and not the child's
# status.

sh_case 'time true 2>&-'
norm_sh 'time true >&-'
sh_case 'time true 2>/dev/full'
sh_case 'time -f "x=%x" false 2>/dev/full'
sh_case 'time -p true 2>/dev/full'
sh_case 'time -v true 2>/dev/full'
sh_case 'time --help >&-'
sh_case 'time --help >/dev/full'
sh_case 'time --version >&-'
sh_case 'time --version >/dev/full'
sh_case 'time 2>&-'                            # missing operand, nowhere to say so
sh_case 'time -x true 2>&-'
sh_case 'time /nope 2>&-'
sh_case 'time -o /nodir/x true 2>&-'
norm_sh 'time false >&- 2>&-'

# =============================================================================
# 16. Cases that differ on purpose
# =============================================================================
# `--help`'s body matches GNU's; what follows it does not, and must not. GNU
# closes with a website, a manual URL and a bug-report address which name an
# upstream this is not. `--version` likewise names SlateOS rather than GNU Time.
#
# The third is the out-of-bounds read described in the header: upstream walks
# past the end of a format string ending in a bare backslash and prints
# whatever is next in memory.

xfail_case 'help closes with a referral to the GNU project, which this is not' --help
xfail_case 'version names SlateOS, not GNU Time' --version
# shellcheck disable=SC1003  # the backslash is the subject, not an escape
xfail_case 'upstream reads past the end of a format ending in a backslash' \
  -f 'ab\' true

# The abbreviations resolve to the same two, so they carry the same difference.
xfail_case 'help closes with a GNU referral' --hel
xfail_case 'version names SlateOS' --vers

# =============================================================================
# Summary
# =============================================================================
total=$((pass+fail+xfail+xpass))
printf 'time         %d case(s): %d passed, %d differed, %d differ on purpose, %d unexpectedly agreed\n' \
  "$total" "$pass" "$fail" "$xfail" "$xpass"
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
