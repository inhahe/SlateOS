#!/bin/bash
# Run every lane F fuzz target in parallel for SECS seconds each (default 1200),
# at low priority, then report what each found. See README.md.
#
# The address sanitizer only where rav1d's `unsafe` code is reachable (image,
# for AVIF; video, for AV1); the all-safe targets run without it, in a target
# directory of their own so the two builds do not invalidate each other.
export PATH="$HOME/.cargo/bin:$PATH"
secs="${1:-1200}"
cd "$HOME/fuzz/tree" || exit 1
mkdir -p "$HOME/fuzz/logs"
opts=(-max_total_time="$secs" -rss_limit_mb=4096 -timeout=20 -print_final_stats=1)
cargo +nightly fuzz build -O -s none --target-dir "$HOME/fuzz/target-none" \
  < /dev/null > "$HOME/fuzz/logs/build-none.log" 2>&1 || exit 1
for t in image video; do
  nice -n 10 cargo +nightly fuzz run -O "$t" -- "${opts[@]}" \
    < /dev/null > "$HOME/fuzz/logs/$t.log" 2>&1 &
done
for t in sound matroska vp8 vp9; do
  nice -n 10 cargo +nightly fuzz run -O -s none --target-dir "$HOME/fuzz/target-none" "$t" -- "${opts[@]}" \
    < /dev/null > "$HOME/fuzz/logs/$t.log" 2>&1 &
done
wait
for t in image video sound matroska vp8 vp9; do
  echo "=== $t"
  grep -a -E "^stat::(number_of_executed_units|new_units_added)|SUMMARY|panicked at|Test unit written|ERROR: libFuzzer" \
    "$HOME/fuzz/logs/$t.log" | head -8
  ls "fuzz/artifacts/$t" 2>/dev/null | head -5
done
