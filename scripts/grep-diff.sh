#!/usr/bin/env bash
# Differential test: our grep against the host's GNU grep.
#
#     sh scripts/grep-diff.sh                      # run it
#     OURS=/usr/bin/grep sh scripts/grep-diff.sh   # control: should be all green
#
# ## Why this harness exists, and why it is the one that can answer the
# ## question that started it
#
# `grep -E` is not the POSIX-extended syntax the same engine gives `osh`,
# `find -regextype posix-extended` and `awk`; GNU asks glibc for
# `RE_SYNTAX_EGREP`. That was filed as a known issue for weeks in a form that
# named the wrong flags, because it had been derived by *reading* the glibc
# header rather than by running the two binaries. Measuring reduced the
# difference to two bits and showed the posix-extended side needed no change at
# all — the opposite of what the header reading had concluded. The same
# question is still open for `-G` (`RE_SYNTAX_GREP`), and this file is how it
# gets answered: by asking the real grep, one pattern at a time.
#
# ## Why it runs on the host rather than inside WSL
#
# Unlike `find`, nothing `grep` does needs an inode. It reads bytes and writes
# lines, `grep.rs` has no `#[cfg(unix)]` in it, and the host has a GNU grep. So
# this takes the ordinary `*-diff.sh` shape — build a native binary, run both
# side by side — and not `find-diff.sh`'s WSL detour.
#
# ## What is compared
#
# stdout and the exit status, byte for byte. stderr only has to agree about
# *whether* there was a diagnostic, for the reason `sed-diff.sh` gives: the
# wording of a regex error comes from glibc on GNU's side and from `ere` on
# ours, and matching glibc's phrasing would be fitting to its parser's
# internals rather than to grep. What matters — and what is compared exactly —
# is that a pattern GNU refuses is a pattern we refuse, with the same status.
set -u

# Our grep is a native Windows binary, so MSYS would helpfully rewrite an
# argument that looks like a path — turning the pattern `/x/` into `X:\`.
export MSYS2_ARG_CONV_EXCL='*'

# grep reads these, and inheriting them from the operator's shell would change
# both sides at once — the worst kind of difference, because it agrees.
unset GREP_OPTIONS GREP_COLOR GREP_COLORS POSIXLY_CORRECT
export LC_ALL=C

. "$(dirname "$0")/diff-subject.sh"
OURS=$(subject_binary coreutils grep "${OURS:-}") || exit 1
GNU=${GNU:-$(command -v grep)}

pass=0; fail=0; xfail=0; xpass=0

fixtures=$(mktemp -d) || exit 1
trap 'rm -rf "$fixtures"' EXIT
cd "$fixtures" || exit 1

# ---------------------------------------------------------------- fixtures ---
#
# Each exists to make one rule observable:
#
#   abc          three one-letter lines: the smallest thing -v, -c and -m can
#                disagree about
#   words        repeated words at different offsets, for -o, -w and -m
#   braces       the literal text `a{b}`, `{b}a`, `a{}` — the lines that tell
#                an interval from a literal brace, which is the whole subject
#                of the -E dialect
#   mixed        case and digits, for -i and the character classes
#   nonl         no trailing newline: does the last line still get one
#   empty        no lines at all
#   binfile      a NUL byte, which GNU calls binary and we deliberately do not.
#                *Not* named `nul`: that is a reserved device name on Windows,
#                so MSYS's `>` made a real file while our native binary opened
#                the null device and reported an empty file. The harness then
#                blamed grep for a difference the fixture had created.
#   zsep         NUL-separated records, for -z
#   pats         a pattern file for -f, including an empty line (which matches
#                everything, and is the classic -f footgun)
printf 'a\nb\nc\n'                              > abc
printf 'foo bar\nbaz foo foo\nqux\nfoofoo\n'    > words
printf 'a{b}\n{b}a\na{}\nab\na\naaa\na{1,2}\n'  > braces
printf 'Alpha1\nbeta22\nGAMMA333\nalpha\n'      > mixed
printf 'a\nb'                                   > nonl
: > empty
printf 'bin\0ary\nplain\n'                      > binfile
printf 'foo\0bar\0foo bar\0'                    > zsep
printf 'foo\n\nqux\n'                           > pats
mkdir -p sub/deep
printf 'foo\n'  > sub/s1
printf 'bar\n'  > sub/deep/s2

# ------------------------------------------------------------------- cases ---
#
# One shell command line per case, `grep` standing for whichever grep is
# running. Blank lines and `#` lines are ignored; a line beginning `!` is a case
# expected to differ, and the text between `!` and `|` says why.
#
# `grep` is a *function* rather than a textual substitution, so that a case can
# put something before it (`GREP_COLORS=… grep …`) and so that a pattern
# containing the word `grep` is not rewritten.
grep() { "$SUBJ" "$@"; }

