//! `<stdbit.h>` -- C23's bit utilities (C23 7.18): for each of the five
//! standard unsigned types, how many zeros or ones a value has at its top or
//! bottom, where the first of them is, how many it has in all, whether it is a
//! power of two, how many bits it needs, and the powers of two either side of
//! it.
//!
//! musl's headers have no `<stdbit.h>`: `posix/include/stdbit.h`, the header
//! overlay C is compiled with here (design-decisions §1141), declares these
//! seventy functions and the type-generic macros over them. Each is one of
//! Rust's integer methods, and none can fail. Where the power of two
//! `stdc_bit_ceil` would return does not fit the type it answers 0, as
//! glibc's does; C23 leaves the case unspecified.
//!
//! The answers are glibc 2.39's, which are the standard's: replayed in the
//! tests from `stdbit_oracle.txt` (`posix/tools/oracle/stdbit_harness.py`),
//! every value of `unsigned char` and `unsigned short`, and the edges and a
//! sample of the wider types.

/// One type's fourteen functions. The counts are at most the type's width,
/// so the `wrapping_add(1)`s never wrap.
macro_rules! stdbit {
    ($t:ty, $c:literal,
     $leading_zeros:ident, $leading_ones:ident, $trailing_zeros:ident, $trailing_ones:ident,
     $first_leading_zero:ident, $first_leading_one:ident, $first_trailing_zero:ident,
     $first_trailing_one:ident, $count_zeros:ident, $count_ones:ident,
     $has_single_bit:ident, $bit_width:ident, $bit_floor:ident, $bit_ceil:ident) => {
        #[doc = concat!("How many 0 bits an `", $c, "` has above its highest 1 -- all of them, for 0.")]
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $leading_zeros(x: $t) -> u32 {
            x.leading_zeros()
        }

        #[doc = concat!("How many 1 bits an `", $c, "` has above its highest 0.")]
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $leading_ones(x: $t) -> u32 {
            x.leading_ones()
        }

        #[doc = concat!("How many 0 bits an `", $c, "` has below its lowest 1 -- all of them, for 0.")]
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $trailing_zeros(x: $t) -> u32 {
            x.trailing_zeros()
        }

        #[doc = concat!("How many 1 bits an `", $c, "` has below its lowest 0.")]
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $trailing_ones(x: $t) -> u32 {
            x.trailing_ones()
        }

        #[doc = concat!("Where an `", $c, "`'s highest 0 bit is, counting the top bit as 1; 0 if it has none.")]
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $first_leading_zero(x: $t) -> u32 {
            if x == <$t>::MAX { 0 } else { x.leading_ones().wrapping_add(1) }
        }

        #[doc = concat!("Where an `", $c, "`'s highest 1 bit is, counting the top bit as 1; 0 for 0.")]
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $first_leading_one(x: $t) -> u32 {
            if x == 0 { 0 } else { x.leading_zeros().wrapping_add(1) }
        }

        #[doc = concat!("Where an `", $c, "`'s lowest 0 bit is, counting the bottom bit as 1; 0 if it has none.")]
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $first_trailing_zero(x: $t) -> u32 {
            if x == <$t>::MAX { 0 } else { x.trailing_ones().wrapping_add(1) }
        }

        #[doc = concat!("Where an `", $c, "`'s lowest 1 bit is, counting the bottom bit as 1; 0 for 0.")]
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $first_trailing_one(x: $t) -> u32 {
            if x == 0 { 0 } else { x.trailing_zeros().wrapping_add(1) }
        }

        #[doc = concat!("How many 0 bits an `", $c, "` has.")]
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $count_zeros(x: $t) -> u32 {
            x.count_zeros()
        }

        #[doc = concat!("How many 1 bits an `", $c, "` has.")]
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $count_ones(x: $t) -> u32 {
            x.count_ones()
        }

        #[doc = concat!("Whether an `", $c, "` has exactly one 1 bit: a power of two.")]
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $has_single_bit(x: $t) -> bool {
            x.is_power_of_two()
        }

        #[doc = concat!("How many bits an `", $c, "` needs: 0 for 0, else one more than its highest 1's index.")]
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $bit_width(x: $t) -> u32 {
            x.checked_ilog2().map_or(0, |l| l.wrapping_add(1))
        }

        #[doc = concat!("The largest power of two not above an `", $c, "`; 0 for 0.")]
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $bit_floor(x: $t) -> $t {
            x.checked_ilog2()
                .and_then(|l| <$t>::checked_shl(1, l))
                .unwrap_or(0)
        }

        #[doc = concat!("The smallest power of two not below an `", $c, "` -- 1 for 0 -- or 0 where that does not fit.")]
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $bit_ceil(x: $t) -> $t {
            if x <= 1 { 1 } else { x.checked_next_power_of_two().unwrap_or(0) }
        }
    };
}

