#!/usr/bin/env bash
# Differential test: our `test` against GNU `test`.
#
# ## Why this harness batches, when the other twenty-eight do not
#
# It was written when the established shape in this tree was one reference
# invocation per case — `wsl -e env LC_ALL=C.UTF-8 <util> …`, compared against
# one native run — costing a WSL process launch per case, roughly a second and a
# half on this host. That is affordable at `csplit`'s 86 cases and already
# unpleasant at `split`'s 210.
#
# `test` needs many more cases than either. Its behaviour is not defined by a
# list of options but by a *grammar*, and the grammar's rules interact: what
# `test a = b` means depends on the argument **count** before it depends on the
# operator, so every operator has to be tried at several counts, under `!`,
# inside parentheses, and on both sides of `-a`/`-o`. That is hundreds of cases,
# and at a WSL spawn each it would be a twenty-minute harness nobody runs.
#
# So this one batches. Every case is a line in a file, arguments joined by
# `\x1f` (ASCII Unit Separator); a small script reads them all and emits one
# record per case, and it is run **once** on each side.
#
# Since the move to `scripts/diff-wsl.sh` the harness is *already* inside WSL,
# so the launch it was avoiding now costs a fork rather than a VM entry, and the
# batching is an optimisation rather than a necessity. It is kept because it is
# not *wrong*: `test` is the rare utility with no output to compare — it writes
# nothing on success, and its entire product is an *exit status* plus, on a
# malformed expression, one line of stderr. There is no file to leave behind and
# no stdout to interleave, so batching cannot let one case contaminate the next.
# A batched `split` harness would be wrong for exactly the reason this one is
# right.
#
# ## Why the exit status is the whole point
#
# `test` has three statuses, not two, and the third is the one that matters:
#
#   0  the expression is true
#   1  the expression is false
#   2  the expression is not an expression
#
# Collapsing 2 into 1 is not a cosmetic bug. `if [ "$x" -eq 0 ]; then` on a
# non-numeric `$x` must *fail loudly*; a `test` that quietly answers "false"
# sends the script down the else branch, and a `test` that quietly answers
# "true" sends it down the then branch with garbage. Both were happening here
# before this harness existed — see `known-issues.md`. Every case below
# therefore compares the status exactly, never merely "did it succeed".
#
# ## Why both sides run inside WSL
#
# `scripts/diff-wsl.sh` gives the reasons. The one that bites here is the
# reference: the host's `test` is MSYS2's, a Cygwin derivative linking
# `msys-2.0.dll` rather than glibc, and its diagnostics are worded differently
# (`known-issues.md` → `TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE`).
#
# The second reason is specific to this utility and is worth naming: `test` is
# almost entirely *about* the filesystem, and the two hosts do not agree about
# what a file is. The probe this preamble replaces existed because WSL was
# entered with the Windows cwd and might land somewhere else; the whole "not
# covered" list at the foot of this file existed because a symlink or a fifo
# made by MSYS is not the same object a WSL binary sees on DrvFs. With both
# sides in one process tree on one Linux filesystem, neither problem exists —
# `$DIFF_TMP` is a real `/tmp` directory, not a `/mnt/c` view of one.
#
# ## Why LC_ALL=C.UTF-8
#
# `diff-wsl.sh` fixes it there, for the whole family. The diagnostics quote the
# offending argument back at the caller through gnulib's `quote()`, which picks
# its quote marks from the locale. Since §351 ours prints U+2018/U+2019 in every
# locale, and GNU prints those under a UTF-8 locale and ASCII under `C` — so
# `C.UTF-8` is the setting the two agree in. This file ran under `C` for the
# mirror-image of that reason until B-Q2 was answered; nothing else here reads
# the locale.
set -u

