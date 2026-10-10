#!/bin/bash
# Line and region coverage of src/ by test targets, with -C instrument-coverage
# (needs a toolchain with the profiler runtime, e.g. Linux's, and the llvm-tools
# component). Run from the crate's directory; writes report.txt (per file)
# and show.txt (each line's count) to $OUT.
#   bash tools/coverage.sh OUT TEST_TARGET... [-- extra test args]
set -u
OUT=$1
shift
TESTS=()
while [ $# -gt 0 ] && [ "$1" != "--" ]; do TESTS+=("$1"); shift; done
[ "${1:-}" = "--" ] && shift
rm -rf "$OUT"
mkdir -p "$OUT/raw"
export CARGO_TARGET_DIR="$OUT/target"
export RUSTFLAGS="-C instrument-coverage"
export CARGO_PROFILE_DEV_OPT_LEVEL=0
export LLVM_PROFILE_FILE="$OUT/raw/%p-%m.profraw"
BIN=$(rustc --print sysroot)/lib/rustlib/x86_64-unknown-linux-gnu/bin
ARGS=()
for t in "${TESTS[@]}"; do
  if [ "$t" = lib ]; then ARGS+=(--lib); else ARGS+=(--test "$t"); fi
done
cargo test "${ARGS[@]}" -- "$@" > "$OUT/test.log" 2>&1
echo "test exit=$?"
grep -a "test result" "$OUT/test.log"
OBJECTS=()
for exe in $(cargo test "${ARGS[@]}" --no-run --message-format=json 2>/dev/null \
  | python3 -c "import sys, json
for l in sys.stdin:
    try: m = json.loads(l)
    except Exception: continue
    if m.get('reason') == 'compiler-artifact' and m.get('executable'): print(m['executable'])"); do
  OBJECTS+=(--object "$exe")
done
"$BIN/llvm-profdata" merge -sparse "$OUT"/raw/*.profraw -o "$OUT/merged.profdata"
"$BIN/llvm-cov" report "${OBJECTS[@]}" --instr-profile "$OUT/merged.profdata" \
  --ignore-filename-regex='(/rustc/|\.cargo|tests/)' > "$OUT/report.txt"
"$BIN/llvm-cov" show "${OBJECTS[@]}" --instr-profile "$OUT/merged.profdata" \
  --ignore-filename-regex='(/rustc/|\.cargo|tests/)' --show-line-counts-or-regions \
  > "$OUT/show.txt"
cat "$OUT/report.txt"
