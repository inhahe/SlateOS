# The single-letter invocation options, and the getopt-style pass that reads them.
#
# bash's `parse_shell_options` walks argv while the word starts with `-` or `+`,
# and inside each word walks the letters. Two of its properties are surprising and
# both are load-bearing:
#
#   * A mode letter does not end the walk. `case 'c'` only sets a flag, so the
#     walk carries on into the *next* word — `bash -c -x 'echo hi'` traces the
#     command instead of trying to run `-x`, and `bash -s -x` applies `-x` rather
#     than making it `$1`. Only `--`, a bare `-`, or a word starting with neither
#     `-` nor `+` ends the walk.
#   * `-c` and `-s` are independent flags, not one setting, and `-c` wins in
#     either order — bash looks for a pending command string before it looks at
#     `read_from_stdin`.
#
# `-o` and `-O` are letters like any other, which is why they can be bundled
# (`-eo pipefail`, `-oc pipefail cmd`). Each takes the *next word*, never the rest
# of its own cluster, and each one seen advances the cursor — so the cursor is
# also what decides where a bundled `-c`'s command string starts. With no word
# left they *list* the options rather than failing, and the shell carries on.
#
mk() { printf '%s\n' "$2" > "$1"; }
mk s.sh 'echo "  MARK script args=[$*]"'

echo "=== a mode letter does not end the option walk"
"$BASH" --noprofile -c -x 'echo "  MARK cmd"' 2>&1 | grep -e '^+ echo' -e '^  MARK'
"$BASH" --noprofile -c -- 'echo "  MARK after-dashdash"' | grep '^  MARK'
"$BASH" --noprofile -c +x 'echo "  MARK plus-cluster"' | grep '^  MARK'

echo "=== -c beats -s in either order"
printf 'echo "  MARK from-stdin"\n' | "$BASH" --noprofile -cs 'echo "  MARK from-c"' | grep '^  MARK'
printf 'echo "  MARK from-stdin"\n' | "$BASH" --noprofile -sc 'echo "  MARK from-c"' | grep '^  MARK'

echo "=== -o and -O bundle, taking the next word each"
"$BASH" --noprofile -eo pipefail -c 'shopt -op pipefail; shopt -op errexit' | sed 's/^/  /'
"$BASH" --noprofile -oo pipefail xtrace -c 'shopt -op pipefail; shopt -op xtrace' 2>/dev/null | sed 's/^/  /'
"$BASH" --noprofile -O extglob -c 'shopt -p extglob' | sed 's/^/  /'
"$BASH" --noprofile +O expand_aliases -c 'shopt -p expand_aliases' | sed 's/^/  /'

echo "=== the cursor they advance is where a bundled -c's command starts"
"$BASH" --noprofile -oc pipefail 'echo "  MARK oc"; shopt -op pipefail' | sed 's/^/  /'
"$BASH" --noprofile -ooc pipefail xtrace 'echo "  MARK ooc"' 2>/dev/null | grep '^  MARK'

echo "=== with no word left they list instead of failing, and the shell runs on"
"$BASH" --noprofile -s -o < /dev/null | head -2 | sed 's/^/  /'
"$BASH" --noprofile -s +o < /dev/null | head -2 | sed 's/^/  /'
"$BASH" --noprofile -s -O < /dev/null | head -2 | sed 's/^/  /'
"$BASH" --noprofile -s +O < /dev/null | head -2 | sed 's/^/  /'
printf 'echo "  MARK ran on"\n' | "$BASH" --noprofile -s -o > out; echo "  rc=$?"
grep '^  MARK' out

echo "=== a bad name is fatal, before the command runs"
"$BASH" --noprofile -o bogus -c 'echo "  MARK not reached"' 2>/dev/null | grep '^  MARK'
echo "  rc=$?"
"$BASH" --noprofile -O bogus -c 'echo "  MARK not reached"' 2>/dev/null | grep '^  MARK'
echo "  rc=$?"

echo "=== a bare - is just end-of-options, exactly like --"
# It reads stdin only because nothing is *left* to run as a script, which is why
# the word after it is a filename rather than another option.
printf 'echo "  MARK bare-dash-alone"\n' | "$BASH" --noprofile - | grep '^  MARK'
"$BASH" --noprofile - -x 2>/dev/null; echo "  rc=$?"
"$BASH" --noprofile -- -x 2>/dev/null; echo "  rc=$?"
"$BASH" --noprofile - s.sh 2>/dev/null; echo "  rc=$?"

echo "=== \$- reports the letters that are set options"
"$BASH" --noprofile -eu -c 'case "$-" in *e*u*) echo "  MARK eu in dash";; esac' | grep '^  MARK'
"$BASH" --noprofile +H -c 'case "$-" in *H*) echo "  MARK H present";; *) echo "  MARK H absent";; esac' | grep '^  MARK'

echo "=== an unrecognised letter aborts, having applied the ones before it"
"$BASH" --noprofile -xz -c 'true' 2>&1 >/dev/null | grep -o -e '-z: invalid option'
"$BASH" --noprofile -xz -c 'true' >/dev/null 2>/dev/null; echo "  rc=$?"
"$BASH" --noprofile +xz -c 'true' 2>&1 >/dev/null | grep -o -e '+z: invalid option'

echo "=== a script's own -s/-c letters are just arguments to it"
"$BASH" --noprofile s.sh -c -s -x | grep '^  MARK'

echo "=== and this script itself was started the ordinary way"
case "$0" in *case.sh) echo "  MARK dollar0 is the script";; *) echo "  MARK dollar0=[$0]";; esac