# Everything observable about one run: stdout, the exit status, and whether
# there was a diagnostic — but not its text. See the header.
#
# `</dev/null` is not tidiness. A case that names no file — `grep a`, or plain
# `grep` — reads *stdin*, and stdin here is the case list itself: the first
# such case swallowed every remaining line and the harness reported "12 passed"
# for a file with 180 cases in it. The cases that do want stdin say so with an
# explicit `< file`, which overrides this.
#
# The `tr` is not cosmetic either. A command substitution discards NUL bytes —
# with a warning, which is how this was noticed — so without it every `-Z` and
# `-z` case would capture identically to the same case without the flag, and
# the whole NUL-delimited half of grep would agree by construction. `\002` is a
# byte neither implementation writes. The exit status is folded into the same
# capture (after a `\001`) because it has to survive the pipeline that the `tr`
# introduces.
capture() {
    SUBJ=$1
    local body err loud
    err=$(mktemp)
    body=$( { eval "$2" </dev/null 2>"$err"; printf '\001rc=%s' "$?"; } | tr '\0' '\002' )
    loud=quiet
    [ -s "$err" ] && loud=loud
    rm -f "$err"
    printf '%s loud=%s' "$body" "$loud"
}

# Render a capture for a human: `|` for a newline, `@` for a NUL.
show() { printf '%s' "$1" | tr '\n\002\001' '|@ '; }

run_case() {
    local line=$1 expect_diff=0 reason=""
    case $line in
        '!'*)
            expect_diff=1
            reason=${line#!}
            reason=${reason%%|*}
            line=${line#*|}
            ;;
    esac

    local a b
    a=$(capture "$GNU" "$line")
    b=$(capture "$OURS" "$line")

    if [ "$a" = "$b" ]; then
        if [ "$expect_diff" = 1 ]; then
            xpass=$((xpass + 1))
            printf 'XPASS  %s\n     (expected to differ: %s)\n' "$line" "$reason"
        else
            pass=$((pass + 1))
        fi
        return
    fi
    if [ "$expect_diff" = 1 ]; then
        xfail=$((xfail + 1))
        return
    fi
    fail=$((fail + 1))
    printf 'FAIL   %s\n' "$line"
    printf '  gnu  | %s\n' "$(show "$a")"
    printf '  ours | %s\n' "$(show "$b")"
}

# The case list arrives on fd 3, not stdin, so that `capture`'s own redirection
# is the only thing deciding what a case reads.
while IFS= read -r case_line <&3; do
    case $case_line in ''|'#'*) continue ;; esac
    run_case "$case_line"
done 3<<'CASES'
# --- the plainest thing it does ---
grep a abc
grep b abc
grep z abc
grep '' abc
grep a empty
grep foo words
grep a nonl
grep b nonl
grep a abc abc
grep a abc empty
grep a /nonexistent
grep -s a /nonexistent
grep a
grep

# --- stdin, which is what "no file" means ---
grep a < abc
grep -c a < abc
grep -n a < abc
grep -H a < abc
grep -l a < abc
grep a - < abc
grep a abc - < braces
grep a < empty

# --- BRE, which is what you get with no flag at all ---
grep '^a' abc
grep 'a$' abc
grep '^a$' abc
grep '^$' pats
grep '.' abc
grep 'a*' abc
grep 'aa*' braces
grep '[abc]' abc
grep '[^a]' abc
grep '[a-c]' abc
grep '[[:alpha:]]' mixed
grep '[[:digit:]]' mixed
grep '[[:upper:]]' mixed
grep 'a\{2\}' braces
grep 'a\{2,\}' braces
grep 'a\{1,2\}' braces
grep 'a{b}' braces
grep 'a{' braces
grep '{b}' braces
grep 'a\|b' abc
grep 'ab\?' braces
grep 'a\+' braces
grep '\(a\)\1' braces
grep '\(a\)b' braces
grep '\.' braces
grep '\\' braces
grep 'a[' braces
grep 'a\{2' braces
grep 'a\{2,1\}' braces
grep '\(a' braces
grep 'a\)' braces
grep '*a' braces
grep '\w' mixed
grep '\W' words
grep '\s' words
grep '\S' words
!we refuse \< \> \b \B: the engine has no word-boundary matcher, and there is no spelling that would quietly do the wrong thing (bre.rs)|grep '\<foo' words
!we refuse \< \> \b \B: the engine has no word-boundary matcher, and there is no spelling that would quietly do the wrong thing (bre.rs)|grep 'foo\>' words
!we refuse \< \> \b \B: the engine has no word-boundary matcher, and there is no spelling that would quietly do the wrong thing (bre.rs)|grep '\bfoo' words

