# pipefail reports the rightmost non-zero stage.
set -o pipefail
false | true
echo "a=$?"
true | false | true
echo "b=$?"
set +o pipefail
false | true
echo "c=$?"
