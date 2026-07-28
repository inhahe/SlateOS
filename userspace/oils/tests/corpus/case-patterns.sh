# `case` matching: glob metacharacters, character classes, alternation, quoting
# that disables metacharacters, and the `;;&`/`;&` fallthrough terminators.
for w in abc a.c '' '*' 'A' 'z9'; do
  case $w in
    '')      echo "[$w] empty" ;;
    a?c)     echo "[$w] a-any-c" ;;
    \*)      echo "[$w] literal-star" ;;
    [[:upper:]]) echo "[$w] upper" ;;
    [a-y]*[0-9]) echo "[$w] letter-then-digit" ;;
    *)       echo "[$w] default" ;;
  esac
done

# `;&` falls through to the next body unconditionally; `;;&` re-tests from the
# next pattern onward.
classify() {
  case $1 in
    one)  echo "  one" ;&
    two)  echo "  two" ;;
    *)    echo "  other" ;;
  esac
}
classify one
classify two

retest() {
  case $1 in
    a*) echo "  starts-a" ;;&
    *z) echo "  ends-z" ;;&
    ??) echo "  two-chars" ;;
  esac
}
retest az
retest abz

# The word is expanded (and NOT split) before matching; patterns are expanded too.
pat='a*'
word='a b'
case $word in
  $pat) echo "unquoted-pattern matched" ;;
  *)    echo "unquoted-pattern no" ;;
esac
case $word in
  "$pat") echo "quoted-pattern matched" ;;
  *)      echo "quoted-pattern no" ;;
esac

# No match at all leaves status 0.
case nope in x) ;; esac
echo "nomatch-status=$?"
