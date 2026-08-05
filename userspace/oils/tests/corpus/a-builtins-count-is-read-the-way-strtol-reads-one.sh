# Three different number readers live behind the builtins, and which one a
# builtin uses is observable.
#
# `shift` and `read -n`/`-N`/`-u` use bash's `legal_number`, which is `strtol`:
# blanks in front of the digits are stepped over, blanks behind them are
# allowed to trail, and a `+` or `-` may lead. So `shift " 1"` shifts once and
# `shift " -1"` is a count out of range rather than no count at all.
#
# `ulimit` uses `all_digits` instead — nothing but digits, so no blank and no
# sign — which is why `ulimit -n +5` is refused where `shift +1` is not. A word
# of no bytes passes that test vacuously and is the number zero.
#
# `read -t` uses `uconvert`, which is neither: `[+-]? DIGIT* ( . DIGIT* )?` and
# nothing left over. An empty word is zero and so is `.`, while ` 1`, `1e2` and
# `0x1` are not timeouts at all.
#
# When a word is refused, the complaint names the base it seems to have reached
# for: a `0` before a digit is octal, a `0x` is hex — but only a lowercase `x`,
# and only where no sign or blank came first.

e() { sed -e 's/^.*: line [0-9]*: /SH: /' -e 's/^/    /'; }

echo "== 1. shift steps over the blanks strtol steps over"
for a in ' 1' '  +1' '1 ' '	1' '+1' '-1' ' -1' '+ 1' '1x' '' '0x1'; do
  ( set -- a b c d e
    { shift "$a"; echo "  shift [$a] rc=$? \$1=$1"; } 2>&1 | e )
done

echo "== 2. read -n and -N read a count the same way"
for a in ' 2' '2 ' '+2' '-2' '0x3' '0X3' '012x' '09' '' 'abc' '1e2'; do
  { read -n "$a" x </dev/null; echo "  read -n [$a] rc=$?"; } 2>&1 | e
  { read -N "$a" x </dev/null; echo "  read -N [$a] rc=$?"; } 2>&1 | e
done

echo "== 3. read -u reads a descriptor the same way"
for a in '+0' ' 0' '0 ' '-1' 'abc' '0x1'; do
  { read -u "$a" x </dev/null; echo "  read -u [$a] rc=$?"; } 2>&1 | e
done

echo "== 4. read -t is its own grammar"
for a in '' ' ' '.' '0' '0.5' '.5' '5.' '00.5' '-0' '+1' ' 1' '1 ' '1e2' '0x1' '1.2.3' '1,5' 'inf' 'nan' '-1'; do
  { read -t "$a" x </dev/null; echo "  read -t [$a] rc=$?"; } 2>&1 | e
done

echo "== 5. ulimit wants digits and nothing else"
for a in '+5' ' 5' '5 ' '-5' 'abc' '0x10' '0X10' '012x' '1e3' 'unlimit' 'UNLIMITED' 'Hard'; do
  { ulimit -n "$a"; echo "  ulimit -n [$a] rc=$?"; } 2>&1 | e
done

echo "== 6. the three words that are not numbers at all"
{ ulimit -c unlimited; ulimit -c; } 2>&1 | e
{ ulimit -c hard; echo "  rc=$?"; } 2>&1 | e
{ ulimit -c soft; echo "  rc=$?"; } 2>&1 | e
