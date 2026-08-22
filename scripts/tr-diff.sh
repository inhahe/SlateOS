#!/usr/bin/env bash
# Differential test: our tr against GNU tr.
#
# `tr` is a byte filter with no file operands at all, so every case here feeds
# stdin and compares stdout byte for byte with `od -An -c`. It has to be that
# way: most of what this implementation had to get right is invisible to a
# whitespace-trimming comparison — whether a deleted newline leaves the line
# joined, whether squeezing crosses a read boundary, whether SET2 pads with its
# last byte or stops, and whether a NUL survives the translation table.
#
# ## Why the reference is glibc, and only glibc
#
# The host's `tr` is MSYS2's — a Cygwin derivative linking `msys-2.0.dll`
# rather than glibc, whose `getopt` words every option diagnostic differently
# (`unknown option -- x` against `invalid option -- 'x'`). A harness pointed at
# it would certify sentences no GNU/Linux system prints. See `known-issues.md`
# → `TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE`, and the identical
# note at the top of `cut-diff.sh`, `head-diff.sh` and `wc-diff.sh`.
#
# ## Why the locale barely matters here
#
# GNU `tr` is byte-oriented — it has no multibyte mode, and `[:alpha:]` was
# measured to expand to the same 52 ASCII bytes under `C` and under `C.UTF-8`.
# So `C.UTF-8` throughout, which since §351 is also the setting the quote marks
# agree in: ours are U+2018/U+2019 in every locale and GNU's are those under any
# UTF-8 one. The diagnostics used to be referenced under `LC_ALL=C` because ours
# stayed ASCII (`open-questions.md` → B-Q2, since answered); `C` is now the
# setting in which the reference would be wrong.
set -u

# Our tr is a native Windows binary, so MSYS would rewrite an argument that
# looks like a path — and a `tr` SET is full of things that look like paths.
export MSYS2_ARG_CONV_EXCL='*'

# Built here, from the package named, rather than picked up out of `target/`.
# A harness that only *runs* that path measures whatever was written there
# last, which need not be current and need not even be this crate — see
# `scripts/diff-subject.sh`.
. "$(dirname "$0")/diff-subject.sh"
OURS=$(subject_binary coreutils tr "${OURS:-}") || exit 1
GNU=${GNU:-"wsl -e env LC_ALL=C.UTF-8 tr"}
export LC_ALL=${LC_ALL:-C.UTF-8}

pass=0; fail=0; xfail=0; xpass=0

