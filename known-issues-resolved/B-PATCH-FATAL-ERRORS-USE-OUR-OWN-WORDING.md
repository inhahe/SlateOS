## B-PATCH-FATAL-ERRORS-USE-OUR-OWN-WORDING (lane B, 2026-09-12) — FIXED

Two fatal diagnostics, both this build's own phrasing:

| input | GNU | ours |
|---|---|---|
| missing patch file | `patch: **** Can't open patch file P : No such file or directory` | `patch: P: No such file or directory (os error 2)` |
| `-p abc` | `patch: **** strip count abc is not a number` | `patch: invalid strip count: abc` |

GNU marks a fatal error with `****` and puts a space before the colon in the
first. Ours also leaked Rust's `(os error 2)` tail, which no C program prints.

**The reason tail now comes from `coreutils::errmsg::strerror` rather than a
third private copy.** `strings.rs` carries a `clean_reason` that trims
`" (os error "` off `Display`, and my first attempt here was a fourth one.
The shared helper does better than trimming: it returns the POSIX text the
errno actually has, which is what a C program prints, rather than whatever Rust
chose to say before the parenthesis.

Two tests were retargeted: `parse_invalid_p_value_errors` and
`parse_invalid_pn_value_errors` both asserted `invalid strip count`, our phrase.

`patch-diff.sh`: 49 passed / 16 differed to **51 / 14**.
