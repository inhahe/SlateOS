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
# ## Why it moved into WSL
#
# It used to argue that it did not need to: unlike `find`, nothing grep does
# needs an inode, `grep.rs` has no `#[cfg(unix)]` in it, and the Windows host
# has a GNU grep. Both halves of that were wrong.
#
# **The reference was MSYS2's**, which is Cygwin-derived — a different libc, a
# different regex build, and getopt diagnostics glibc does not print. A harness
# that certifies against it certifies wording no GNU/Linux system produces.
#
# **The subject being a Windows binary was corrupting the results**, and
# visibly: *six* cases were recorded as deliberate divergences whose entire
# stated reason was "the Windows build joins recursive paths with `\`". They
# were not divergences at all. They were the harness measuring its own host, and
# they made the whole of `-r` — the flag people actually use grep with — into a
# blind spot. Inside WSL the separator is `/` on both sides and every one of
# them is now a plain case.
#
# The second reason `diff-wsl.sh` gives applies too, even though `grep.rs`
# itself is portable: `coreutils::stdfd`, which is what makes `grep pat >&-`
# behave rather than crash, is `#[cfg(target_os = "linux")]`. A Windows-hosted
# harness cannot exercise a line of it.
#
# ## What is compared
#
# stdout and the exit status, byte for byte, always.
#
# stderr text is compared **by default**, which is the change from the old
# harness — it compared presence only, for every case. The reason it gave is
# real but narrow: the wording of a *pattern* error comes from glibc's regcomp
# on GNU's side and from `ere` on ours, and matching glibc's phrasing would fit
# us to its parser's internals rather than to grep. That argument covers about
# fifteen cases. It does not cover `grep: /nonexistent: No such file or
# directory`, or `grep: invalid max count`, or an unknown-option diagnostic,
# which are plain reports a script may reasonably grep for and which nothing
# here had ever checked.
#
# So the rule is inverted: text is pinned unless a case opts out with `~`.
#
# ## The case-list markers
#
# | prefix | meaning |
# |---|---|
# | (none) | stdout, status and stderr text must all match |
# | `~REASON\|` | stderr text is glibc's to choose; compare only whether there was a diagnostic |
# | `!REASON\|` | differs **on purpose**; the run fails if it stops differing |
# | `?REASON\|` | differs because we have not built it **yet** |
#
# `!` and `?` behave identically — both expect a difference and both fail the
# run when the difference goes away, which is what makes them self-deleting.
# They are spelled differently because they mean opposite things to a reader
# deciding whether to open a bug: `!` is a decision, `?` is a debt. Counting
# them together, as this harness did when every `!` was really a `?`, is how
# six cases spent months labelled "deliberate" while describing a defect.
set -u

# grep reads these, and inheriting them from the operator's shell would change
# both sides at once — the worst kind of difference, because it agrees.
unset GREP_OPTIONS GREP_COLOR GREP_COLORS POSIXLY_CORRECT

DIFF_PROG=grep
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xgap=0; xpass=0

