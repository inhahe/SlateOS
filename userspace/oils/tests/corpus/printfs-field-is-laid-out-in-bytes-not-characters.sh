# bash's `printf` does not lay out its own field. `PF` (builtins/printf.def:124)
# reconstructs the directive and hands the width and precision to the C library
# as `%*.*s`, so what does the padding is C — and C's `%s` is a `char *` counted
# in *bytes*. A multibyte argument therefore fills more of the field than its
# width suggests: `%-8s` on `a…b` is five bytes of text and three spaces, not
# eight columns.
#
# `%c` is the same story one step earlier: `getchr` is
# `(int)garglist->word->word[0]` (printf.def:1165), a single `char`. So `%c`
# takes the first *byte* and will cut a multibyte character in half.
#
# None of this is locale-dependent, which is the point worth checking: nothing
# on the path ever decodes the string, so the answers below are the same in a
# UTF-8 locale and in C. That makes them different in kind from `${#s}`,
# `${s:o:l}` and `${v^^}`, which bash really does compute in characters when the
# locale has them — see `known-issues.md`,
# `TD-OILS-THE-CORPUS-HARNESS-RUNS-THE-REFERENCE-BASH-IN-THE-C-LOCALE`.
#
# Precision is bytes too, and has always been, so `%.2s` on `a…b` cuts the `…`
# mid-character and emits a lone lead byte. That is not an error to bash.
#
# Verified against bash 5.2.37, under both LC_ALL=C and LC_ALL=C.UTF-8.

p() { printf '%-30s [%s]\n' "$1" "$2"; }

# Built with printf so the file itself stays ASCII: e=…(3 bytes), a=é(2 bytes).
e=$(printf '\xe2\x80\xa6')
a=$(printf '\xc3\xa9')

echo "=== width pads to a byte count ==="
p 'left, 3-byte char'   "$(printf '%-8s' "a${e}b")"
p 'right, 3-byte char'  "$(printf '%8s'  "a${e}b")"
p 'left, 2-byte char'   "$(printf '%-8s' "a${a}b")"
p 'ascii, for contrast' "$(printf '%-8s' 'ab')"
# Wide enough to need no padding at all, and the text is not truncated.
p 'no padding needed'   "$(printf '%-4s' "a${e}b")"

echo "=== so the same width gives visibly different column counts ==="
printf '|%-8s|\n' "a${e}b"
printf '|%-8s|\n' 'aXb'

echo "=== precision cuts by bytes, mid-character and all ==="
p 'cut before the char'  "$(printf '%.1s' "a${e}b")"
p 'cut inside the char'  "$(printf '%.2s' "a${e}b")"
p 'cut after the char'   "$(printf '%.4s' "a${e}b")"
p 'width and precision'  "$(printf '%-8.2s' "a${e}b")"

echo "=== %c takes the first byte, not the first character ==="
p 'a multibyte argument'  "$(printf '%c' "${e}x")"
p 'an ascii argument'     "$(printf '%c' 'xy')"
p 'padded'                "$(printf '%-4c' "${e}x")"
# An empty argument still has a first byte: C's terminator.
printf '%c' '' | od -An -tu1

echo "=== a byte that is no character is just a byte ==="
b=$(printf '\xff')
p 'a lone 0xff, width'    "$(printf '%-6s' "a${b}b")"
p 'a lone 0xff, %c'       "$(printf '%c' "${b}z")"

echo done
