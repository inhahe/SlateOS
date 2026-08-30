# `select`'s menu layout, which bash builds in `print_select_list`: items are
# packed **column-major** into as many `COLS`-wide columns as fit, and the
# gutters are padded with *tabs* wherever a tab stop can be reached. Measured
# against bash 5.2.
#
#   * the column stride is the widest item plus the index width, ") " and a
#     two-column gutter; the column count is COLS/stride, then re-derived from
#     the row count so the last column is as full as possible;
#   * a layout that comes out one row tall is transposed into one *column*,
#     which is why a short list prints one item per line;
#   * the first column right-aligns its indices to the digit count of the *row*
#     count and every later column to that of the *item* count, so with two
#     rows and twelve items the leftmost column alone loses its leading blank;
#   * COLS is `$COLUMNS` read the way `atoi` reads it — leading whitespace and
#     a sign are skipped and the first non-digit ends the number, so `40x` is
#     40 while `abc` is 0 and falls back to 80.
#
# The menu and the PS3 prompt go to *stderr* (the closing newline at EOF goes
# to stdout), so every probe merges them. `cat -A`-style tab visibility is not
# available portably, so tabs are spelled out with `tr`.
show() { sed 's/\t/<TAB>/g'; }
menu() { COLUMNS=$1 "$BASH" -c "select o in $2; do break; done" </dev/null 2>&1 | show; }

echo "=== a short list is one item per line"
menu 80 'aaaa bbbb cccc'
echo "=== twelve one-character items at 80"
menu 80 'a b c d e f g h i j k l'
echo "=== the same list narrowed"
menu 40 'a b c d e f g h i j k l'
menu 20 'a b c d e f g h i j k l'
menu 1 'a b c d e f g h i j k l'
echo "=== items wider than half the screen"
menu 80 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb cccccccccccccccccccccccccccccc'
echo "=== two-digit indices pad the later columns only"
menu 80 'i1 i2 i3 i4 i5 i6 i7 i8 i9 i10 i11'
echo "=== ragged widths"
menu 60 'x yy zzz wwww vvvvv uuuuuu ttttttt ssssssss rrrrrrrrr qqqqqqqqqq'
echo "=== a hundred items"
menu 80 "$(seq 1 100 | tr '\n' ' ')"

echo "=== COLUMNS is read like atoi"
for c in 40x ' 40' +40 0040 -5 0 abc '' 999999999999999999999999; do
    printf 'COLUMNS=[%s] ' "$c"
    COLUMNS=$c "$BASH" -c 'select o in aaaa bbbb cccc dddd eeee ffff gggg hhhh; do break; done' \
        </dev/null 2>&1 | show | head -1
done

echo "=== the menu is reprinted for a blank line but not between iterations"
printf '\n1\n2\n' | COLUMNS=80 "$BASH" -c \
    'select o in aa bb; do echo "got=$o"; [ "$o" = bb ] && break; done' 2>&1 | show

echo "=== an empty list runs nothing and succeeds"
COLUMNS=80 "$BASH" -c 'select o in; do echo NOT-REACHED; done; echo "rc=$? o=${o-unset}"' 2>&1 | show
set --
COLUMNS=80 "$BASH" -c 'select o; do echo NOT-REACHED; done; echo "rc=$?"' 2>&1 | show

echo "=== PS3 and \$@"
printf '2\n' | COLUMNS=80 "$BASH" -c 'PS3="pick> "; select o; do echo "got=$o"; break; done' x aa bbb 2>&1 | show
echo "=== PS3 is re-read every prompt"
printf '\n1\n' | COLUMNS=80 "$BASH" -c 'PS3=one; select o in a b; do break; done' 2>&1 | show
