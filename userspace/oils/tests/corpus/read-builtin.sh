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

# The classic while-read loop over a file, and the fact that it consumes the
# whole file in one subshell-free pass.
printf 'l1\nl2\nl3\n' > f7
n=0
while read -r ln; do n=$((n + 1)); echo "got $n:$ln"; done < f7
echo "loop-count=$n"