# Under `$DIFF_TMP`, so that `diff-wsl.sh`'s one EXIT trap removes it. A second
# `trap ... EXIT` here would replace that one rather than add to it, and leak
# the scratch directory every run.
fixtures=$DIFF_TMP/fixtures
mkdir -p "$fixtures" || exit 1
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
#   binfile      a NUL byte, which GNU calls binary and we deliberately do not
#   zsep         NUL-separated records, for -z
#   pats         a pattern file for -f, including an empty line (which matches
#                everything, and is the classic -f footgun)
#   accent       the same word in two cases with a two-byte character in it:
#                what `-i`, `.` and `[[:alpha:]]` do to a character that is not
#                one byte. Only meaningful now that the locale is `C.UTF-8` on
#                both sides rather than whatever the Windows host had.
#   ctx          eight numbered lines with two well-separated hits, so -A, -B
#                and -C have room to overlap or not
#   ctxtop       a hit on the *first* line, which is the only way to ask
#                whether a file's opening group takes a `--` when it is already
#                adjacent to the top of the file (it does: a new file is never
#                adjacent to the previous one)
#   run3         three consecutive hits, so that `-m` can be satisfied while
#                trailing context is still owed — the lines that follow print
#                as context even though they match
#   w99          exactly ninety-nine bytes, which is the only size here that
#                gives `-T` a field width above 1 *and* a different one with
#                `-n` than without it
printf 'a\nb\nc\n'                              > abc
printf 'foo bar\nbaz foo foo\nqux\nfoofoo\n'    > words
printf 'a{b}\n{b}a\na{}\nab\na\naaa\na{1,2}\n'  > braces
printf 'Alpha1\nbeta22\nGAMMA333\nalpha\n'      > mixed
printf 'a\nb'                                   > nonl
: > empty
printf 'bin\0ary\nplain\n'                      > binfile
printf 'foo\0bar\0foo bar\0'                    > zsep
printf 'foo\n\nqux\n'                           > pats
printf 'caf\303\251\nCAF\303\211\ncafe\n'       > accent
printf '1\n2\nHIT\n4\n5\n6\nHIT\n8\n'           > ctx
printf 'HIT\n2\n3\n'                            > ctxtop
printf 'HIT\nHIT\nHIT\n'                        > run3
# CRLF, because colouring treats the carriage return as part of the line
# terminator rather than as text: `sl`'s run stops before it. The third line
# ends with a CR that is *not* followed by a newline, and the second holds a CR
# in the middle, where it is ordinary text and does get painted.
printf 'foo\r\nfoo\rzz\r\nfoo bar\r'            > crlf

# Exactly ninety-nine bytes: nine eleven-byte lines. The one fixture whose
# `-T` field width is neither 1 nor equal with and without `-n` — 99 bytes is
# two digits, and the 100 lines a 99-byte file could hold is three. Every other
# fixture here is under ten bytes per field and so cannot tell a computed width
# from a hardcoded one.
i=0
while [ $i -lt 8 ]; do printf 'nine bytes\n'; i=$((i + 1)); done > w99
printf 'HITxxxxxxx\n' >> w99

# The recursion fixtures. `sub` is the plain tree; `symdir` exists because -r
# and -R differ over exactly one thing — a symlink met *during* the walk, which
# -r skips and -R follows — and neither could be tested at all while the subject
# was a Windows binary.
mkdir -p sub/deep
printf 'foo\n'  > sub/s1
printf 'bar\n'  > sub/deep/s2
mkdir -p symdir
printf 'foo\n' > symdir/plain
ln -s ../sub  symdir/tosub
ln -s ../abc  symdir/tofile
ln -s /nowhere-at-all symdir/dangling

# A tree that contains a link back to its own root, which is an infinite tree to
# anything that follows links: `-R` must notice and say so, and `-r` must never
# reach the question. Ours did reach it, and did not terminate.
#
# The link points at `loopdir`, not at the fixture directory above it, so that
# the loop cases stay about the loop. Pointed one level higher it walked the
# entire fixture tree, which meant each of them also compared binary-file
# suppression and directory ordering — and would have needed re-marking every
# time a fixture was added anywhere in this file.
mkdir -p loopdir/inner
printf 'foo\n' > loopdir/inner/leaf
ln -s .. loopdir/inner/up

# Exactly one matching file under a directory, which is the case that separates
# "prefix because there are several files" from "prefix because -r was pointed
# at a directory". Ours counted the expansion and so got this one wrong.
mkdir -p onefile
printf 'foo\n' > onefile/only

# ------------------------------------------------------------------- cases ---
#
# One shell command line per case, `grep` standing for whichever grep is
# running. Blank lines and `#` lines are ignored; the marker table in the header
# says what a leading `~` or `!` means.
#
# `grep` is a *function* rather than a textual substitution, so that a case can
# put something before it (`GREP_COLORS=… grep …`) and so that a pattern
# containing the word `grep` is not rewritten.
#
# It goes through `env PATH=…` rather than naming the binary, because the
# diagnostic prefix is `argv[0]`: run as `/home/x/.cache/…/debug/grep` our side
# would print that whole path where GNU prints `grep`, and every stderr
# comparison below would fail for a reason that is the harness's doing. The
# directory holds one symlink and is the whole of `PATH` for that one process —
# safe here because grep runs no subprocess, so there is nothing else it could
# need to find.
grep() { env PATH="$bindir/$CAP_SIDE" grep "$@"; }

