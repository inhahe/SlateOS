//! Every decoder gzip has, on input a stranger wrote: whatever the bytes
//! are -- gzip, pkzip, `pack`, `compress`, LZH, or none of them -- `gzip
//! -dc` must end with output and a diagnostic, never a panic or a hang.
//! `-dcf` and `-dcff` take the other paths through the header.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    for force in 0..=2 {
        let _ = gzip::zcat(data, force);
    }
});
