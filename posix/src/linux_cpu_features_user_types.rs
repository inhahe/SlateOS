//! `<asm/cpufeatures.h>` — CPU feature flag bit positions surfaced
//! through `/proc/cpuinfo`'s "flags" line and `cpuid_count()`.
//!
//! These are the x86-64 "X86_FEATURE_*" constants identifying which
//! architectural and microarchitectural features the running CPU
//! advertises. The list here is the most commonly observed subset.

// ---------------------------------------------------------------------------
// CPUID leaf input values
// ---------------------------------------------------------------------------

pub const CPUID_LEAF_BASIC_INFO: u32 = 0x0000_0000;
pub const CPUID_LEAF_FEATURE_INFO: u32 = 0x0000_0001;
pub const CPUID_LEAF_EXTENDED_FEATURE: u32 = 0x0000_0007;
pub const CPUID_LEAF_TOPOLOGY: u32 = 0x0000_000B;
pub const CPUID_LEAF_EXTENDED_TOPOLOGY: u32 = 0x0000_001F;
pub const CPUID_LEAF_HIGHEST_EXTENDED: u32 = 0x8000_0000;
pub const CPUID_LEAF_EXTENDED_BRAND: u32 = 0x8000_0004;

// ---------------------------------------------------------------------------
// CPUID(1).EDX feature bits — pulled from /arch/x86/include/asm/cpufeatures.h
// ---------------------------------------------------------------------------

pub const X86_FEATURE_FPU: u32 = 0;
pub const X86_FEATURE_TSC: u32 = 4;
pub const X86_FEATURE_MSR: u32 = 5;
pub const X86_FEATURE_PAE: u32 = 6;
pub const X86_FEATURE_APIC: u32 = 9;
pub const X86_FEATURE_PGE: u32 = 13;
pub const X86_FEATURE_CMOV: u32 = 15;
pub const X86_FEATURE_PAT: u32 = 16;
pub const X86_FEATURE_CLFLUSH: u32 = 19;
pub const X86_FEATURE_MMX: u32 = 23;
pub const X86_FEATURE_FXSR: u32 = 24;
pub const X86_FEATURE_SSE: u32 = 25;
pub const X86_FEATURE_SSE2: u32 = 26;
pub const X86_FEATURE_HT: u32 = 28;

// ---------------------------------------------------------------------------
// CPUID(1).ECX feature bits
// ---------------------------------------------------------------------------

pub const X86_FEATURE_SSE3: u32 = 0;
pub const X86_FEATURE_PCLMULQDQ: u32 = 1;
pub const X86_FEATURE_SSSE3: u32 = 9;
pub const X86_FEATURE_FMA: u32 = 12;
pub const X86_FEATURE_SSE4_1: u32 = 19;
pub const X86_FEATURE_SSE4_2: u32 = 20;
pub const X86_FEATURE_POPCNT: u32 = 23;
pub const X86_FEATURE_AES: u32 = 25;
pub const X86_FEATURE_XSAVE: u32 = 26;
pub const X86_FEATURE_OSXSAVE: u32 = 27;
pub const X86_FEATURE_AVX: u32 = 28;
pub const X86_FEATURE_F16C: u32 = 29;
pub const X86_FEATURE_RDRAND: u32 = 30;

// ---------------------------------------------------------------------------
// CPUID(7,0).EBX bits (extended features)
// ---------------------------------------------------------------------------

pub const X86_FEATURE_FSGSBASE: u32 = 0;
pub const X86_FEATURE_BMI1: u32 = 3;
pub const X86_FEATURE_AVX2: u32 = 5;
pub const X86_FEATURE_SMEP: u32 = 7;
pub const X86_FEATURE_BMI2: u32 = 8;
pub const X86_FEATURE_RDSEED: u32 = 18;
pub const X86_FEATURE_ADX: u32 = 19;
pub const X86_FEATURE_SMAP: u32 = 20;
pub const X86_FEATURE_CLFLUSHOPT: u32 = 23;
pub const X86_FEATURE_AVX512F: u32 = 16;
pub const X86_FEATURE_SHA_NI: u32 = 29;

// ---------------------------------------------------------------------------
// /proc/cpuinfo well-known field names
// ---------------------------------------------------------------------------

