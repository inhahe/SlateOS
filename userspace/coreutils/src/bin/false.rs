//! false -- do nothing, unsuccessfully.
//!
//! Usage: false
//!   Always exits with status 1.

use std::process;

fn main() {
    process::exit(1);
}
