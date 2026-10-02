#!/usr/bin/env bash
# fmt-diff.sh — compare our `fmt` against GNU coreutils 9.4's, inside WSL.
#
# ## What this is checking
#
# Each case is one shell command line; standard output and standard error go
# to one file, compared byte for byte with the exit status after them.
#
# `fmt` chooses its line breaks by minimising a cost over the whole paragraph,
# so a difference anywhere in the cost function shows as a different break
# somewhere in a long enough text. The cases therefore lean on real prose
# (coreutils' own README, NEWS and manual, and the comments of its sources,
# which also exercise `-p`), at many widths and goals and in every mode, with
# a checksum where the output would be long. Around that: the paragraph rules
# (indents under `-c`, `-t` and `-s`, prefixes matched in part or with the
# wrong indent), white space (tabs on input switch tabs on in the output),
# the limits of 1000 words and 5000 bytes (a paragraph printed in parts, a
# word printed raw), odd bytes (NUL, CR, form feed, invalid UTF-8), and
# every message on the way.
#
# Last, 300 random texts formatted with random options: words with every kind
# of ending the cost function cares about, indents that change mid-paragraph,
# prefixes present and absent, tabs, CRLF, blank lines of white space, long
# words and longer paragraphs. They come from a seeded generator, so a
# failure reruns, and each output is headed by its seed and options, so a
# failure names the input that caused it.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `--version` names SlateOS.
set -u

DIFF_PROG='fmt'
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

for v in $(env | sed -n 's/^\(LC_[A-Z_]*\)=.*/\1/p'); do unset "$v"; done
export LC_ALL=C.UTF-8

pass=0; fail=0; xfail=0; xpass=0

GNU_PATH="$bindir/gnu:$PATH"
OURS_PATH="$bindir/ours:$PATH"
out_a="$DIFF_TMP/out-gnu"
out_b="$DIFF_TMP/out-ours"

mkdir -p "$DIFF_TMP/tree" || exit 1
cd "$DIFF_TMP/tree" || exit 1

# ---------------------------------------------------------------- fixtures ---

# Real prose, from the pinned coreutils tree the reference was built from.
src=$gnu_dir/..
cp "$src/README" readme
cp "$src/NEWS" news
cp "$src/src/fmt.c" fmt.c
cp "$src/src/ls.c" ls.c
# The manual, without its Texinfo commands' lines, as a long run of prose.
grep -v '^@' "$src/doc/coreutils.texi" > manual

printf 'The quick brown fox jumps over the lazy dog.  It barks.  Then it sleeps\nsoundly, (as foxes do) until morning; at dawn, it wakes!  Mr. Fox\nruns away.  "Why?" asks the dog.  Nobody knows.\n' > fox
printf 'one\ntwo\nthree\n\n\nfour five\n   \nsix\n' > blanks
printf 'a b c' > noeol
printf '  ' > spaces
printf '' > empty
printf '\tindented with a tab\n\tand another line\n\n        eight spaces here\n        and here\n' > tabs
printf 'word\tthen a tab\tand   three spaces\n' > midtabs
printf 'first line\n   second line\n   third line\n\n  alpha\n    beta\n    gamma\n' > indents
printf 'one\r\ntwo\r\nthree\r\n' > crlf
printf 'form\ffeed and\vvertical tab\n' > ffvt
printf 'a\0b c\0 d.\0 e\n' > nuls
printf 'caf\303\251 na\303\257ve r\303\251sum\303\251 \342\200\224 \346\227\245\346\234\254\350\252\236 text\n' > utf8
printf 'bad \377\376 bytes \200 here\n' > invalid
printf '# one\n# two\n#three\n  # four\n# \n#\nnone\n# five\n' > hashes
printf '> quoted text\n>  more quoted\n>> nested\nplain\n> back\n' > quotes
printf '%s\n' '/* A comment' ' * that goes on' ' * and on.' ' */' 'code();' > ccomment
# A paragraph of 1200 words: past MAXWORDS, so it is printed in parts.
for i in $(seq 1 1200); do printf 'w%d ' "$i"; done > manywords; echo >> manywords
# 300 words of 30 bytes: past MAXCHARS long before MAXWORDS.
for i in $(seq 1 300); do printf 'abcdefghijklmnopqrstuvwxyz%04d ' "$i"; done > manychars; echo >> manychars
# One word longer than MAXCHARS, alone and among others.
head -c 12000 /dev/zero | tr '\0' 'x' > longword; echo >> longword
{ printf 'start '; head -c 6000 /dev/zero | tr '\0' 'y'; printf ' end\n'; } > longmid
# Crown and tagged paragraphs long enough to be printed in parts.
{ printf 'Head line\n'; for i in $(seq 1 1100); do printf '    t%d\n' "$i"; done; } > longcrown
mkdir dir

