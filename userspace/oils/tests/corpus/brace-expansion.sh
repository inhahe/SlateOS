# Brace expansion: the first expansion the shell performs, before parameter,
# command, and arithmetic expansion — and purely textual, so it happens even
# when nothing it produces exists.

# A comma list expands left to right, keeping the preamble and postscript.
echo pre{a,b,c}post

# An empty element is still an element.
echo x{,y}z

# Nested braces expand outward-in; the result is the cross product.
echo {a,b}{1,2}
echo {a,{b,c}d}e

# A sequence expression counts up or down, and honours an increment.
echo {1..5}
echo {5..1}
echo {1..10..3}
echo {a..e}
echo {e..a}

# A zero-padded sequence keeps the width of the widest endpoint.
echo {01..10}
echo {-3..3}

# A brace with no comma and no valid range is *not* an expansion — it stays
# literal, braces and all.
echo {single}
echo {}
echo {1..}

# Brace expansion happens before parameter expansion, so a variable inside the
# braces is NOT used to build the list — the braces are already gone.
n=3
echo {1..$n}
# ...but the *result* of brace expansion is still subject to the later stages.
v=world
echo {hello,$v}

# Quoting suppresses it entirely.
echo "{a,b}"
echo '{a,b}'
echo \{a,b\}

# Brace expansion is not applied to the result of parameter expansion.
list='{a,b}'
echo $list

# It applies to every word of a command, including ones that become multiple
# arguments — counted here so the argument split is visible.
count() { echo "argc=$#"; }
count {a,b,c}
count {1..4}
count "{1..4}"

# A brace expression may be attached to other expansions in the same word.
echo ${v}{1,2}
echo {x,y}${v}

# Backslash inside a brace list protects the comma.
echo {a\,b,c}

# Unbalanced braces are literal.
echo {a,b
echo a,b}