# Everything observable about one run, left in globals rather than printed,
# because the stderr *text* and the mere fact of a diagnostic are two different
# answers and `run_case` picks between them per case.
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
# introduces. stderr goes through the same `tr` for the same reason.
capture() {
    CAP_SIDE=$1
    local err=$DIFF_TMP/err
    CAP_BODY=$( { eval "$2" </dev/null 2>"$err"; printf '\001rc=%s' "$?"; } | tr '\0' '\002' )
    CAP_MSG=$(tr '\0' '\002' <"$err")
    CAP_LOUD=quiet
    [ -s "$err" ] && CAP_LOUD=loud
    rm -f "$err"
}

# The single string a case is judged on: stdout and status always, then either
# the diagnostic's text or only whether there was one.
capture_key() {
    if [ "$1" = text ]; then
        printf '%s\001msg=%s' "$CAP_BODY" "$CAP_MSG"
    else
        printf '%s\001%s' "$CAP_BODY" "$CAP_LOUD"
    fi
}

# Render a capture for a human: `|` for a newline, `@` for a NUL.
show() { printf '%s' "$1" | tr '\n\002\001' '|@ '; }

run_case() {
    local line=$1 mode=text expect_diff="" reason=""
    case $line in
        '~'*)
            mode=presence
            reason=${line#\~}
            reason=${reason%%|*}
            line=${line#*|}
            ;;
        '!'*)
            expect_diff=purpose
            reason=${line#!}
            reason=${reason%%|*}
            line=${line#*|}
            ;;
        '?'*)
            expect_diff=gap
            reason=${line#\?}
            reason=${reason%%|*}
            line=${line#*|}
            ;;
    esac

    local a b
    capture gnu  "$line"; a=$(capture_key "$mode")
    capture ours "$line"; b=$(capture_key "$mode")

    if [ "$a" = "$b" ]; then
        if [ -n "$expect_diff" ]; then
            xpass=$((xpass + 1))
            printf 'XPASS  %s\n     (expected to differ: %s)\n' "$line" "$reason"
        else
            pass=$((pass + 1))
        fi
        return
    fi
    case $expect_diff in
        purpose) xfail=$((xfail + 1)); return ;;
        gap)     xgap=$((xgap + 1));   return ;;
    esac
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
# GNU answers a missing operand with the usage summary and `Try 'grep --help'`;
# we answer `grep: missing PATTERN`. Ours names the actual fault and GNU's does
# not, but GNU's is the shape every other utility on the system uses, and a
# harness is not the place to relitigate it. Tracked with the rest of the
# getopt-diagnostic shape work in known-issues.md.
?our missing-operand diagnostic is a sentence, not GNU's usage summary (TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE)|grep

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
~glibc regcomp's wording, not grep's|grep 'a[' braces
~glibc regcomp's wording, not grep's|grep 'a\{2' braces
~glibc regcomp's wording, not grep's|grep 'a\{2,1\}' braces
~glibc regcomp's wording, not grep's|grep '\(a' braces
~glibc regcomp's wording, not grep's|grep 'a\)' braces
grep '*a' braces
grep '\w' mixed
grep '\W' words
grep '\s' words
grep '\S' words
# \< \> \b \B were three deliberate divergences until the ere/bre word-boundary
# operators landed (todo item 10). They are plain cases now, and the harness
# fails if either side stops agreeing about them again.
grep '\<foo' words
grep 'foo\>' words
grep '\bfoo' words
grep 'foo\b' words
grep '\Bfoo' words
grep '\<o' words
grep -E '\<foo' words
grep -E 'foo\>' words
grep -E '\bfoo' words
grep -E '\Bo' words

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
# expression rather than being REG_BADRPT. The *answer* agreed from the first
# run; what did not is that GNU also prints `grep: warning: * at start of
# expression` to stderr while still exiting 0. That warning is the only thing
# telling a user their pattern is not doing what they think, so it is a gap and
# not a difference of opinion.
?we accept a leading quantifier silently; GNU warns `* at start of expression` on stderr and still exits 0|grep -E '*a' braces
?we accept a leading quantifier silently; GNU warns `+ at start of expression` on stderr and still exits 0|grep -E '+a' braces
?we accept a leading quantifier silently; GNU warns `? at start of expression` on stderr and still exits 0|grep -E '?a' braces
grep -E 'a^*b' braces
?we accept a leading quantifier silently; GNU warns `* at start of expression` on stderr and still exits 0|grep -E '^*' abc
# RE_INVALID_INTERVAL_ORD: a `{` that does not open a well-formed interval is a
# literal brace.
grep -E 'a{b}' braces
grep -E 'a{' braces
grep -E '{b}' braces
grep -E 'a{,}' braces
# A well-formed-looking but wrong interval stays an error in both dialects.
~glibc regcomp's wording, not grep's|grep -E 'a{}' braces
~glibc regcomp's wording, not grep's|grep -E 'a{1,2,3}' braces
~glibc regcomp's wording, not grep's|grep -E 'a{2,1}' braces
~glibc regcomp's wording, not grep's|grep -E 'a{99999999}' braces
~glibc regcomp's wording, not grep's|grep -E 'a(' braces
~glibc regcomp's wording, not grep's|grep -E 'a)' braces
~glibc regcomp's wording, not grep's|grep -E 'a[' braces
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
# A long option's value may be the next argv entry, not just the text after an
# `=`. getopt_long accepts both and so must we; ours took only the `=` form
# until 2026-08-25, which rejected every one of these as a missing argument.
grep --regexp foo words
grep --regexp=foo words
grep --file pats words
grep --max-count 1 foo words
grep --group-separator XX -C 1 HIT ctx
?ours is `unknown option: --zzz`; GNU is `unrecognized option '--zzz'` plus the usage summary (TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE)|grep --zzz a abc
?ours names the offending value (`invalid max count: x`); GNU's message does not|grep -m x foo words

# --- recursion ---
#
# Every case here used to be a deliberate divergence, on the stated grounds
# that "the Windows build joins recursive paths with `\`". That was never a
# divergence in grep; it was the harness measuring the host it ran on, and it
# made the flag people actually use grep with into a blind spot. Inside WSL the
# separator is `/` on both sides, so the whole section is plain cases — and the
# section has grown, because -r is now testable at all.
grep -r foo sub
grep -rn foo sub
grep -rl foo sub
grep -rL foo sub
grep -rc foo sub
grep -rh foo sub
# `.` sweeps in `zsep`, which has NUL bytes in it, so this case differs for the
# binary reason at the bottom of the file — and it is also the one case whose
# output is long enough to show that we list a directory in sorted order where
# GNU lists it in readdir order (design-decisions.md §380). Both are choices;
# neither is a defect, which is why this is `!` and not `?`.
!we never suppress binary output, and we list a directory sorted where GNU uses readdir order|grep -r foo .
grep -r foo sub/deep
grep foo sub
grep -r foo sub/s1
grep -r foo /nonexistent
?-d is not implemented; GNU's `-d recurse` is -r and `-d skip` ignores directories silently|grep -d recurse foo sub
?-d is not implemented; GNU's `-d recurse` is -r and `-d skip` ignores directories silently|grep -d skip foo sub
grep -r foo sub empty
?--include is not implemented|grep -r --include='s1' foo .
?--exclude is not implemented|grep -r --exclude='s1' foo .
?--exclude-dir is not implemented|grep -r --exclude-dir='deep' bar .

# -r and -R differ over exactly one thing: a symlink met during the walk. -r
# skips it, -R follows it. A symlink named on the command line is followed by
# both. None of this could be tested while the subject was a Windows binary,
# and the first run inside WSL found that we did not make the distinction at
# all — we followed during the walk, so `symdir/tosub/s1` was reported that GNU
# does not report, and `-r` on a tree with a loop in it did not terminate.
grep -r foo symdir
grep -R foo symdir
grep -r foo symdir/tosub
grep -r a symdir/tofile
grep -rl foo symdir
grep -Rl foo symdir
grep -r foo symdir/dangling
grep -R foo symdir/dangling
grep -Rs foo symdir
grep -r foo loopdir
grep -R foo loopdir
grep -Rl foo loopdir

# The loop warning has three separate properties, and we had all three wrong at
# once. It is silenced by -s; it does *not* raise the exit status, so a -R that
# found nothing in a looping tree still exits 1 and not 2; and it names the
# link, not what the link resolves to.
grep -Rs foo loopdir
grep -R zzz loopdir
grep -Rc foo loopdir

# -q stops at the first match, and the walk has to be *streaming* for that to
# be visible: what it means is that the files after the match are never looked
# at, so the diagnostics they would have produced never appear. Expanding the
# tree into a list first produced the warning either way, because the expansion
# had already finished before the first file was read. The pair below is the
# whole test: same tree, same flags, and the warning appears only in the one
# that had to walk all of it.
grep -Rq foo loopdir
grep -Rq zzz loopdir
grep -rq foo sub

# -q asks one question, and an error reading some other file does not unanswer
# it: POSIX says a selected line means status 0 "even if an error was
# detected". The three below separate that from the ordinary rule — the first
# reports the missing file and still exits 0, the second exits 0 without ever
# reaching it, and the third has no match to outrank the error and so exits 2.
grep -q foo nonexistent words
grep -q foo words nonexistent
grep -q zzz nonexistent words

# The filename prefix under -r is not "more than one file": it is "the operand
# was a directory". These two say so, and they disagreed before the walk was
# rewritten — the second printed a bare `foo`.
grep -r foo onefile
grep -r a symdir/tofile
grep -rH foo onefile
grep -rh foo onefile

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

# --- context, which nothing here had ever exercised ---
#
# `ctx` has its two hits six lines apart so that -C 1 leaves a gap and -C 3
# closes it: the `--` separator between non-adjacent groups, and its absence
# when the groups touch, is the part of this that is easy to get wrong.
grep -A 1 HIT ctx
grep -B 1 HIT ctx
grep -C 1 HIT ctx
grep -C 3 HIT ctx
grep -A 99 HIT ctx
grep -B 99 HIT ctx
grep -C 0 HIT ctx
grep -A 0 HIT ctx
grep -B 0 HIT ctx
grep -nC 1 HIT ctx
grep -A 1 HIT ctx ctx
grep -cC 1 HIT ctx
grep -vC 1 HIT ctx
grep -A1 HIT ctx
grep -A 1 -m 1 HIT ctx
grep --group-separator=XX -C 1 HIT ctx
grep --no-group-separator -C 1 HIT ctx

# The long spellings, and the `=`-less forms that take the next argv entry.
grep --after-context=1 HIT ctx
grep --before-context=1 HIT ctx
grep --context=1 HIT ctx
grep --context 1 HIT ctx
# `--group-separator=` with an empty value is a *blank line* between groups,
# which is a different answer from --no-group-separator and is the reason the
# separator cannot be modelled as an Option<Vec<u8>>.
grep --group-separator= -C 1 HIT ctx

# The digit shorthand. `-1` is `-C 1`; the digits of `-12` accumulate rather
# than the last one winning; and a non-digit ends the run, so `-1n` is
# `-C 1 -n` and not a context length of one followed by nothing.
grep -1 HIT ctx
grep -3 HIT ctx
grep -12 HIT ctx
grep -1n HIT ctx
grep -n1 HIT ctx

# -A and -B each keep their own value and fall back to -C's only if unset, so
# these two are the same command however they are ordered. Written as a plain
# `usize` field the later flag would clobber the earlier one and they would not
# be.
grep -A 3 -C 1 -n HIT ctx
grep -C 1 -A 3 -n HIT ctx
grep -B 3 -C 1 -n HIT ctx

# A bad context length is grep's own diagnostic, not the family's `invalid
# number`, and it exits 2. `-A -1` is not the digit shorthand: -A demands an
# argument, so `-1` is consumed as one and then refused.
grep -A x HIT ctx
grep -B x HIT ctx
grep -C x HIT ctx
grep -A -1 HIT ctx
grep --context=x HIT ctx

# The separator between *files*: a file's first group is never adjacent to the
# previous file's last, even when it starts at line 1.
grep -A 1 HIT ctxtop ctx
grep -A 1 HIT ctx ctxtop
grep -C 1 -H HIT ctx ctxtop
# An empty operand between two matching ones prints nothing and no separator.
grep -C 1 HIT ctx empty ctx

# -m satisfied while trailing context is still owed: the lines that follow
# print as context (`-`), not as matches, even though they match.
grep -n -m 1 -A 2 HIT run3
grep -n -m 2 -A 2 HIT run3
grep -n -m 1 -B 2 HIT run3

# -o prints the matching part, and a context line has none — so it prints
# nothing at all, prefix included. The grouping still applies to the lines it
# printed nothing for, which is why -oA1 gets a separator and -oC2 does not.
grep -oA 1 HIT ctx
grep -oC 2 HIT ctx
grep -oC 3 HIT ctx
grep -onC 1 HIT ctx

# Context is ignored outright by the options that answer a question about the
# file rather than about its lines.
grep -lC 1 HIT ctx
grep -LC 1 zzz ctx
grep -qC 1 HIT ctx

# --- byte offsets and the other output decorations ---
grep -b a abc
grep -bn a abc
grep -bo foo words
grep -bH a abc
# Under `-z` the NUL separators are bytes of the file like any other, so they
# count towards the offset. (Without `-z` GNU calls this fixture binary and
# prints no lines at all — see the `!` case above.)
grep -bz foo zsep
grep --byte-offset foo words
# The offset is of the *line*, not of the match — except under -o, where each
# match carries its own. Context lines get one too, with a `-` after it.
grep -bC 1 HIT ctx
grep -bnoH foo words
grep -bv a abc

# -T pads the numeric fields to a width taken from the file's size *before a
# line is read*, and ends the prefix with a tab. Which is why `w99` is here: at
# 99 bytes the width is 2 without -n and 3 with it, because a 99-byte file can
# hold 100 lines. Every other fixture is small enough for both to be 1, so none
# of them can tell a correct width from a constant.
grep -T a abc
grep -Tn a abc
grep -T HIT w99
grep -Tn HIT w99
grep -Tb HIT w99
grep -Tbn HIT w99
grep -TnH HIT w99
grep -TnHZ HIT w99
grep -TH HIT w99
grep -THZ HIT w99
grep -Tbo foo words
grep -TbA 1 HIT ctx
grep -TnC 1 HIT ctx
grep --initial-tab --line-number HIT w99
# -T with nothing to pad prints no tab at all: the tab follows the last field,
# and here there is no field.
grep -T HIT ctx
# The options that answer a question about the file print no line prefix, and
# so take no tab either.
grep -Tc HIT w99
grep -THc HIT w99
grep -Tl HIT w99
# A stream has no size to take a width from, so GNU falls back to the largest a
# signed off_t can hold — nineteen columns.
cat w99 | grep -Tn HIT
cat w99 | grep -Tb HIT
# Redirected stdin *is* a regular file, so it gets the file's own width rather
# than the stream fallback.
grep -Tn HIT < w99
grep -Tb HIT < w99

# --- colour, which is escape sequences on stdout and therefore comparable ---
#
# The `;` in GREP_COLORS is quoted because an unquoted one ends the command:
# the first draft of this case ran `GREP_COLORS=mt=01` and then `32 grep …`,
# which failed identically on both sides and was recorded as a pass.
grep --color=never foo words
grep --color=always foo words
grep --color=always -o foo words
grep --color=always -n foo words
grep --color=always -i alpha mixed
grep --color=auto foo words
GREP_COLORS='mt=01;32' grep --color=always foo words
# Every prefix field has a colour of its own, and so does each separator
# between them — including the `--` between context groups.
grep --color=always -nbH foo words
grep --color=always -nC 1 HIT ctx
grep --color=always -nbHTC 1 HIT w99
grep --color=always -Z -nH foo words
grep --color=always foo words abc
grep --color=always -Hc foo words
grep --color=always -l foo words
grep --color=always -lZ foo words
grep --color=always -L zzz words
grep --color=always -q foo words
grep --color=always -z foo zsep
# `-v` selects the lines that did not match, so there is nothing on them to
# colour — but a *context* line under -v may match, and gets `mc`.
grep --color=always -v foo words
grep --color=always -vC 1 foo words
# The empty-match rule of -o survives colouring: `o*` matches nothing at most
# positions, and nothing is what gets printed for them.
grep --color=always -o 'o*' words
grep --color=always -i café accent
# `--colour` is the same option, and bare `--color` means `auto`, which off a
# terminal means never.
grep --colour=always foo words
grep --color foo words
# GREP_COLORS: `mt` sets both `ms` and `mc`, last assignment wins, an unknown
# key or an unparsable value is ignored in silence, `ne` drops the `\e[K` that
# otherwise follows every escape, and `rv` swaps the selected- and context-line
# colours when -v is in effect.
GREP_COLORS='ms=01;36:mt=01;33' grep --color=always foo words
GREP_COLORS='mt=01;33:ms=01;36' grep --color=always foo words
GREP_COLORS='fn=35:ln=33:bn=34:se=36' grep --color=always -nbH foo words
GREP_COLORS='sl=33' grep --color=always foo words
GREP_COLORS='cx=90' grep --color=always -C 1 HIT ctx
GREP_COLORS='ne' grep --color=always foo words
GREP_COLORS='rv:sl=33:cx=34' grep --color=always -v foo words
GREP_COLORS='rv:sl=33:cx=34' grep --color=always foo words
GREP_COLORS='zz=1' grep --color=always foo words
GREP_COLORS='ms=zz' grep --color=always foo words
GREP_COLORS='' grep --color=always foo words
# A value capability with no `=` is *ignored*, not read as "set it to empty":
# `ms` leaves the default highlight alone where `ms=` removes it. The two
# booleans are the exception — they fire with or without a value.
GREP_COLORS='ms' grep --color=always foo words
GREP_COLORS='ms=' grep --color=always foo words
GREP_COLORS='ne=1' grep --color=always -n foo words
GREP_COLORS='rv=1:sl=33:cx=34' grep --color=always -v foo words
GREP_COLORS='sl=33::cx=34' grep --color=always -C 1 HIT ctx
# `ms=` and `ms=:sl=33` differ in *shape*, not by one escape: with no match
# colour there is no per-match pass at all, so the whole line is written as one
# `sl` run — start, text, end — where a highlighted line's `sl` runs are opened
# before each match and never closed.
GREP_COLORS='ms=:sl=33' grep --color=always foo words
GREP_COLORS='sl=33' grep --color=always -o foo words
GREP_COLORS='cx=34' grep --color=always -o foo words
# `sl`'s trailing run: `foo bar` has text after its last match, `baz foo foo`
# ends on one, and an empty pattern has no match to open a run at all.
GREP_COLORS='sl=33' grep --color=always '' words
GREP_COLORS='sl=33' grep --color=always 'o*' words
GREP_COLORS='sl=33' grep --color=always -x qux words
# The carriage return of a CRLF line is terminator, not text.
GREP_COLORS='sl=33' grep --color=always foo crlf
GREP_COLORS='sl=33' grep --color=always -n foo crlf
grep --color=always foo crlf
# The `--` between groups is `se`, and so is a `--group-separator` of one's own;
# the newline after it is not.
grep --color=always -A 1 HIT ctx
GREP_COLORS='se=45' grep --color=always -A 1 HIT ctx
GREP_COLORS='se=45' grep --color=always -A 1 --group-separator=XX HIT ctx
GREP_COLORS='se=' grep --color=always -nA 1 HIT ctx
GREP_COLORS='ne:sl=33' grep --color=always -nbH foo words
# The file-name-only outputs colour the name and nothing else — and under -Z the
# NUL after it stays outside the escape, as it does in a line prefix.
grep --color=always -HcZ foo words
GREP_COLORS='fn=45:se=46' grep --color=always -Hc foo words
GREP_COLORS='fn=' grep --color=always -Hc foo words
grep --color=always -rn foo sub
grep --color=always -rlZ foo sub
GREP_COLORS='sl=33' grep --color=always -TnbHZ HIT w99
# The deprecated spelling still works, and says so on stderr.
GREP_COLOR='01;35' grep --color=always foo words
# An empty one is not a setting: no warning, and the default highlight stands.
GREP_COLOR='' grep --color=always foo words
# GREP_COLOR loses to a GREP_COLORS that names the same capability, and neither
# is read at all when colour is off — the deprecation warning included.
GREP_COLOR='01;35' GREP_COLORS='ms=01;36' grep --color=always foo words
GREP_COLOR='01;35' grep foo words
GREP_COLOR='01;35' grep --color=never foo words
# `--color=` with a word that is none of the three is not an error: GNU sets
# `show_help` and prints the whole usage text on *stdout*, exiting 0. Matching
# that would mean printing a help text advertising options we do not have.
!--color=WORD with an unrecognised WORD prints GNU'\''s full usage summary and exits 0; ours has no --help text to print|grep --color=bogus foo words

# --- a character that is not one byte, now that the locale is C.UTF-8 ---
grep -i cafe accent
grep -i café accent
grep -i CAFÉ accent
grep café accent
grep 'caf.' accent
grep -o '.' accent
grep '[[:alpha:]]' accent
grep -c '[[:alpha:]]' accent
grep -w café accent
grep -x café accent
grep -o 'caf.' accent
grep -E 'caf.' accent

# --- a closed descriptor, which is what `coreutils::stdfd` exists for ---
#
# `close_stdout` is called and still we exit 0 with nothing said: the write to
# the closed descriptor fails and the failure is dropped somewhere between the
# match loop and the exit status. GNU exits 2 and says `grep: write error: Bad
# file descriptor`. The two cases that write nothing pass, which locates it in
# the writing rather than in the setup.
grep a abc >&-
grep -c a abc >&-
grep -q a abc >&-
grep z abc >&-
grep -l a abc >&-
grep -o a abc >&-
grep -r foo sub >&-
grep a /nonexistent 2>&-
CASES

# --- the cases that only mean something as an ordinary user -------------------
#
# `chmod 000` does not stop root, so as root these would pass by reading the
# file successfully on both sides — a green that certifies nothing. They are
# skipped instead, loudly, rather than quietly turned into their own opposite.
if [ "$(id -u)" != 0 ]; then
    printf 'secret\n' > noread
    chmod 000 noread
    mkdir -p nolist/inner
    printf 'foo\n' > nolist/inner/f
    chmod 000 nolist
    run_case 'grep a noread'
    run_case 'grep -s a noread'
    run_case 'grep a noread abc'
    # Two separate faults, both found on the first run inside WSL and both
    # fixed by giving the recursive walk the error path the file case already
    # had: an unopenable directory left the status at 1, and -s did not cover
    # it. The plain-file cases above passed throughout, which is what said it
    # was the walk and not the shared path.
    run_case 'grep -r foo nolist'
    run_case 'grep -rs foo nolist'
    run_case 'grep -r foo nolist abc'
    run_case 'grep -R foo nolist'
    # Restore, so `diff_cleanup` can descend even if the chmod -R there ever
    # stops covering a case.
    chmod 755 nolist
else
    printf 'note: running as root inside WSL; the unreadable-file cases are skipped\n'
    printf '      (root reads them anyway, so they would agree for the wrong reason)\n'
fi

printf '\n%d passed, %d differed, %d on purpose, %d not built yet, %d unexpectedly agreed\n' \
    "$pass" "$fail" "$xfail" "$xgap" "$xpass"
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