# Into WSL, build ours for Linux, find glibc's, and put both behind the one name
# `test` so `argv[0]` matches. See `scripts/diff-wsl.sh`.
#
# The reference is named explicitly because `command -v test` inside a shell
# finds the *builtin*, which is a different implementation and deliberately not
# what is compared. `/usr/bin/test` is the real binary.
DIFF_PROG=test
DIFF_REF='/usr/bin/test /bin/test'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

US=$'\x1f'   # joins the arguments of one case
RS=$'\x1e'   # ends one case's record in the results

work=$DIFF_TMP/work
mkdir -p "$work"
cd "$work" >/dev/null || exit 1

# --- fixtures -----------------------------------------------------------------
#
# Deliberately plain. Anything exotic (symlinks, device nodes, setuid bits) is
# either unrepresentable on this host or represented differently by MSYS and
# WSL, and a fixture the two sides disagree about tests the harness, not `test`.
# The operators that need those fixtures are listed under "not covered" at the
# bottom of this file rather than being tested against a fiction.

: > empty            # exists, regular, zero length: -e -f -s
printf 'x' > full    # exists, regular, one byte:    -s is the difference
mkdir -p adir        # exists, directory:            -d
# `missing` is deliberately never created.

# --- machinery ----------------------------------------------------------------

cases=$work/cases     # one line per case, arguments joined by US
labels=$work/labels   # one line per case, human-readable, same order
whys=$work/whys       # `-` for an ordinary case, else the xfail reason
: > "$cases"; : > "$labels"; : > "$whys"

# Record one case, as `<count>US<arg1>US<arg2>…`.
#
# The count is not redundant. Joining the arguments alone would encode `test`
# and `test ''` as the same empty line, and those are *different expressions*
# with different answers — no arguments is false, one empty argument is also
# false but by a different rule, and the rule is what a rewrite can break. The
# count also makes a trailing empty argument survive, which `read` would
# otherwise swallow.
add() {
  local why=$1; shift
  local n=$# joined
  local IFS=$US
  joined="$*"
  printf '%s%s%s\n' "$n" "$US" "$joined" >> "$cases"
  printf 'test %s\n' "$(printf '%s' "$joined" | tr "$US" ' ')" >> "$labels"
  printf '%s\n' "$why" >> "$whys"
}

# The ordinary case, and the expected-to-differ case. Counted separately so a
# case that starts agreeing is reported too — an xfail that has silently become
# correct is a stale note in the harness.
t()     { add - "$@"; }
xfail() { local why=$1; shift; add "$why" "$@"; }