# The random texts: `rN` formatted with the options in `rN.opts`, shell-quoted.
cat > gen.py <<'PY'
import random, shlex, sys

WORDS = [
    "a", "an", "the", "of", "to", "in", "is", "it", "fox", "dog", "quick",
    "brown", "lazy", "jumps", "over", "paragraph", "formatting", "width",
    "goal", "sentence", "Mr.", "Dr.", "e.g.", "i.e.", "etc.", "U.S.A.",
    "end.", "why?", "now!", "(paren", "close)", "[bracket]", '"quoted"',
    "'single'", "`tick'", "comma,", "semi;", "colon:", "dash-word", "x",
    "supercalifragilisticexpialidocious", "antidisestablishmentarianism",
    "café", "naïve", "日本語", "done.)", 'said."',
    "(a)", "1.", "2)", "--", "...", "?!", "!", ".", ")", "(",
]
PREFIXES = ["", "#", "# ", "  #", ">", "> ", " * ", "//", "   "]

def text(rng):
    out = bytearray()
    prefix = rng.choice(PREFIXES[1:]) if rng.random() < 0.25 else ""
    for _ in range(rng.randint(1, 6)):
        nwords = rng.randint(900, 1300) if rng.random() < 0.05 else rng.randint(1, 120)
        first = rng.choice([0, 0, 0, 2, 4, 8])
        other = rng.choice([first, first, 0, 2, 3, 4])
        tabs = rng.random() < 0.1
        line, lines = bytearray(), []
        for _ in range(nwords):
            w = rng.choice(WORDS).encode("utf-8")
            r = rng.random()
            if r < 0.01:
                w = b"y" * rng.randint(50, 6000)
            elif r < 0.015:
                w = w + bytes([rng.choice([0, 13, 12, 11, 0xff, 0x80])]) + w
            if line:
                sep = rng.choice([b" ", b" ", b" ", b"  ", b"   "])
                if tabs and rng.random() < 0.1:
                    sep = b"\t"
                line += sep
            line += w
            if len(line) > rng.randint(10, 90):
                lines.append(line)
                line = bytearray()
        if line:
            lines.append(line)
        for n, l in enumerate(lines):
            ind = first if n == 0 else other
            if rng.random() < 0.03:
                ind = rng.randint(0, 6)
            lead = b"\t" * (ind // 8) + b" " * (ind % 8) if tabs else b" " * ind
            pre = prefix.encode()
            if prefix and rng.random() < 0.05:
                pre = b""
            out += pre + lead + bytes(l) if rng.random() < 0.5 else lead + pre + bytes(l)
            if rng.random() < 0.03:
                out += b"   "
            out += b"\r\n" if rng.random() < 0.02 else b"\n"
        for _ in range(rng.choice([0, 1, 1, 1, 2])):
            out += (prefix.encode() if prefix and rng.random() < 0.5 else b"")
            out += rng.choice([b"", b"  ", b"\t"]) + b"\n"
    if rng.random() < 0.2 and out.endswith(b"\n"):
        out = out[:-1]
    return bytes(out), prefix

def options(rng, prefix):
    opts = []
    r = rng.random()
    if r < 0.3:
        opts += ["-w", str(rng.randint(1, 140))]
    elif r < 0.4:
        opts += ["-" + str(rng.randint(1, 140))]
    elif r < 0.45:
        opts += ["-w", str(rng.choice([1, 2, 3, 2500]))]
    if rng.random() < 0.2:
        w = int(opts[1]) if opts[:1] == ["-w"] else (int(opts[0][1:]) if opts else 75)
        opts += ["-g", str(rng.randint(0, w))]
    for flag in ["-c", "-t", "-s", "-u"]:
        if rng.random() < 0.2:
            opts.append(flag)
    if prefix and rng.random() < 0.8:
        opts += ["-p", rng.choice([prefix, prefix.strip(), prefix + " "])]
    elif rng.random() < 0.05:
        opts += ["-p", rng.choice(PREFIXES)]
    return opts

for seed in range(int(sys.argv[1]), int(sys.argv[2]) + 1):
    rng = random.Random(seed)
    data, prefix = text(rng)
    with open(f"r{seed}", "wb") as f:
        f.write(data)
    with open(f"r{seed}.opts", "w", encoding="utf-8") as f:
        f.write(" ".join(shlex.quote(o) for o in options(rng, prefix)) + "\n")
PY
python3 gen.py 1 300 || exit 1
# One random text, headed by its seed and options.
random_case() {
    printf '== r%s: %s\n' "$1" "$(cat "r$1.opts")"
    eval "fmt $(cat "r$1.opts") r$1"
    echo "rc=$?"
}

fmt() { PATH=$FMT_PATH command fmt "$@"; }

# ------------------------------------------------------------------ driver ---

run_case() {
    line=$1
    expect_diff=0
    reason=""
    case $line in
        '!'*)
            expect_diff=1
            reason=${line#!}
            reason=${reason%%|*}
            line=${line#*|}
            ;;
    esac
    ( FMT_PATH=$GNU_PATH; diff_run eval "$line" >"$out_a" 2>&1; printf 'rc=%s' "$?" >>"$out_a" )
    ( FMT_PATH=$OURS_PATH; diff_run eval "$line" >"$out_b" 2>&1; printf 'rc=%s' "$?" >>"$out_b" )
    if cmp -s "$out_a" "$out_b"; then
        if [ "$expect_diff" = 1 ]; then
            xpass=$((xpass + 1))
            printf 'XPASS  %s\n     (expected to differ: %s)\n' "$line" "$reason"
        else
            pass=$((pass + 1))
            [ -n "${VERBOSE:-}" ] && printf 'OK     %s\n' "$line"
        fi
        return
    fi
    if [ "$expect_diff" = 1 ]; then
        xfail=$((xfail + 1))
        return
    fi
    fail=$((fail + 1))
    printf 'FAIL   %s\n' "$line"
    printf '  ---- unified (gnu < , ours >) ----\n'
    diff <(cat -A "$out_a") <(cat -A "$out_b") \
        | sed 's/^/        /' | sed -n '1,24p'
}

while IFS= read -r case_line; do
    case $case_line in ''|'#'*) continue ;; esac
    run_case "$case_line"
done <<'CASES'
# --- prose, at every width ---
fmt fox
fmt readme
fmt news | md5sum
fmt manual | md5sum
fmt -w 20 fox
fmt -w 30 readme
fmt -w 40 news | md5sum
fmt -w 60 manual | md5sum
fmt -w 72 readme
fmt -w 74 fox
fmt -w 76 fox
fmt -w 100 news | md5sum
fmt -w 2500 readme
fmt -w 2500 manual | md5sum
fmt -40 readme
fmt -1 fox
fmt -0 fox
fmt -w 0 fox
fmt -w 1 fox
fmt -w 2 fox
fmt -w 5 fox
fmt -w 9 fox

# --- the goal ---
fmt -g 40 readme
fmt -g 60 -w 75 readme
fmt -g 75 news | md5sum
fmt -g 0 fox
fmt -g 1 -w 30 fox
fmt -g 30 -w 30 fox
fmt -w 50 -g 10 readme
fmt -g 20 manual | md5sum

# --- modes ---
fmt -u fox
fmt -u readme
fmt -u -w 50 news | md5sum
fmt -s fox
fmt -s -w 30 readme
fmt -s -w 20 news | md5sum
fmt -c indents
fmt -c readme
fmt -c -w 40 manual | md5sum
fmt -t indents
fmt -t readme
fmt -t -w 40 manual | md5sum
fmt -t blanks
fmt -c -t indents
fmt -s -c indents
fmt -s -t indents
fmt -u -s fox
fmt -cu -w 30 fox
fmt indents

# --- paragraphs and blank lines ---
fmt blanks
fmt noeol
fmt spaces | od -c
fmt empty
fmt -w 3 noeol
printf '\n\n\n' | fmt | od -c
printf 'a\n\n' | fmt | od -c
printf '   a   \n   b   \n' | fmt | od -c
printf 'a  \nb\n' | fmt | od -c
printf 'end.\nnext\n' | fmt -u
printf 'end.)\nnext\n' | fmt -u
printf 'end.") x  y\n' | fmt -u
printf 'a.  b. c.   d.\te.\n' | fmt -u

# --- prefixes ---
fmt -p '#' hashes
fmt -p '# ' hashes
fmt -p '  #' hashes
fmt -p '#' -w 10 hashes
fmt -p '>' quotes
fmt -p '> ' quotes
fmt -p '>>' quotes
fmt -p '' quotes
fmt -p '   ' indents
fmt -p ' ' indents
fmt -p ' *' ccomment
fmt -p ' * ' ccomment
fmt -p '/*' ccomment
fmt -p ' *' -w 50 fmt.c | md5sum
fmt -p ' * ' -w 60 ls.c | md5sum
fmt -p '   ' -w 60 ls.c | md5sum
fmt -p '#' -t hashes
fmt -p '#' -c hashes
fmt -p '#' -s hashes
fmt -p '#' -u hashes
fmt -p 'x' -p '#' hashes
fmt --prefix='> ' quotes
printf '#a\n#\n#b\n' | fmt -p '#' | od -c
printf '#a\n  #b\n#c\n' | fmt -p '#'
printf '\t#a\n\t#b\n' | fmt -p '#'
printf '##a\n#b\n' | fmt -p '##'
printf 'ab' | fmt -p 'abc' | od -c
printf 'ab\n' | fmt -p 'abc' | od -c
printf '  ab' | fmt -p 'ab' | od -c
printf '  ' | fmt -p 'ab' | od -c

# --- white space and tabs ---
fmt tabs | od -c
fmt midtabs | od -c
fmt -w 20 tabs | od -c
fmt -u midtabs | od -c
fmt -c tabs | od -c
printf 'a\tb\n' | fmt | od -c
printf '\t\ta b\n\t\tc d\n' | fmt -w 30 | od -c
printf '   \ta b\n' | fmt | od -c
printf 'x  y\n\tz\n' | fmt | od -c
fmt tabs fox | od -c

# --- the limits: 1000 words, 5000 bytes ---
fmt manywords | md5sum
fmt -w 30 manywords | md5sum
fmt -u manywords | md5sum
fmt manychars | md5sum
fmt -w 200 manychars | md5sum
fmt longword | md5sum
fmt -w 20 longword | md5sum
fmt longmid | md5sum
sed 's/^/   /' longword | fmt | md5sum
sed 's/^/# /' longword | fmt -p '#' | md5sum
fmt -c longcrown | md5sum
fmt -t longcrown | md5sum
fmt longcrown | md5sum
sed 's/^/    /' manywords | fmt -w 50 | md5sum
cat manywords manywords | fmt -c | md5sum
fmt -s manywords | md5sum

# --- odd bytes ---
fmt crlf | od -c
fmt -w 5 crlf | od -c
fmt ffvt | od -c
fmt nuls | od -c
fmt -w 3 nuls | od -c
fmt utf8
fmt -w 15 utf8
fmt invalid | od -c
fmt -w 8 invalid | od -c
printf 'a\0\nb\n' | fmt | od -c
printf '(a b) [c d] "e" `f` g\n' | fmt -w 6

# --- files ---
fmt fox readme
fmt fox - < readme
fmt - - < fox
fmt - < fox
cat fox | fmt - -
fmt nonexistent
fmt fox nonexistent readme
fmt dir
fmt fox dir fox
fmt <&-
fmt - <&-
fmt fox <&-
fmt fox - <&-
fmt fox >&-
fmt news >&-
fmt -- -w
fmt ''
fmt fox >/dev/full
fmt fox nonexistent >/dev/full
fmt news >/dev/full
fmt news | head -2

# --- the command line ---
fmt -Z
fmt --bogus
fmt --w
fmt --w=30 fox
fmt --c fox
fmt --wid=30 --go=20 fox
fmt -w
fmt -p
fmt -g
fmt --prefix
fmt -w x fox
fmt -w '' fox
fmt -w -5 fox
fmt -w ' 30' fox
fmt -w 30k fox
fmt -w 2501 fox
fmt -w 2500 -g 2501 fox
fmt -w 99999999999999 fox
fmt -w 99999999999999999999999 fox
fmt -g x fox
fmt -g 80 fox
fmt -g 75 fox
fmt -g 5 -w 3 fox
fmt -w x -Z fox
fmt -w x -g y fox
fmt -c7 fox
fmt -c -72 fox
fmt -w5 -7 fox
fmt -72x fox
fmt -7 -8 fox
fmt -- -7
fmt -30 -w 20 fox
fmt fox -w 20
POSIXLY_CORRECT=1 fmt fox -w 20

# --- random texts, random options ---
for i in $(seq 1 100); do random_case "$i"; done
for i in $(seq 101 200); do random_case "$i"; done
for i in $(seq 201 300); do random_case "$i"; done

# --- deliberately different ---
!--help text is ours|fmt --help
!--version text is ours|fmt --version
CASES

total=$((pass + fail + xfail + xpass))
printf '%d case(s): %d passed, %d differed, %d differ on purpose, %d unexpectedly agreed\n' \
    "$total" "$pass" "$fail" "$xfail" "$xpass"
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