fixtures=$(mktemp -d)
trap 'rm -rf "$fixtures"' EXIT
cd "$fixtures" >/dev/null || exit 1
OURS_ABS=$OURS
case $OURS in /*|[A-Za-z]:*) ;; *) OURS_ABS="$OLDPWD/$OURS" ;; esac

# `tr` takes no file operands, so the cwd never matters to it — but the
# reference still has to be *reachable*, and a `wsl` that is not installed
# fails silently enough to look like agreement on every case at once.
if [ "$(printf 'probe\n' | $GNU a-z A-Z 2>/dev/null)" = "PROBE" ]; then
  HAVE_GNU=yes
else
  HAVE_GNU=no
  echo "tr-diff: glibc tr not reachable (tried: $GNU); skipping"
fi

compare() {
  local o_out g_out o_err g_err o_rc g_rc stdin=$1 ref=$2; shift 2
  o_err=$(mktemp); g_err=$(mktemp)
  # stdout through a file, not a pipe: in `x=$(tr | od)` the recorded status is
  # od's, and `PIPESTATUS` is set in the substitution's subshell where it
  # cannot be read. See the same note in cat-diff.sh.
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  if [ "$stdin" = "-" ]; then
    "$OURS_ABS" "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
    $ref "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  else
    printf '%b' "$stdin" | "$OURS_ABS" "$@" >"$o_bin" 2>"$o_err"; o_rc=$?
    printf '%b' "$stdin" | $ref "$@" >"$g_bin" 2>"$g_err"; g_rc=$?
  fi
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  rm -f "$o_bin" "$g_bin"

  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")

  # stderr is compared in full, not merely for emptiness: the whole point of
  # the getopt module is that the sentences match, so a harness that only asked
  # "did it complain?" would pass on every wording this exists to fix.
  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
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

# A case with input: the first argument is fed to `printf '%b'` and piped in.
run_in() {
  [ "$HAVE_GNU" = yes ] || return 0
  local input="$1"; shift
  compare "$input" "$GNU" "$@"
  report "printf '$input' | tr $*"
}

# A case with no input, for the diagnostics — every one of them is decided
# before a byte is read.
run_case() { [ "$HAVE_GNU" = yes ] || return 0; compare - "$GNU" "$@"; report "tr $*"; }


xfail_case() {
  [ "$HAVE_GNU" = yes ] || return 0
  local reason="$1"; shift
  compare - "$GNU" "$@"
  if [ "$AGREED" = no ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL tr %s  (%s)\n' "$*" "$reason"
  else
    xpass=$((xpass+1))
    printf 'XPASS tr %s\n  now agrees with GNU, so this reason is stale: %s\n' "$*" "$reason"
  fi
  return 0
}

# Two of our own invocations compared against each other. The reference cannot
# arbitrate an abbreviation whose long form is *meant* to differ from GNU's, but
# the abbreviation must still resolve to the same option — which is the whole
# point of the getopt module — so that much is checked here.
selfsame() {
  local a="$1" b="$2" x y xr yr
  # shellcheck disable=SC2086  # both are single options by construction
  x=$("$OURS_ABS" $a </dev/null 2>&1); xr=$?
  y=$("$OURS_ABS" $b </dev/null 2>&1); yr=$?
  if [ "$x" = "$y" ] && [ "$xr" = "$yr" ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   tr %s == tr %s\n' "$a" "$b"
  else
    fail=$((fail+1))
    printf 'DIFF tr %s != tr %s\n  %s (rc=%s)\n  %s (rc=%s)\n' \
      "$a" "$b" "$(printf '%s' "$x" | tr '\n' '|')" "$xr" \
      "$(printf '%s' "$y" | tr '\n' '|')" "$yr"
  fi
  return 0
}

# --- plain translation --------------------------------------------------------
run_in 'hello world\n' abc xyz
run_in 'hello world\n' a-z A-Z
run_in 'HELLO World\n' A-Z a-z
run_in 'abcdef\n' abc ABC
# SET2 shorter than SET1 pads with SET2's *last* byte, rather than stopping.
run_in 'abcdef\n' abcdef xy
run_in 'abcdef\n' a-f x
run_in 'abcdef\n' a-f xyz
# SET2 longer than SET1 simply leaves the tail unused.
run_in 'abc\n' ab wxyz
# A byte named twice in SET1: the *later* pairing wins.
run_in 'aaa\n' aa xy
run_in 'abcabc\n' abca wxyz
# The identity translation, and one that maps a byte to itself.
run_in 'abc\n' abc abc
run_in 'abc\n' a a
# Empty input, and input with no newline at the end.
run_in '' a-z A-Z
run_in 'abc' a-z A-Z
run_in '\n\n\n' a-z A-Z
# ROT13, the canonical two-range case.
run_in 'Hello, World!\n' A-Za-z N-ZA-Mn-za-m

# --- ranges -------------------------------------------------------------------
run_in 'abcdef\n' a-c x-z
run_in '0123456789\n' 0-9 a-j
run_in 'abc\n' a-a x
# A range whose endpoints touch, and one spanning the whole byte space.
run_in 'abc\n' a-b xy
run_in '\101\102\103\n' '\000-\377' '\377'
# A trailing hyphen is an ordinary byte, not half a range.
run_in 'a-b\n' 'a-' 'xy'
run_in 'a-b\n' '-' 'x'
run_in 'a-b\n' 'ab-' 'xy-'
# An escaped hyphen is likewise ordinary.
run_in 'a-c\n' 'a\-c' 'xyz'

# --- delete -------------------------------------------------------------------
run_in 'hello world\n' -d aeiou
run_in 'hello world\n' -d ' '
run_in 'a1b2c3\n' -d 0-9
run_in 'line one\nline two\n' -d '\n'
run_in 'hello\n' -d ''
run_in 'hello\n' --delete aeiou
run_in 'hello\n' --delete=aeiou
# Deleting everything, and deleting nothing that is present.
run_in 'abc\n' -d 'a-z\n'
run_in 'abc\n' -d xyz

# --- squeeze ------------------------------------------------------------------
run_in 'aaabbbccc\n' -s abc
run_in 'hello    world\n' -s ' '
run_in 'aaabbb\n' -s a
# Squeeze with two sets squeezes SET2, and translates by SET1/SET2 first.
run_in 'aaabbb\n' -s ab xy
run_in 'aaa\n' -s a b
run_in 'hello\n' -s a-y a-y
# A translation that *creates* a run is squeezed after the translation.
run_in 'abab\n' -s ab xx
run_in 'aaabbb\n' --squeeze-repeats abc
# Squeezing a byte that never repeats.
run_in 'abc\n' -s abc

# --- delete and squeeze together ---------------------------------------------
run_in 'aaabbbccc\n' -ds a b
run_in 'hello   world\n' -ds aeiou ' '
run_in 'a1b22c333\n' -ds 0-9 abc

# --- complement ---------------------------------------------------------------
run_in 'hello world\n' -d -c 'a-z\n'
run_in 'hello world\n' -dc 'a-z\n'
run_in 'a1b2c3\n' -cd '0-9\n'
run_in 'hello world\n' -c 'a-z' '*'
run_in 'hello world\n' -c 'a-z\n' 'X'
run_in 'aaa   bbb\n' -cs 'a-z\n' ' '
run_in 'hello\n' -C 'a-z\n' 'X'
run_in 'hello\n' --complement -d 'a-z\n'
# The complement of a set is taken *before* SET2 is applied, so the padding
# byte covers 200-odd bytes.
run_in 'ab\n' -c 'a' 'xy'

# --- truncate -----------------------------------------------------------------
run_in 'abcdef\n' -t a-f xy
run_in 'abcdef\n' --truncate-set1 abcdef xy
run_in 'abc\n' -t abc ''
run_in 'abc\n' -t ab wxyz
# `-t` with an empty SET2 is the only way an empty SET2 is legal at all.
run_in 'abc\n' -t 'abc' ''

# --- character classes --------------------------------------------------------
run_in 'Hello, World! 42\n' -d '[:digit:]'
run_in 'Hello, World! 42\n' -d '[:punct:]'
run_in 'Hello, World! 42\n' -d '[:space:]'
run_in 'Hello, World! 42\n' -d '[:alpha:]'
run_in 'Hello, World! 42\n' -d '[:alnum:]'
run_in 'Hello, World! 42\n' -d '[:upper:]'
run_in 'Hello, World! 42\n' -d '[:lower:]'
run_in 'Hello\tWorld\n' -d '[:blank:]'
run_in 'Hello\tWorld\n' -d '[:cntrl:]'
run_in 'Hello World\n' -d '[:graph:]'
run_in 'Hello World\n' -d '[:print:]'
run_in 'deadBEEF123xyz\n' -d '[:xdigit:]'
# The two classes that may appear in SET2 at all.
run_in 'Hello World\n' '[:lower:]' '[:upper:]'
run_in 'Hello World\n' '[:upper:]' '[:lower:]'
run_in 'Hello World\n' '[:lower:]' '[:lower:]'
# A class in SET1 against an ordinary SET2 pads with the last byte.
run_in 'Hello World\n' '[:alpha:]' 'x'
run_in 'Hello World\n' '[:space:]' '_'
# Squeezing a class is the standard whitespace-collapse one-liner.
run_in 'a   b\t\tc\n\n\nd\n' -s '[:space:]'
run_in 'a   b\n' -s '[:blank:]'
# The standard "strip everything unprintable" one-liner.
run_in 'a\001b\002c\n' -cd '[:print:]\n'
run_in 'a\001b\002c\n' -d '[:cntrl:]'
# Classes combined with ordinary bytes and ranges in one set.
run_in 'Hello 42\n' -d '[:digit:]xyz'
run_in 'Hello 42\n' -d 'a-f[:digit:]'
# A class in SET1 with complement.
run_in 'Hello, World! 42\n' -cd '[:alpha:]\n'

# --- equivalence classes ------------------------------------------------------
# In the C locale an equivalence class holds exactly its own byte.
run_in 'hello\n' -d '[=l=]'
run_in 'hello\n' '[=l=]' 'L'
run_in 'hello\n' -d '[=l=][=o=]'
run_in 'hello\n' -s '[=l=]'

# --- the repeat construct -----------------------------------------------------
run_in 'abcdef\n' 'abcdef' '[x*]'
run_in 'abcdef\n' 'abc' '[x*3]'
run_in 'abcdef\n' 'abcdef' 'x[y*]'
run_in 'abcdef\n' 'abcdef' '[x*2]y'
run_in 'abcdef\n' 'abcdef' '[x*0]y'
# A leading zero makes the count octal, so `[x*010]` repeats eight times.
run_in 'abcdefghij\n' 'a-j' '[x*010]z'
run_in 'abcdefghij\n' 'a-j' '[x*10]z'
# The fill repeat is what makes `tr a-z '[x*]'` a one-liner.
run_in 'hello world\n' 'a-z' '[*]'
run_in 'hello world\n' '[:alpha:]' '[#*]'

# --- the repeat count is `strtoumax`, not a string of digits ------------------
# Upstream hands the field straight to `strtoumax`, so it inherits that
# function's whole grammar and not a hand-rolled digit scan: leading whitespace
# is skipped, a leading `+` is accepted, and `-` is not. A 13-byte SET1 is what
# makes the octal-vs-decimal cases discriminate — with a short SET1 the padding
# hides the difference.
run_in 'abcdef\n' 'a-f' '[x*1]y'
run_in 'abcdef\n' 'a-f' '[x* 1]y'
run_in 'abcdef\n' 'a-f' '[x*+1]y'
run_in 'abcdef\n' 'a-f' '[x* +1]y'
run_in 'abcdef\n' 'a-f' '[x*\n2]y'
# The base comes from the field's *raw* first byte, so skipped whitespace does
# not make `010` octal: ` 010` is ten and `010` is eight.
run_in 'abcdefghijklm\n' 'a-m' '[x*010]y'
run_in 'abcdefghijklm\n' 'a-m' '[x* 010]y'
run_in 'abcdefghijklm\n' 'a-m' '[x* 08]y'
# ... and the same rule makes `08` an invalid octal number rather than eight.
run_case a '[x*08]'
run_case a '[x*+ 1]'
run_case a '[x* ]'
run_case a '[x*-1]'
run_case a '[x*1x]'

# --- an escaped byte aborts the repeat scan ----------------------------------
# `find_bracketed_repeat` gives up at the first escaped byte of any kind, rather
# than looking past it for the `]`. So each of these is a run of literal bytes,
# even the ones whose `]` is unescaped and would otherwise close the construct.
run_in 'abcdef\n' 'a-f' '[x*a\b]y'
run_in 'abcdef\n' 'a-f' '[x*1\]y'
run_in 'abcdef\n' 'a-f' '[x*\062]y'
run_in 'abcdef\n' 'a-f' '[x*\]]y'
run_in '[x*1]y\n' -d '[x*1\]y'
# The class and equivalence scans have no such rule — they read straight past.
run_in 'abc\n' -d '[:al\pha:]'

# --- escapes ------------------------------------------------------------------
run_in 'a\tb\n' -d '\t'
run_in 'a\tb\n' '\t' ' '
run_in 'a\\b\n' -d '\\\\'
run_in 'a\nb\n' '\n' '\t'
run_in 'a\rb\n' -d '\r'
run_in 'a\ab\n' -d '\a'
run_in 'a\bb\n' -d '\b'
run_in 'a\fb\n' -d '\f'
run_in 'a\vb\n' -d '\v'
run_in 'a\000b\n' -d '\0'
run_in 'abc\n' '\141' 'X'
run_in 'abc\n' '\141-\143' 'XYZ'
run_in 'abc\n' -d '\141\142'
# An unknown escape is the escaped byte itself, with no warning.
run_in 'aqb\n' -d '\q'
run_in 'a.b\n' -d '\.'
# A trailing backslash is a literal backslash.
run_in 'a\\b\n' -d 'a\\'

# --- the literal-bracket fallback --------------------------------------------
# Every one of these is a bracket that opens no construct, so the bytes are
# ordinary. This fallback is what makes `tr -d '[]'` work at all.
run_in 'a[b]c\n' -d '[]'
run_in 'a[b]c\n' -d '['
run_in 'a[b]c\n' -d ']'
run_in 'a[b]c\n' '[' 'X'
run_in 'a:b\n' -d '[:]'
run_in 'a=b\n' -d '[=]'
run_in 'a*b\n' -d '[*]x'
run_in 'ab\n' '[ab]' 'XYZW'
run_in 'a[:b\n' -d '[:'
run_in 'a[=b\n' -d '[='
run_in 'a[b\n' -d '[a*'
# `[a*]` in SET1 is a fill repeat and refused; `[a*1]` is not.
run_in 'aaa\n' -d '[a*1]'
run_in 'aaa\n' -d '[a*2]'

# --- binary and high bytes ----------------------------------------------------
run_in 'a\377b\n' -d '\377'
run_in 'a\200\201b\n' '\200-\201' 'XY'
run_in 'a\000b\000c\n' -d '\000'
run_in 'a\000b\n' '\000' 'X'
run_in '\300\301\302\n' -d '\300-\302'
# A UTF-8 sequence is bytes, and `tr` will happily split one.
run_in '\303\251x\n' -d '\251'
run_in '\303\251x\n' 'a-z' 'A-Z'

# --- a run long enough to cross a read boundary ------------------------------
# Our filter reads in 64 KiB chunks and squeezes across the seam; GNU's buffer
# is a different size, so agreement here is the whole point.
if [ "$HAVE_GNU" = yes ]; then
  long=$(printf 'a%.0s' $(seq 1 200000))
  compare "$long" "$GNU" -s a; report "200k a's | tr -s a"
  compare "$long" "$GNU" -d a; report "200k a's | tr -d a"
  compare "$long" "$GNU" a b; report "200k a's | tr a b"
fi

# --- operand-count diagnostics ------------------------------------------------
run_case
run_case -d
run_case -s
run_case -c
run_case abc
run_case -ds abc
run_case -s abc def ghi
# The excess operand named is the first one past what the *mode* allows, not
# simply the third — deleting without squeezing allows one set — and the
# explanatory second line is dropped once more than one operand is excess.
run_case -d abc def
run_case abc def ghi
run_case abc def ghi jkl
run_case -d abc def ghi
run_case -d abc def ghi jkl
run_case -ds abc def ghi
run_case -dc abc def ghi
run_case -t abc def ghi

# --- set diagnostics ----------------------------------------------------------
run_case 'z-a' 'x'
run_case -d 'z-a'
run_case '\143-\141' 'x'
run_case -d '[:nosuch:]'
run_case -d '[::]'
run_case -d '[=ab=]'
run_case 'a' '[x*y]'
run_case 'a' '[x*-1]'
run_case 'a' '[x*]'
run_case '[x*]' 'a'
run_case 'abc' ''
run_case 'a' '[:digit:]'
run_case 'a-z' '[:space:]'
run_case 'a' '[=x=]'
run_case 'a[:lower:]' 'bc[:upper:]'
run_case '[:lower:]a' '[:upper:]bc'
# The alignment is by expanded offset, so this one is legal and that one is not.
run_in 'ab\n' 'a-b[:lower:]' 'cd[:upper:]'

# --- how the diagnostics render the text they echo back ----------------------
# `tr` does not use gnulib's `quote` for the text it quotes back. It has two
# private renderers, and which one runs depends on the message:
#
#   reverse range      `make_printable_char`  printable, else `\NNN` octal —
#                                             *never* a named escape
#   `[=c=]` operand    `make_printable_str`   named escapes for \a\b\t\n\v\f\r,
#                                             else printable-or-octal
#   invalid class      `make_printable_str` then `quote()`, so a `\` in the
#   invalid count      rendering comes back out doubled
#
# Neither renderer escapes `'` or `\`, which is what makes the composed cases
# above differ from the bare ones. None of this shows on printable input, so
# every case here is deliberately unprintable.
run_case '\377-\376' x
run_case '\012-\011' x
run_case '\177-\176' x
run_case '\140-\047' x
run_case '\041-\040' x
run_case -d '[=\n\t=]'
run_case -d '[=\a\b\v\f\r\177=]'
run_case -d '[==]'
run_case -d '[=\377\001=]'
run_case -d '[:no\377such:]'
run_case -d '[:a\nb:]'
run_case -d "[:a'b:]"
run_case a "[x*a'b]"
run_case a '[x*a\nb]'
run_case a '[x*\377]'

# --- the octal escape that is too large --------------------------------------
# GNU neither truncates nor refuses: it backs off to two digits and warns,
# on stderr, in two lines with a tab in the second.
run_in 'a b\n' -d '\400'
run_in 'a b\n' '\400' 'X'
run_in 'a b\n' -d '\777'

# --- option diagnostics -------------------------------------------------------
run_case -x a b
run_case --nosuch a b
run_case --del a b
run_case --=x a b
run_case -d -x a
# `--` ends the options, so a set may start with a hyphen.
run_in 'a-b\n' -d -- '-'
run_in 'a-b\n' -- '-' 'x'
run_case -d --
# Clustering and long-option abbreviation.
run_in 'aaabbb\n' -dsc 'a\n' 'b'
run_in 'hello\n' --dele aeiou
run_in 'hello\n' --sq l
run_in 'abcdef\n' --trunc a-f xy
run_in 'hello\n' --comp -d 'a-y\n'

# --- corners ------------------------------------------------------------------
# Empty sets, which are legal in the places they are legal at all.
run_in 'abc\n' -d ''
run_in 'abc\n' -s ''
run_in 'abc\n' -ds '' ''
run_in 'abc\n' -t '' ''
run_in 'abc\n' '' ''
run_in 'abc\n' -c -d ''
# An empty SET2 is refused only when SET1 has something left to translate, and
# the length compared is SET1's *post-complement* length — which is why the
# complemented case below errors while the bare one does not. `-t` suppresses
# the check outright, since truncating to nothing is a well-defined no-op.
run_in 'abc\n' '' x
run_in 'abc\n' -t a ''
run_in 'abc\n' -ct 'a-z' ''
run_case a ''
run_case -c '' ''
# `-t` where it has no effect: GNU documents it as significant only when
# translating, and accepts it silently everywhere else.
run_in 'aaabbb\n' -td a
run_in 'aaabbb\n' -ts a
run_in 'aaabbb\n' -tds a b
# `-c` and `-t` together truncate the *complemented* SET1.
run_in 'abcxyz\n' -ct 'a-c' 'XY'
run_in 'abcxyz\n' -ct 'a-c' 'X'
# Case conversion under complement, and classes on both sides at once.
run_in 'Hello World 42\n' -c '[:lower:]' 'X'
run_in 'Hello World\n' '[:upper:][:lower:]' '[:lower:][:upper:]'
run_in 'Hello World\n' 'a-z[:upper:]' 'A-Z[:lower:]'
# A fill repeat against a complemented SET1 has 200-odd bytes to cover.
run_in 'abc\n' -c 'a' '[x*]'
run_in 'abc\n' -c '\000-\176' '[x*]'
# Repeat counts at and past the edges.
run_in 'abcdef\n' 'a-f' '[x*1]y'
run_in 'abcdef\n' 'a-f' '[x*6]'
run_in 'abcdef\n' 'a-f' '[x*7]'
run_case 'a' '[x*99999999999999999999]'
run_case 'a' '[x*4294967296]'
run_case 'a' '[x* 1]'
run_case 'a' '[x*+1]'
run_case 'a' '[x*08]'
run_case 'a' '[x*1x]'
# Unterminated brackets, every shape, all of which fall back to literal bytes.
run_in 'a[:alpha\n' -d '[:alpha:'
run_in 'a[=x\n' -d '[=x'
run_in 'a[x*\n' -d '[x*'
run_in 'a[x*1\n' -d '[x*1'
run_in 'ab[\n' -d 'ab['
run_in 'a]b\n' -d 'a]'
run_in 'a[]b\n' -d '[[]]'
# The octal escape at its boundaries.
run_in 'a\000b\n' -d '\0'
run_in 'a\000b\n' -d '\00'
run_in 'a\000b\n' -d '\000'
run_in 'a\0000b\n' -d '\0000'
run_in 'a0b\n' -d '\060'
run_in 'a8b\n' -d '\8'
run_in 'a9b\n' -d '\9'
# A backslash before a bracket keeps the bracket literal.
run_in 'a[b\n' -d '\[b'
run_in 'a[:b\n' -d '\[:b'
run_in 'a*b\n' -d '[a\*]'
# A class next to a hyphen is not a range endpoint.
run_case '[:digit:]-x' 'y'
run_in 'a-9\n' -d '[:digit:]-'
# Duplicate bytes in SET1 under truncation.
run_in 'abc\n' -t 'aab' 'xy'
run_in 'abc\n' 'aab' 'xy'
# High-byte ranges, forward and reversed.
run_in '\376\377\n' '\376-\377' 'XY'
run_case '\377-\376' 'XY'
run_case -d '\377-\376'
# A NUL in every position it can occupy.
run_in 'a\000b\000\000c\n' -s '\000'
run_in 'a\000b\n' -d '\000a'
run_in 'a\000b\n' '\000ab' 'XYZ'
run_in 'abc\n' 'abc' '\000\000\000'
# Long options that take no argument still refuse one.
run_case --complement=x a b
run_case --delete=x a
run_case --squeeze-repeats=x a
run_case --truncate-set1=x a b
run_case --help=x
run_case --version=x
# An ambiguous prefix lists its candidates in GNU's declaration order.
run_case --c a b
run_case --s a b
run_case --t a b
run_case --de a
# `--v` is not ambiguous — it is `--version`, whose output is *meant* to differ.
# It is checked below as an xfail plus a `selfsame`, not here.

# --- differ on purpose --------------------------------------------------------
# `--help`'s body matches GNU's byte for byte; what follows it does not, and
# must not. GNU closes every `--help` with `emit_ancillary_info` — links to
# gnu.org, the Translation Project and `info '(coreutils) tr invocation'` —
# which name an upstream project this is not and documentation this does not
# ship. `--version` likewise names SlateOS coreutils rather than GNU coreutils
# 9.4 with its copyright and authors.
xfail_case help-closes-with-a-referral-to-the-gnu-project-which-this-is-not --he
xfail_case version-names-slateos-coreutils-not-gnu-coreutils --v
# The abbreviations above still have to resolve, which the comparison cannot
# show while the outputs are expected to differ.
selfsame --he --help
selfsame --v --version
selfsame --hel --help

# --- summary ------------------------------------------------------------------
if [ "$HAVE_GNU" != yes ]; then
  echo "tr-diff: skipped (no glibc tr)"
  exit 0
fi
printf '\n%d passed, %d differed' "$pass" "$fail"
[ "$xfail" -gt 0 ] && printf ', %d differ on purpose' "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d XPASS' "$xpass"
printf '\n'
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
