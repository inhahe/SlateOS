#!/bin/bash
# Profiles tests/bench.rs (one pass) and Tremor's tools/bench.c under
# callgrind, in WSL, from a copy of the crate in ~/vorbis-bench-src; writes
# ~/cg/rust.out, ~/cg/c.out and annotated text beside them.
#   bash tools/profile.sh
set -u
source ~/.cargo/env 2>/dev/null
VG="env VALGRIND_LIB=$HOME/valgrind-root/usr/libexec/valgrind $HOME/valgrind-root/usr/bin/valgrind"
CA=$HOME/valgrind-root/usr/bin/callgrind_annotate
mkdir -p ~/cg
rm -rf ~/vorbis-bench-src && mkdir -p ~/vorbis-bench-src
cp -r src tests Cargo.toml ~/vorbis-bench-src/
cd ~/vorbis-bench-src
export CARGO_TARGET_DIR=~/vorbis-bench-target
BIN=$(CARGO_PROFILE_RELEASE_OPT_LEVEL=3 CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 CARGO_PROFILE_RELEASE_DEBUG=true \
  cargo test --release --test bench --no-run --message-format=json 2>/dev/null \
  | python3 -c "import sys, json
for l in sys.stdin:
    try: m = json.loads(l)
    except Exception: continue
    if m.get('reason') == 'compiler-artifact' and m.get('executable'): print(m['executable'])" | tail -1)
VORBIS_BENCH_PASSES=1 $VG --tool=callgrind --callgrind-out-file=$HOME/cg/rust.out "$BIN" --ignored --test-threads=1 > /dev/null 2> ~/cg/rust.err
grep Collected ~/cg/rust.err
VORBIS_BENCH_PASSES=1 $VG --tool=callgrind --callgrind-out-file=$HOME/cg/c.out ~/vorbisref/build/bench tests/data > /dev/null 2> ~/cg/c.err
grep Collected ~/cg/c.err
$CA --auto=yes --show-percs=no ~/cg/rust.out > ~/cg/rust.txt 2>&1
$CA --auto=yes --show-percs=no ~/cg/c.out > ~/cg/c.txt 2>&1
$CA --inclusive=yes ~/cg/rust.out 2>/dev/null | sed -n '20,45p' | cut -c1-160