# --- -E, the egrep dialect: the two syntax bits, measured ---
grep -E 'a+' braces
grep -E 'a?b' braces
grep -E 'a|b' abc
grep -E '(a)(b)' braces
grep -E 'a{2}' braces
grep -E 'a{2,}' braces
grep -E 'a{1,2}' braces
grep -E '[abc]+' abc
# RE_CONTEXT_INDEP_OPS: a quantifier with nothing to quantify repeats the empty
# expression rather than being REG_BADRPT.
grep -E '*a' braces
grep -E '+a' braces
grep -E '?a' braces
grep -E 'a^*b' braces
grep -E '^*' abc
# RE_INVALID_INTERVAL_ORD: a `{` that does not open a well-formed interval is a
# literal brace.
grep -E 'a{b}' braces
grep -E 'a{' braces
grep -E '{b}' braces
grep -E 'a{,}' braces
# A well-formed-looking but wrong interval stays an error in both dialects.
grep -E 'a{}' braces
grep -E 'a{1,2,3}' braces
grep -E 'a{2,1}' braces
grep -E 'a{99999999}' braces
grep -E 'a(' braces
grep -E 'a)' braces
grep -E 'a[' braces
# GNU is two engines here — at the start of an expression glibc regcomp skips
# the offending token while dfa.c makes it a literal — so these exit 1 with no
# diagnostic. They were written as expected-to-differ and turned out to agree:
# rolling the brace back to a literal lands on the same observable answer as
# GNU's disagreement does. Kept, unmarked, as the cases that would notice if
# either side started erroring.
grep -E '{}a' braces
grep -E '{1,2,3}a' braces

# --- -F, where none of the above is a metacharacter ---
grep -F 'a{b}' braces
grep -F '*a' braces
grep -F '.' braces
grep -F '[abc]' abc
grep -F 'a' abc
grep -F '' abc
grep -Fx 'a' braces
grep -Fw 'foo' words

# --- selecting, counting, and not printing ---
grep -v a abc
grep -c a abc
grep -c a abc empty
grep -vc a abc
grep -q a abc
grep -q z abc
grep -l a abc empty
grep -L a abc empty
grep -l z abc
grep -n a abc
grep -nv a abc
grep -o 'foo' words
grep -o 'o*' words
grep -o '' abc
grep -on 'foo' words
grep -m 1 foo words
grep -m 0 foo words
grep -m 2 o words
grep -w foo words
grep -w o words
grep -x a abc
grep -x foo words
grep -i alpha mixed
grep -i ALPHA mixed
grep -ic a mixed
grep -iw ALPHA mixed
grep -h a abc empty
grep -H a abc
grep -H a abc empty
grep -Hn a abc

# --- -e and -f, and the empty pattern that matches everything ---
grep -e a abc
grep -e a -e b abc
grep -e '^a' -e 'c$' abc
grep -f pats words
grep -cf pats words
grep -f /nonexistent abc
grep -e a -f pats words
grep -F -e a -e b abc

# --- option parsing at the edges ---
grep -- -a braces
grep -e -a braces
grep -ivn a mixed
grep -in a mixed
grep -Z a abc
grep --zzz a abc
grep -m x foo words

# --- recursion ---
#
# A recursive hit is named `sub/s1` by GNU and `sub\s1` by us *in this harness
# only*: the path is built with `Path::join`, which uses the host separator,
# and the subject here is the Windows build. On the target — where `/` is the
# separator and the Windows build does not exist — the two agree. `grep foo
# sub` (no -r) is the unmarked control: it names no child path, and passes.
!the Windows build joins recursive paths with `\`; the target build joins with `/`|grep -r foo sub
!the Windows build joins recursive paths with `\`; the target build joins with `/`|grep -rn foo sub
!the Windows build joins recursive paths with `\`; the target build joins with `/`|grep -rl foo sub
!the Windows build joins recursive paths with `\`; the target build joins with `/`|grep -rc foo sub
!the Windows build joins recursive paths with `\`; the target build joins with `/`|grep -r foo .
grep foo sub

# --- -Z and -z, the NUL-delimited pair ---
grep -Z a abc
grep -lZ a abc
grep -LZ z abc
grep -HZ a abc
grep -HZc a abc
grep -nZ a abc abc
!the Windows build joins recursive paths with `\`; the target build joins with `/`|grep -rlZ foo .
grep -z foo zsep
grep -zc foo zsep
grep -zn foo zsep
grep -zo foo zsep
grep -zHc foo zsep
grep -zl foo zsep
grep -zZl foo zsep
grep -z foo words
!we never suppress binary output; GNU 3.0 sees the NUL and replaces the lines with "Binary file X matches"|grep foo zsep

# --- binary ---
!we never suppress binary output; GNU 3.0 replaces the line with "Binary file X matches"|grep ary binfile
grep -a ary binfile
grep -c ary binfile
grep -q ary binfile
CASES

printf '\n%d passed, %d differed, %d deliberate, %d unexpectedly agreed\n' \
    "$pass" "$fail" "$xfail" "$xpass"
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
