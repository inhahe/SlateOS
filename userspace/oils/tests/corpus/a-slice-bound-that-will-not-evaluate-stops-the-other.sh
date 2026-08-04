# `${v:off:len}` reads its two bounds left to right, and stops at the first one
# that does not answer. An arithmetic error in the offset ends the expansion
# there — the length is never looked at, so it is never complained about and
# whatever it would have done is never done. The offset also decides, before the
# length is read at all, whether the parameter has a position to start from: an
# offset past the end returns nothing without evaluating the length, which is
# how `${v:99:j++}` leaves `j` alone.
#
# Every spelling of the operator answers the same way — a scalar, an element, an
# array, the positionals, an associative array — because they are all the same
# operator underneath.
#
# What is *not* this rule: a length that evaluates to a negative number. That is
# a real value, read from a real position, and it is either an end-position (on
# a string) or a fatal "substring expression < 0" (on a list).

v=abcdef
a=(p q r)
m=([k]=one)
set -- x y

echo "### a bound that will not evaluate says so once, not twice"
echo "[${v:'0':'2'}]"
echo "[${a[@]:'0':'2'}]"
echo "[${@:'0':'2'}]"
echo "[${m[@]:'0':'2'}]"
echo "[${v:0:'2'}]"
echo "[${a[@]:0:'2'}]"

echo "### the offset decides whether the length is read at all"
j=0; echo "[${v:0:j++}] j=$j"
j=0; echo "[${v:6:j++}] j=$j"
j=0; echo "[${v:7:j++}] j=$j"
j=0; echo "[${v:99:j++}] j=$j"
j=0; echo "[${v: -1:j++}] j=$j"
j=0; echo "[${v: -6:j++}] j=$j"
j=0; echo "[${v: -7:j++}] j=$j"

echo "### and an empty scalar has exactly one position"
w=
j=0; echo "[${w:0:j++}] j=$j"
j=0; echo "[${w:1:j++}] j=$j"
j=0; echo "[${w: -1:j++}] j=$j"

echo "### a list counts its own way, and still gates the length"
j=0; echo "[${a[@]:0:j++}] j=$j"
j=0; echo "[${a[@]:2:j++}] j=$j"
j=0; echo "[${a[@]:3:j++}] j=$j"
j=0; echo "[${@:0:j++}] j=$j"
j=0; echo "[${@:2:j++}] j=$j"
j=0; echo "[${@:3:j++}] j=$j"

echo "### an array with nothing in it reads neither bound"
e=()
j=0; echo "[${e[@]:0:j++}] j=$j"
j=0; echo "[${nada[@]:0:j++}] j=$j"

echo "### an element is a scalar and answers as one"
j=0; echo "[${a[1]:0:j++}] j=$j"
j=0; echo "[${a[1]:2:j++}] j=$j"

echo "### a negative length is a value, not a failure to evaluate"
echo "[${v:1:-1}]"
echo "[${v:6:-1}]"
echo "### and on a list it is fatal"
echo "[${a[@]:1:-1}]"
echo "still here"