pub const CPUINFO_FIELD_FLAGS: &str = "flags";
pub const CPUINFO_FIELD_VENDOR_ID: &str = "vendor_id";
pub const CPUINFO_FIELD_MODEL_NAME: &str = "model name";
pub const CPUINFO_PATH: &str = "/proc/cpuinfo";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpuid_leaf_constants_known() {
        assert_eq!(CPUID_LEAF_BASIC_INFO, 0);
        assert_eq!(CPUID_LEAF_FEATURE_INFO, 1);
        assert_eq!(CPUID_LEAF_EXTENDED_FEATURE, 7);
        assert_eq!(CPUID_LEAF_HIGHEST_EXTENDED, 0x8000_0000);
        // Extended brand string spans 0x8000_0002..0x8000_0004.
        assert_eq!(CPUID_LEAF_EXTENDED_BRAND, 0x8000_0004);
    }

    #[test]
    fn test_edx_bits_within_word() {
        for b in [
            X86_FEATURE_FPU,
            X86_FEATURE_TSC,
            X86_FEATURE_MSR,
            X86_FEATURE_PAE,
            X86_FEATURE_APIC,
            X86_FEATURE_CMOV,
            X86_FEATURE_PAT,
            X86_FEATURE_CLFLUSH,
            X86_FEATURE_MMX,
            X86_FEATURE_FXSR,
            X86_FEATURE_SSE,
            X86_FEATURE_SSE2,
            X86_FEATURE_HT,
        ] {
            assert!(b < 32);
        }
    }

    #[test]
    fn test_sse_family_ordered_in_edx() {
        // FXSR < SSE < SSE2 in CPUID(1).EDX (chronological order).
        assert!(X86_FEATURE_FXSR < X86_FEATURE_SSE);
        assert!(X86_FEATURE_SSE < X86_FEATURE_SSE2);
    }

    #[test]
    fn test_ecx_bits_within_word() {
        for b in [
            X86_FEATURE_SSE3,
            X86_FEATURE_PCLMULQDQ,
            X86_FEATURE_SSSE3,
            X86_FEATURE_FMA,
            X86_FEATURE_SSE4_1,
            X86_FEATURE_SSE4_2,
            X86_FEATURE_POPCNT,
            X86_FEATURE_AES,
            X86_FEATURE_XSAVE,
            X86_FEATURE_OSXSAVE,
            X86_FEATURE_AVX,
            X86_FEATURE_F16C,
            X86_FEATURE_RDRAND,
        ] {
            assert!(b < 32);
        }
    }

    #[test]
    fn test_xsave_osxsave_consecutive() {
        // OSXSAVE = XSAVE + 1.
        assert_eq!(X86_FEATURE_OSXSAVE, X86_FEATURE_XSAVE + 1);
    }

    #[test]
    fn test_sse4_pair_consecutive() {
        assert_eq!(X86_FEATURE_SSE4_2, X86_FEATURE_SSE4_1 + 1);
    }

    #[test]
    fn test_ebx7_bits_within_word() {
        for b in [
            X86_FEATURE_FSGSBASE,
            X86_FEATURE_BMI1,
            X86_FEATURE_AVX2,
            X86_FEATURE_SMEP,
            X86_FEATURE_BMI2,
            X86_FEATURE_RDSEED,
            X86_FEATURE_ADX,
            X86_FEATURE_SMAP,
            X86_FEATURE_CLFLUSHOPT,
            X86_FEATURE_AVX512F,
            X86_FEATURE_SHA_NI,
        ] {
            assert!(b < 32);
        }
        // BMI1 < BMI2 < SHA-NI as a sanity check.
        assert!(X86_FEATURE_BMI1 < X86_FEATURE_BMI2);
        assert!(X86_FEATURE_BMI2 < X86_FEATURE_SHA_NI);
    }

    #[test]
    fn test_cpuinfo_field_names_distinct() {
        assert_ne!(CPUINFO_FIELD_FLAGS, CPUINFO_FIELD_VENDOR_ID);
        assert_ne!(CPUINFO_FIELD_FLAGS, CPUINFO_FIELD_MODEL_NAME);
        assert!(CPUINFO_PATH.starts_with("/proc/"));
    }
}