# The runner, in the shell dialect both sides share. It reads the case file and
# writes one record per case: status, US, stderr, US, stdout, RS.
#
# stdout is compared even though `test` should never write any, precisely
# because it should never write any: the utility's whole contract is "say
# nothing, mean it with the exit status", so a stray byte on stdout is itself
# the bug. It is captured by redirecting to a file rather than by running the
# case a second time, which would double the work to learn something we expect
# to be empty every time.
#
# The `test: ` prefix on a diagnostic is *compared*, not stripped. GNU echoes
# argv[0] verbatim, so when this harness named its reference `/usr/bin/test`
# every single error case "differed" from our bare `test:` — 37 false diffs on
# its first run, all of them the prefix and none of them the message — and the
# fix at the time was to strip a known prefix from each side. `diff-wsl.sh`
# removes the need: both binaries are reached through a symlink named `test` in
# a directory on `PATH`, so both are handed the bare word as argv[0] and both
# print `test: `. That gets back the one thing the stripping cost, which is the
# ability to catch a *wrong* prefix.
#
# `env` is what makes that work. A bare `test` inside this runner would be
# bash's builtin, which is a different implementation and deliberately not what
# is being compared; a plain path would put that path in argv[0] and bring the
# prefix problem back. `env` searches `PATH` and passes the bare name along.
runner_body='
US=$(printf "\037"); RS=$(printf "\036")
tmp=$(mktemp)
trap "rm -f \"$tmp\"" EXIT
while IFS= read -r line; do
  n=${line%%"$US"*}
  rest=${line#*"$US"}
  [ "$n" = "$line" ] && rest=""
  argv=()
  i=0
  while [ "$i" -lt "$n" ]; do
    argv+=("${rest%%"$US"*}")
    rest=${rest#*"$US"}
    i=$((i+1))
  done
  err=$(env test "${argv[@]}" 2>&1 >"$tmp" </dev/null); rc=$?
  out=$(<"$tmp")
  printf "%s%s%s%s%s%s" "$rc" "$US" "$err" "$US" "$out" "$RS"
done
'

# `$1` is `ours` or `gnu`, `$2` the output file. That side's directory goes on
# the *front* of `PATH` rather than replacing it: the runner needs `mktemp` and
# `env` from the system, and a one-entry `PATH` — the idiom the single-case
# harnesses in this family use — would starve it of both. Our `test` still wins
# the lookup, because it is first.
run_side() {
  PATH="$bindir/$1:$PATH" bash -c "$runner_body" < "$cases" > "$2" 2>/dev/null
}

# --- the cases ----------------------------------------------------------------
#
# Ordered by argument count, because that is the order `test` itself decides in:
# POSIX defines the zero-, one-, two-, three- and four-argument forms
# explicitly, and only past four does a parser get involved. A bug in the count
# rules therefore hides every bug in the operators, so the counts come first.

# --- zero and one argument ---
t
t ''
t x
t -
t '-f'          # a lone operator name is a *string*, and a non-empty one
t '!'
t '('
t ')'
t '-a'
t '-o'
t '='

# --- two arguments: `!` and the unary operators ---
t '!' ''
t '!' x
t '!' '!'
t '!' '('
t -n ''
t -n x
t -z ''
t -z x
t -e empty
t -e missing
t -f empty
t -f adir
t -d adir
t -d empty
t -s empty
t -s full
t -r empty
t -w empty
t -x adir
t -b empty
t -c empty
t -p empty
t -S empty
t -g empty
t -u empty
t -k empty
t -O empty
t -G empty
t -L empty
t -h empty
t -N empty
t -t 1
t -t 0
t -t x
t -q x          # not an operator: two arguments, first not unary -> error
t -- x
t x y           # two non-operator arguments -> error

# --- three arguments: the binary operators ---
t x = x
t x = y
t '' = ''
t x != y
t x != x
t x == x        # a GNU extension, and it is *not* the same as `=` everywhere
t x '<' y
t y '<' x
t x '>' y
t 1 -eq 1
t 1 -eq 2
t 1 -ne 2
t 1 -lt 2
t 2 -le 2
t 3 -gt 2
t 3 -ge 4
t -1 -eq -1
t +1 -eq 1
t ' 1' -eq 1    # leading blank: accepted, like strtol
t '1 ' -eq 1    # trailing blank
t '' -eq 1
t abc -eq 0     # the case that made this harness: must be an *error*
t 1 -eq abc
t 0x10 -eq 16   # not hex to `test`
t 010 -eq 8     # not octal either
t 9223372036854775807 -eq 9223372036854775807
t 9223372036854775808 -eq 1     # past int64
t -9223372036854775808 -lt 0

# Arbitrary precision. `test` compares the decimal *text*, so these have
# answers rather than overflows; an implementation that reaches for i64 gets
# every one of them wrong, and the first two silently.
t 99999999999999999999999999 -eq 99999999999999999999999999
t 99999999999999999999999999 -lt 100000000000000000000000000
t 100000000000000000000000000 -gt 99999999999999999999999999
t -99999999999999999999999999 -lt 0
t -00000000005 -eq -5            # leading zeros, and a sign
t 0000 -eq 0
t -0 -eq 0                       # negative zero is zero, not less than it
t +0 -eq -0
t '  7  ' -eq 7                  # blanks both ends
t '7 7' -eq 77                   # ...but not *inside*: an error
t ' ' -eq 0                      # all-blank is an error, not zero
t - -eq 0                        # a lone sign is not a number
t empty -ef empty
t empty -ef full
t empty -nt full
t empty -ot full
t missing -ef missing
t x -eq y
t x -badop y
t '!' -n x
t '!' -z x
t '!' -f empty
t '(' x ')'
t '(' '' ')'
t '(' '!' ')'
t x -a y
t x -a ''
t '' -o y
t '' -o ''
t -n -a x       # `-n` as an *operand* of -a, not a unary operator

# --- four arguments ---
t '!' x = x
t '!' x = y
t '!' 1 -eq 1
t '!' '(' x ')'
t '(' -n x ')'
t '(' -z x ')'
t '(' x = x     # unbalanced
t x = x ')'     # unbalanced the other way
t '!' '!' -n x
t '!' '!' '!' x
t -n x -a y
t x -a -n y
t x -o -z ''

# --- five or more: the parser proper ---
t x = x -a y = y
t x = x -a y = z
t x = y -o y = y
t x = y -o y = z
t '!' x = y -a y = y
t '(' x = x ')' -a y = y
t '(' x = y -o y = y ')'
t '(' '(' x = x ')' ')'
t '(' x = x -a y = y ')' -o z = w
t x = x -a y = y -a z = z
t x = y -o y = z -o z = z
t x = x -o y = y -a '' = z   # precedence: -a binds tighter than -o
t '' = x -a x = x -o y = y
t '!' '(' x = x ')'
t '!' '(' x = y ')'
t -n x -a -n y -a -n z
t -z '' -o -z x
t '(' -n x ')' -a '(' -z '' ')'
t '(' x = x ')'                  # a whole expression in parentheses, alone

# Neither connective short-circuits. GNU accumulates with `value |= and()` and
# `value &= term()`, so the far side is evaluated even once the answer is
# settled — and an error over there is still reported. An implementation that
# returns early looks correct on every case whose far side is well-formed,
# which is every case anyone writes on purpose.
t x = x -o abc -eq 1             # true on the left, error on the right
t x = y -a abc -eq 1             # false on the left, error on the right
t x = x -o -q foo
t x = y -a -q foo

# `-l STRING` is an integer equal to that string's length, accepted wherever an
# integer is. Undocumented outside `--help`'s last line, and absent from most
# reimplementations.
t -l abc -eq 3
t -l abc -gt 2
t -l '' -eq 0
t 3 -eq -l abc                   # and on the right-hand side too
t -l abc -eq -l xyz              # ...and on both at once
t -l abc -lt -l wxyz
t -l abc -ef x                   # refused by name on the file comparisons
t x -ef -l abc
t -l abc -nt x
t -l abc -ot x
t -l                             # not enough arguments left for it to be `-l`
t x -eq -l                       # ditto on the right

# --- malformed: every one of these must be status 2, with a message ---
t '('
t ')'
t '(' ')'
t '(' x
t x ')'
t '!'
t x -a
t x -o
t -a x
t -o x
t 1 -eq
t -eq 1
t x =
t = x
t '(' x = x -a
t '!' '('
t x y z
t x y z w
t x = x = x
t '(' '(' x ')'
t '(' x ')' ')'
t x -a -a y
t -n
t -z
t -f
t -t

# `-t` has two different failure modes and they are not interchangeable: a
# malformed descriptor number is an error (it goes through the same integer
# check as `-eq`), while a well-formed one that is simply too large is false.
t -t x
t -t ''
t -t 99999999999999999999
t -t -1
t -t 0                           # redirected in this harness, so false

# `--` is not an end-of-options marker here — `test` has none. Alone it is an
# ordinary non-empty string; in front of an operand it is a bad operator.
t --
t -- x
t '!' --

# --- `--help` and `--version`, which are NOT options here ---
#
# POSIX requires `test --help` to be the one-argument form applied to the
# non-empty string "--help": status 0, not a word of output. GNU obeys this
# (measured: rc=0, 0 bytes, for both spellings). The long options exist only
# for the `[` spelling and only as the sole argument before the `]` — which
# this harness cannot reach, because argv[0] is not settable from the runner.
# So these are ordinary cases, not expected differences; if ours ever prints
# a usage message here it is a real bug and must show up as a diff.
t --help
t --version
t --help x       # not alone: an ordinary two-argument expression -> error
t x --help
t '!' --help
t --version x

# --- run and compare ---------------------------------------------------------

run_side ours "$work/ours.out"
run_side gnu  "$work/gnu.out"

# Flatten both RS-delimited result streams to one record per line, then join
# them with the labels and reasons so a single awk can walk all four.
#
# The obvious loop — `awk 'NR==n'` over the result file once per case — is
# what this replaces. It re-read both files for every case and forked three
# subshells per case; at 157 cases that was ~470 process creations, which on
# MSYS took thirteen minutes for work that is one linear pass. A harness slow
# enough to be run reluctantly is a harness that stops being run.
#
# Flattening is safe because a `test` record cannot contain a newline: the
# diagnostics are single-line and stdout is always empty. If that ever stops
# being true the field count goes wrong and the case reports as a difference,
# which is noisy but not silent.
for side in ours gnu; do
  tr "$RS" '\n' < "$work/$side.out" | sed '$d' > "$work/$side.lines"
done
paste -d "$RS" "$labels" "$whys" "$work/ours.lines" "$work/gnu.lines" \
  | awk -F"$RS" -v US="$US" -v verbose="${VERBOSE:-}" '
    { label = $1; why = $2; o = $3; g = $4 }
    o == g && why == "-" { pass++; if (verbose) printf "OK   %s\n", label; next }
    o == g               { xpass++
                           printf "XPASS %s  (expected to differ: %s)\n", label, why
                           next }
    why != "-"           { xfail++; if (verbose) printf "XFAIL %s  (%s)\n", label, why
                           next }
    {
      fail++
      gsub(US, "|", o); gsub(US, "|", g)
      printf "DIFF %s\n  ours: %s\n  gnu : %s\n", label, o, g
    }
    END {
      printf "\n%d passed, %d differed, %d differ on purpose", pass, fail, xfail
      if (xpass) printf ", %d no longer differ (update the harness)", xpass
      printf "\n"
      exit (fail == 0 && xpass == 0) ? 0 : 1
    }'
rc=$?

# --- not covered, and why ------------------------------------------------------
#
# `-b -c -p -S` (block/character/fifo/socket) and `-g -u -k` (setgid/setuid/
# sticky) appear above only in their *false* form, against a plain file, which
# pins "does not crash, does not claim yes" and nothing more. `-L -h -N -O -G`
# are in the same position.
#
# That used to be forced: MSYS could not create the file types, and one made in
# WSL under /mnt/c was not seen as such by a native Windows binary, so a fixture
# the two sides disagreed about would have tested the harness rather than
# `test`. Since the move to `diff-wsl.sh` both sides read one real Linux
# filesystem, so `mkfifo`, `ln -s`, `chmod u+s` and a dangling symlink are all
# representable and all mean the same thing to both. The true-form cases are
# therefore now *possible* and merely absent; adding them is worth a commit of
# its own, since it is new coverage rather than the same coverage moved.
# (`-O`/`-G` and `-N` still need care: the first two are true of everything this
# harness creates, and the third needs an atime the mount option may not update.)
#
# `-t` is tested only for the answers that do not depend on the terminal: both
# sides run with stdin, stdout and stderr redirected, so every `-t N` is false,
# and what is being compared is the *argument handling*, not the tty detection.
#
# `[` — the same program under its other name, where the final `]` is mandatory
# — is not exercised here. It is now reachable in principle (a second symlink in
# each side's `PATH` directory would do it) but is currently covered by unit
# tests in `test.rs` instead.
exit "$rc"
