#!/bin/bash
# Round 3 of the -S measurement: are OPTIONS inside the split string honoured?
#
# This decides the implementation shape. If `env -S'-i cmd'` applies -i, the
# split words must re-enter the option parser; if it does not, they are plain
# operands and the split is a much smaller change. Guessing either way would
# put the answer in the code with nothing behind it.
#
# A file, not a `bash -c` one-liner: the round-1/2 probe used `<%s>` as its
# marker and the `<` and `>` were re-parsed when wsl.exe reassembled the
# command line, so every label came back empty. Marker here is `[%s]`.
set -u
ENV=/usr/bin/env
export FOO=bar

p() { printf '%-30s rc=%-4s %s\n' "$1" "$2" "$3"; }

run() {
    local label="$1"; shift
    local out rc
    out=$("$@" 2>&1)
    rc=$?
    # Collapse newlines so a multi-line result stays on one row.
    p "$label" "$rc" "$(printf '%s' "$out" | tr '\n' '|')"
}

echo "=== GNU $($ENV --version | head -1) ==="

# --- control: the marker itself works, and the failing half fails -----------
run 'CONTROL plain'        "$ENV" /usr/bin/printf '[%s]' a b
run 'CONTROL needs -S'     "$ENV" -S'/usr/bin/printf [%s] a b'
run 'CONTROL must fail'    "$ENV" '/usr/bin/printf [%s] a b'

# --- options inside the split string ----------------------------------------
# -i clears the environment. If honoured inside -S, `env` printing its own
# environment shows nothing; if not, -i is passed to printf as an argument.
run 'short -i inside'      "$ENV" -S'-i /usr/bin/printf [%s] x'
run 'count env with -i'    "$ENV" -S'-i /usr/bin/env'
run 'count env without'    "$ENV" -S'/usr/bin/env'
run 'long --unset inside'  "$ENV" -S'--unset=FOO /usr/bin/env'
run 'assignment inside'    "$ENV" -S'X=1 /usr/bin/env'
run 'opt AFTER command'    "$ENV" -S'/usr/bin/printf [%s] -i'

# --- where -S may appear ----------------------------------------------------
run '-vS combined'         "$ENV" -vS'/usr/bin/printf [%s] a'
run '-S then more argv'    "$ENV" -S'/usr/bin/printf [%s] a' b
run 'option before -S'     "$ENV" -i -S'/usr/bin/env'
run '-S not first'         "$ENV" Y=2 -S'/usr/bin/printf [%s] a'

# --- degenerate strings -----------------------------------------------------
run 'empty string'         "$ENV" -S''
run 'only spaces'          "$ENV" -S'   '
run 'only a comment'       "$ENV" -S'# nothing here'

echo "=== done ==="