stdbit!(
    u8,
    "unsigned char",
    stdc_leading_zeros_uc,
    stdc_leading_ones_uc,
    stdc_trailing_zeros_uc,
    stdc_trailing_ones_uc,
    stdc_first_leading_zero_uc,
    stdc_first_leading_one_uc,
    stdc_first_trailing_zero_uc,
    stdc_first_trailing_one_uc,
    stdc_count_zeros_uc,
    stdc_count_ones_uc,
    stdc_has_single_bit_uc,
    stdc_bit_width_uc,
    stdc_bit_floor_uc,
    stdc_bit_ceil_uc
);
stdbit!(
    u16,
    "unsigned short",
    stdc_leading_zeros_us,
    stdc_leading_ones_us,
    stdc_trailing_zeros_us,
    stdc_trailing_ones_us,
    stdc_first_leading_zero_us,
    stdc_first_leading_one_us,
    stdc_first_trailing_zero_us,
    stdc_first_trailing_one_us,
    stdc_count_zeros_us,
    stdc_count_ones_us,
    stdc_has_single_bit_us,
    stdc_bit_width_us,
    stdc_bit_floor_us,
    stdc_bit_ceil_us
);
stdbit!(
    u32,
    "unsigned int",
    stdc_leading_zeros_ui,
    stdc_leading_ones_ui,
    stdc_trailing_zeros_ui,
    stdc_trailing_ones_ui,
    stdc_first_leading_zero_ui,
    stdc_first_leading_one_ui,
    stdc_first_trailing_zero_ui,
    stdc_first_trailing_one_ui,
    stdc_count_zeros_ui,
    stdc_count_ones_ui,
    stdc_has_single_bit_ui,
    stdc_bit_width_ui,
    stdc_bit_floor_ui,
    stdc_bit_ceil_ui
);
stdbit!(
    u64,
    "unsigned long",
    stdc_leading_zeros_ul,
    stdc_leading_ones_ul,
    stdc_trailing_zeros_ul,
    stdc_trailing_ones_ul,
    stdc_first_leading_zero_ul,
    stdc_first_leading_one_ul,
    stdc_first_trailing_zero_ul,
    stdc_first_trailing_one_ul,
    stdc_count_zeros_ul,
    stdc_count_ones_ul,
    stdc_has_single_bit_ul,
    stdc_bit_width_ul,
    stdc_bit_floor_ul,
    stdc_bit_ceil_ul
);
stdbit!(
    u64,
    "unsigned long long",
    stdc_leading_zeros_ull,
    stdc_leading_ones_ull,
    stdc_trailing_zeros_ull,
    stdc_trailing_ones_ull,
    stdc_first_leading_zero_ull,
    stdc_first_leading_one_ull,
    stdc_first_trailing_zero_ull,
    stdc_first_trailing_one_ull,
    stdc_count_zeros_ull,
    stdc_count_ones_ull,
    stdc_has_single_bit_ull,
    stdc_bit_width_ull,
    stdc_bit_floor_ull,
    stdc_bit_ceil_ull
);

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    /// glibc 2.39's answers: `<function> <x> = <result>` in hex, and for
    /// the two narrow types `digest <function> <FNV-1a of every result>`.
    const ORACLE: &str = include_str!("stdbit_oracle.txt");

    /// Each of the seventy by name, on an argument truncated to its type,
    /// its result widened -- what the oracle's C prints.
    macro_rules! by_name {
        ($name:expr, $x:expr; $($t:ty => [$($f:ident),* $(,)?]);* $(;)?) => {{
            let (name, x): (&str, u64) = ($name, $x);
            $($(
                if name == stringify!($f) {
                    #[allow(clippy::cast_possible_truncation)]
                    return Some(u64::from($f(x as $t)));
                }
            )*)*
            None
        }};
    }

    fn call(name: &str, x: u64) -> Option<u64> {
        by_name!(name, x;
            u8 => [stdc_leading_zeros_uc, stdc_leading_ones_uc, stdc_trailing_zeros_uc,
                   stdc_trailing_ones_uc, stdc_first_leading_zero_uc, stdc_first_leading_one_uc,
                   stdc_first_trailing_zero_uc, stdc_first_trailing_one_uc, stdc_count_zeros_uc,
                   stdc_count_ones_uc, stdc_has_single_bit_uc, stdc_bit_width_uc,
                   stdc_bit_floor_uc, stdc_bit_ceil_uc];
            u16 => [stdc_leading_zeros_us, stdc_leading_ones_us, stdc_trailing_zeros_us,
                    stdc_trailing_ones_us, stdc_first_leading_zero_us, stdc_first_leading_one_us,
                    stdc_first_trailing_zero_us, stdc_first_trailing_one_us, stdc_count_zeros_us,
                    stdc_count_ones_us, stdc_has_single_bit_us, stdc_bit_width_us,
                    stdc_bit_floor_us, stdc_bit_ceil_us];
            u32 => [stdc_leading_zeros_ui, stdc_leading_ones_ui, stdc_trailing_zeros_ui,
                    stdc_trailing_ones_ui, stdc_first_leading_zero_ui, stdc_first_leading_one_ui,
                    stdc_first_trailing_zero_ui, stdc_first_trailing_one_ui, stdc_count_zeros_ui,
                    stdc_count_ones_ui, stdc_has_single_bit_ui, stdc_bit_width_ui,
                    stdc_bit_floor_ui, stdc_bit_ceil_ui];
            u64 => [stdc_leading_zeros_ul, stdc_leading_ones_ul, stdc_trailing_zeros_ul,
                    stdc_trailing_ones_ul, stdc_first_leading_zero_ul, stdc_first_leading_one_ul,
                    stdc_first_trailing_zero_ul, stdc_first_trailing_one_ul, stdc_count_zeros_ul,
                    stdc_count_ones_ul, stdc_has_single_bit_ul, stdc_bit_width_ul,
                    stdc_bit_floor_ul, stdc_bit_ceil_ul];
            u64 => [stdc_leading_zeros_ull, stdc_leading_ones_ull, stdc_trailing_zeros_ull,
                    stdc_trailing_ones_ull, stdc_first_leading_zero_ull,
                    stdc_first_leading_one_ull, stdc_first_trailing_zero_ull,
                    stdc_first_trailing_one_ull, stdc_count_zeros_ull, stdc_count_ones_ull,
                    stdc_has_single_bit_ull, stdc_bit_width_ull, stdc_bit_floor_ull,
                    stdc_bit_ceil_ull];
        )
    }

    /// Every sampled call is glibc's answer, and every function of the two
    /// narrow types gives glibc's answer at every value.
    #[test]
    fn every_answer_is_glibcs() {
        let (mut calls, mut digests) = (0, 0);
        for line in ORACLE
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
        {
            if let Some(rest) = line.strip_prefix("digest ") {
                let (name, want) = rest.split_once(' ').unwrap();
                let want = u64::from_str_radix(want, 16).unwrap();
                let max: u64 = if name.ends_with("_uc") { 0xff } else { 0xffff };
                let mut h: u64 = 0xcbf2_9ce4_8422_2325;
                for x in 0..=max {
                    let r = call(name, x).unwrap_or_else(|| panic!("no function {name}"));
                    h = (h ^ r).wrapping_mul(0x0000_0100_0000_01b3);
                }
                assert_eq!(h, want, "{name}: some value's answer is not glibc's");
                digests += 1;
            } else {
                let (lhs, want) = line.split_once(" = ").unwrap();
                let (name, x) = lhs.split_once(' ').unwrap();
                let x = u64::from_str_radix(x, 16).unwrap();
                let want = u64::from_str_radix(want, 16).unwrap();
                let got = call(name, x).unwrap_or_else(|| panic!("no function {name}"));
                assert_eq!(got, want, "{name}({x:#x})");
                calls += 1;
            }
        }
        // Fourteen functions for each of the two narrow types, and a sample
        // for all five: a truncated file would pass with fewer.
        assert_eq!(digests, 28);
        assert!(calls > 13_000, "{calls} sampled calls");
    }

    /// The standard's examples, by hand: the edges of each family.
    #[test]
    fn the_edges() {
        assert_eq!(stdc_leading_zeros_ui(0), 32);
        assert_eq!(stdc_trailing_zeros_ull(0), 64);
        assert_eq!(stdc_first_leading_one_uc(0x10), 4);
        assert_eq!(stdc_first_trailing_one_us(0x10), 5);
        assert_eq!(stdc_first_leading_zero_uc(0xff), 0);
        assert_eq!(stdc_first_trailing_zero_uc(0xfe), 1);
        assert!(stdc_has_single_bit_ul(1 << 40) && !stdc_has_single_bit_ul(0));
        assert_eq!(stdc_bit_width_ui(0), 0);
        assert_eq!(stdc_bit_width_ui(255), 8);
        assert_eq!(stdc_bit_floor_us(0), 0);
        assert_eq!(stdc_bit_floor_us(1000), 512);
        assert_eq!(stdc_bit_ceil_uc(0), 1);
        assert_eq!(stdc_bit_ceil_uc(128), 128);
        assert_eq!(stdc_bit_ceil_uc(129), 0, "does not fit: glibc's 0");
        assert_eq!(stdc_bit_ceil_ull(u64::MAX), 0);
    }
}
