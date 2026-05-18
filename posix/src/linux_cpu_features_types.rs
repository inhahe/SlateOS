//! `<asm/cpufeatures.h>` — x86_64 CPU feature flag constants.
//!
//! CPUID feature bits identify hardware capabilities of the
//! processor. The kernel uses these to enable optimized code paths,
//! security mitigations, and hardware-accelerated operations.

// ---------------------------------------------------------------------------
// Basic feature flags (CPUID.01H:EDX)
// ---------------------------------------------------------------------------

/// Floating Point Unit on-chip.
pub const X86_FEATURE_FPU: u32 = 0;
/// Virtual Mode Extension.
pub const X86_FEATURE_VME: u32 = 1;
/// Debugging Extension.
pub const X86_FEATURE_DE: u32 = 2;
/// Page Size Extension (4MB pages).
pub const X86_FEATURE_PSE: u32 = 3;
/// Time Stamp Counter.
pub const X86_FEATURE_TSC: u32 = 4;
/// Model-Specific Registers.
pub const X86_FEATURE_MSR: u32 = 5;
/// Physical Address Extension.
pub const X86_FEATURE_PAE: u32 = 6;
/// CMPXCHG8B instruction.
pub const X86_FEATURE_CX8: u32 = 8;
/// On-chip APIC.
pub const X86_FEATURE_APIC: u32 = 9;
/// SYSENTER/SYSEXIT instructions.
pub const X86_FEATURE_SEP: u32 = 11;
/// Page Global Enable.
pub const X86_FEATURE_PGE: u32 = 13;
/// CLFLUSH instruction.
pub const X86_FEATURE_CLFLUSH: u32 = 19;
/// MMX Technology.
pub const X86_FEATURE_MMX: u32 = 23;
/// FXSAVE/FXRSTOR instructions.
pub const X86_FEATURE_FXSR: u32 = 24;
/// SSE (Streaming SIMD Extensions).
pub const X86_FEATURE_SSE: u32 = 25;
/// SSE2.
pub const X86_FEATURE_SSE2: u32 = 26;
/// Hyper-Threading Technology.
pub const X86_FEATURE_HTT: u32 = 28;

// ---------------------------------------------------------------------------
// Extended feature flags (CPUID.01H:ECX)
// ---------------------------------------------------------------------------

/// SSE3.
pub const X86_FEATURE_SSE3: u32 = 32;
/// PCLMULQDQ instruction.
pub const X86_FEATURE_PCLMULQDQ: u32 = 33;
/// SSSE3.
pub const X86_FEATURE_SSSE3: u32 = 41;
/// FMA3 (Fused Multiply-Add).
pub const X86_FEATURE_FMA: u32 = 44;
/// CMPXCHG16B instruction.
pub const X86_FEATURE_CX16: u32 = 45;
/// SSE4.1.
pub const X86_FEATURE_SSE4_1: u32 = 51;
/// SSE4.2.
pub const X86_FEATURE_SSE4_2: u32 = 52;
/// POPCNT instruction.
pub const X86_FEATURE_POPCNT: u32 = 55;
/// AES-NI (hardware AES).
pub const X86_FEATURE_AES: u32 = 57;
/// AVX (Advanced Vector Extensions).
pub const X86_FEATURE_AVX: u32 = 60;
/// RDRAND instruction.
pub const X86_FEATURE_RDRAND: u32 = 62;

// ---------------------------------------------------------------------------
// Extended features (CPUID.07H:EBX)
// ---------------------------------------------------------------------------

/// BMI1 (Bit Manipulation Instruction Set 1).
pub const X86_FEATURE_BMI1: u32 = 64 + 3;
/// AVX2.
pub const X86_FEATURE_AVX2: u32 = 64 + 5;
/// BMI2.
pub const X86_FEATURE_BMI2: u32 = 64 + 8;
/// RDSEED instruction.
pub const X86_FEATURE_RDSEED: u32 = 64 + 18;
/// ADX (multi-precision add-carry).
pub const X86_FEATURE_ADX: u32 = 64 + 19;
/// SHA extensions.
pub const X86_FEATURE_SHA: u32 = 64 + 29;
/// AVX-512 Foundation.
pub const X86_FEATURE_AVX512F: u32 = 64 + 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_features_distinct() {
        let feats = [
            X86_FEATURE_FPU, X86_FEATURE_VME, X86_FEATURE_DE,
            X86_FEATURE_PSE, X86_FEATURE_TSC, X86_FEATURE_MSR,
            X86_FEATURE_PAE, X86_FEATURE_CX8, X86_FEATURE_APIC,
            X86_FEATURE_SEP, X86_FEATURE_PGE, X86_FEATURE_CLFLUSH,
            X86_FEATURE_MMX, X86_FEATURE_FXSR,
            X86_FEATURE_SSE, X86_FEATURE_SSE2, X86_FEATURE_HTT,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_ne!(feats[i], feats[j]);
            }
        }
    }

    #[test]
    fn test_extended_features_distinct() {
        let feats = [
            X86_FEATURE_SSE3, X86_FEATURE_PCLMULQDQ,
            X86_FEATURE_SSSE3, X86_FEATURE_FMA,
            X86_FEATURE_CX16, X86_FEATURE_SSE4_1,
            X86_FEATURE_SSE4_2, X86_FEATURE_POPCNT,
            X86_FEATURE_AES, X86_FEATURE_AVX, X86_FEATURE_RDRAND,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_ne!(feats[i], feats[j]);
            }
        }
    }

    #[test]
    fn test_fpu_is_zero() {
        assert_eq!(X86_FEATURE_FPU, 0);
    }

    #[test]
    fn test_sse_ordering() {
        assert!(X86_FEATURE_SSE < X86_FEATURE_SSE2);
        assert!(X86_FEATURE_SSE2 < X86_FEATURE_SSE3);
        assert!(X86_FEATURE_SSE3 < X86_FEATURE_SSE4_1);
        assert!(X86_FEATURE_SSE4_1 < X86_FEATURE_SSE4_2);
    }

    #[test]
    fn test_avx512_in_extended_range() {
        assert!(X86_FEATURE_AVX512F >= 64);
    }
}
