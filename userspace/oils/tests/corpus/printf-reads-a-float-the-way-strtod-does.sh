# `printf`'s float conversions read their argument with `strtod`, which knows
# C's hexadecimal form — `0x` then hex digits, optionally a `.`, optionally a
# `p` binary exponent — so `0x10` is 16 and `0X1P-2` is a quarter. The exponent
# is optional here, unlike `scanf`'s `%a`.
#
# When the word runs out of number, `strtod` keeps the prefix it did read and
# `printf` prints that value *and* complains, costing it a status but not the
# output. `0x` with no hex digit behind it backs up to the bare `0`, and a `p`
# with no digits behind it is dropped, so `0x1p` is 1.
#
# The complaint names a base, but not the one the value was read in: that is
# `sh_invalidnum`, which looks at the raw word's first two bytes only, with the
# octal arm tested first and a lower-case `x` only. So `0X3z` is read as hex and
# reported plainly, and a sign or a blank in front of the `0` hides the base
# from the message the same way.
#
# A value with no digits — an infinity or a NaN — is spelled by the case of the
# conversion letter alone, `nan`/`inf` against `NAN`/`INF`, and carries its sign
# bit, so a negative NaN prints `-nan`.
#
# Everything here is chosen to be exact in a double. bash reads and prints these
# as `long double`, so anything past 53 bits of mantissa, or outside a double's
# exponent range, would be measuring that rather than this.

e() { sed -e 's/^.*: line [0-9]*: /SH: /' -e 's/^/    /'; }
p() { { eval "$1"; echo "|rc=$?"; } 2>&1 | e; }

echo "== 1. the hexadecimal form, exponent and all"
for w in 0x10 0X10 0xA 0xa.b 0x10.8 0x1p4 0x1P4 0X1P-2 0x1.8p1 0x.8p1 0x0 -0x10 +0x10 0x1p-1074; do
  printf '  -- [%s]\n' "$w"
  p 'printf "%f\n" "$w"'
done

echo "== 2. where strtod stops, and what it keeps"
for w in 0x 0X 0x. 0xg 0x_ 0x10zz 0x1p 0x1p+ 0x1pz 0x1p4z 0x10.8zz 0.x; do
  printf '  -- [%s]\n' "$w"
  p 'printf "%f\n" "$w"'
done

echo "== 3. the same words as an integer conversion"
for w in 0x10 0X10 0x3z 0X3z 012x 09x 00z -012x ' 0x3z' 0b101; do
  printf '  -- [%s]\n' "$w"
  p 'printf "%d\n" "$w"'
done

echo "== 4. and the base the float side names"
for w in 012x 09x 00z 0X3z -012x 0.x; do
  printf '  -- [%s]\n' "$w"
  p 'printf "%f\n" "$w"'
done

echo "== 5. a value with no digits is spelled by the letter's case"
for c in f F e E g G a A; do
  printf "  %%$c: "
  printf "%$c|%$c|%$c|%$c\n" nan -nan inf -inf
done

echo "== 6. …and neither precision nor a pad flag reaches it"
p 'printf "[%08f][%-8f][%8.3f][%+f][% f]\n" nan nan nan nan nan'
p 'printf "[%08G][%#g][%.0e]\n" -inf inf inf'

echo "== 7. the ordinary spellings still read as before"
for w in 1.5 .5 5. -1.5 +1.5 1e3 1E3 1e-3 inf infinity INF nan 1.5e 1e .e3 infinit ''; do
  printf '  -- [%s]\n' "$w"
  p 'printf "%f\n" "$w"'
done
