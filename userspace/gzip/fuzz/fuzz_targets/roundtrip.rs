//! Compression, at the level the first byte picks: whatever the input,
//! deflate must not panic, and what it writes must decompress to the input.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&pick, input)) = data.split_first() else {
        return;
    };
    let level = i32::from(pick % 9) + 1;
    let compressed = gzip::compress(input, level).expect("levels 1-9 cannot fail in memory");
    let (out, err, status) = gzip::zcat(&compressed, 0);
    assert_eq!(status, 0, "{}", String::from_utf8_lossy(&err));
    assert!(out == input, "level {level}: {} bytes came back as {}", input.len(), out.len());
});
