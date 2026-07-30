# The `read` builtin: field splitting against IFS, the leftover-fields rule,
# -a/-d/-n/-N/-r, and the status at end of input.
printf 'one two three four\n' > f1
read -r a b c < f1
echo "a=[$a] b=[$b] c=[$c]"

# The *last* variable soaks up the remaining fields, including their internal
# separators but not the leading/trailing IFS whitespace.
printf '  lead   mid   trail  \n' > f2
read -r x y < f2
echo "x=[$x] y=[$y]"

# Fewer fields than variables leaves the extras empty.
read -r p q r <<< 'only two'
echo "p=[$p] q=[$q] r=[$r]"

# A custom IFS splits on exactly those characters, and a non-whitespace IFS
# character does NOT coalesce runs: `a::b` has an empty middle field.
IFS=: read -r u v w <<< 'a::b'
echo "u=[$u] v=[$v] w=[$w]"
IFS=: read -r u v <<< 'a:b:c'
echo "trailing-soak=[$v]"

# One delimiter is `ws* nonws? ws*`: IFS whitespace beside a non-whitespace IFS
# character belongs to the same delimiter, and a delimiter reaching the end of
# the line ends it rather than opening one last empty field.
nf() { # nf <ifs> <line>
  local IFS=$1
  read -r -a A <<< "$2"
  printf '%d fields:' "${#A[@]}"
  local e; for e in "${A[@]}"; do printf '[%s]' "$e"; done
  echo
}
nf ':' 'a:b:'
nf ':' 'a:b::'
nf ':' 'a:'
nf ':' ':'
nf ':' '::'
nf ':' ':::'
nf ': ' 'a : b'
nf ': ' 'a  :'
nf ': ' 'a:  '
nf ': ' ' :a: '
nf ': ' 'a: :b'
nf ': ' 'a: : '
nf ' ' '  a  b  '

# The last name soaks up the remaining words *and their separators* — but only
# when there are more fields than names. `a:b:` is two fields, so two names each
# take one and the trailing delimiter is simply gone; `a:b:c:` is three, so the
# second name keeps the separators and the trailing delimiter with them.
soak() { # soak <ifs> <line> <names...>
  local ifs=$1 line=$2; shift 2
  printf '%-9s %-9s' "[$line]" "$*"
  IFS=$ifs read -r "$@" <<< "$line"
  local v; for v in "$@"; do printf '[%s]' "${!v}"; done
  echo
}
soak ':' 'a:b:'   x y
soak ':' 'a:b:c:' x y
soak ':' 'a:b::'  x y
soak ':' 'a::b:'  x y
soak ':' 'a:b::'  x y z
soak ':' 'a:b:'   x
soak ':' ':a:'    x
soak ':' '::'     x
soak ':' 'a:'     x
soak ':' ':'      x
soak ': ' 'a:  '  x
soak ': ' 'a  :'  x
soak ' ' 'a b '   x
soak ':' 'a:b: '  x y
soak ': ' 'a:b: ' x y
soak ':' 'a'      x y

# Backslash handling: without -r, a backslash escapes the next character (and a
# backslash-newline continues the line); with -r it is literal.
printf 'a\\tb\n' > f3
read line < f3
echo "no-r=[$line]"
read -r line < f3
echo "with-r=[$line]"

# -a reads all fields into an array.
read -r -a arr <<< 'k1 k2 k3'
echo "arr=${arr[*]} n=${#arr[@]}"

# -d changes the delimiter; the delimiter itself is not stored.
printf 'alpha;beta;' > f4
read -r -d ';' d1 < f4
echo "d1=[$d1]"

# -n stops after N characters, -N reads exactly N (ignoring the delimiter).
printf 'abcdef\n' > f5
read -r -n 3 n1 < f5
echo "n1=[$n1]"
read -r -N 4 n2 < f5
echo "n2=[$n2]"

# At end of input `read` returns non-zero, but still assigns what it got.
printf 'partial' > f6
read -r last < f6
echo "eof-status=$? last=[$last]"

# Only the *delimiter* comes off the line. A `\r` before the newline is ordinary
# data, so CRLF-framed input arrives intact — and the `-n`/`-d` record path
# agrees. (Piped through `od` because a bare CR would rewrite the line on a
# terminal; `$( )` is avoided here because MSYS bash strips a trailing CR from a
# command substitution, which real bash and osh do not.)
printf 'a\r\nb\r\n' > f8
IFS= read -r cr1 < f8
printf '%s' "$cr1" | od -An -c
read -r -n 3 cr2 < f8
printf '%s' "$cr2" | od -An -c
read -r -d $'\r' cr3 < f8
printf '%s' "$cr3" | od -An -c
n=0
while IFS= read -r ln; do n=$((n + 1)); done < f8
echo "crlf-lines=$n"

# The classic while-read loop over a file, and the fact that it consumes the
# whole file in one subshell-free pass.
printf 'l1\nl2\nl3\n' > f7
n=0
while read -r ln; do n=$((n + 1)); echo "got $n:$ln"; done < f7
echo "loop-count=$n"
