//! `<linux/rkisp1-config.h>` — Rockchip ISP1 V4L2 parameter constants.
//!
//! Rockchip RK33xx/RV1109 ISP driver (rkisp1) exposes a per-frame
//! parameter buffer over V4L2. Userspace 3A / tuning libraries
//! consume these module-enable bits and dimensions.

// ---------------------------------------------------------------------------
// ISP module enable bits (struct rkisp1_params_cfg.module_en_update / cfg)
// ---------------------------------------------------------------------------

/// Differential pixel correction (DPCC).
pub const RKISP1_CIF_ISP_MODULE_DPCC: u32 = 1 << 0;
/// Black-level subtraction (BLS).
pub const RKISP1_CIF_ISP_MODULE_BLS: u32 = 1 << 1;
/// Sensor degamma.
pub const RKISP1_CIF_ISP_MODULE_SDG: u32 = 1 << 2;
/// Histogram statistics.
pub const RKISP1_CIF_ISP_MODULE_HST: u32 = 1 << 3;
/// Lens-shading correction.
pub const RKISP1_CIF_ISP_MODULE_LSC: u32 = 1 << 4;
/// Auto-white-balance gains.
pub const RKISP1_CIF_ISP_MODULE_AWB_GAIN: u32 = 1 << 5;
/// Frame-pattern filter.
pub const RKISP1_CIF_ISP_MODULE_FLT: u32 = 1 << 6;
/// Bayer demosaic.
pub const RKISP1_CIF_ISP_MODULE_BDM: u32 = 1 << 7;
/// Color-correction matrix.
pub const RKISP1_CIF_ISP_MODULE_CTK: u32 = 1 << 8;
/// Global tone mapping.
pub const RKISP1_CIF_ISP_MODULE_GOC: u32 = 1 << 9;
/// Color processing.
pub const RKISP1_CIF_ISP_MODULE_CPROC: u32 = 1 << 10;
/// AF statistics.
pub const RKISP1_CIF_ISP_MODULE_AFC: u32 = 1 << 11;
/// AWB statistics.
pub const RKISP1_CIF_ISP_MODULE_AWB: u32 = 1 << 12;
/// Image-effect block.
pub const RKISP1_CIF_ISP_MODULE_IE: u32 = 1 << 13;
/// AE statistics.
pub const RKISP1_CIF_ISP_MODULE_AEC: u32 = 1 << 14;
/// Wide-dynamic-range gamma.
pub const RKISP1_CIF_ISP_MODULE_WDR: u32 = 1 << 15;
/// De-mosaic purple-fringing.
pub const RKISP1_CIF_ISP_MODULE_DPF: u32 = 1 << 16;
/// Strength control on DPF.
pub const RKISP1_CIF_ISP_MODULE_DPF_STRENGTH: u32 = 1 << 17;

// ---------------------------------------------------------------------------
// Dimensions / table sizes
// ---------------------------------------------------------------------------

/// Lens-shading correction sector grid (h).
pub const RKISP1_CIF_ISP_LSC_SECTORS_MAX: u32 = 17;
/// Number of histogram bins.
pub const RKISP1_CIF_ISP_HIST_BIN_N_MAX: u32 = 16;
/// Number of AWB grid zones (one dimension).
pub const RKISP1_CIF_ISP_AE_MEAN_MAX: u32 = 81;
/// Number of AF measurement windows.
pub const RKISP1_CIF_ISP_AFM_MAX_WINDOWS: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_bits_distinct_powers_of_two() {
        let modules = [
            RKISP1_CIF_ISP_MODULE_DPCC,
            RKISP1_CIF_ISP_MODULE_BLS,
            RKISP1_CIF_ISP_MODULE_SDG,
            RKISP1_CIF_ISP_MODULE_HST,
            RKISP1_CIF_ISP_MODULE_LSC,
            RKISP1_CIF_ISP_MODULE_AWB_GAIN,
            RKISP1_CIF_ISP_MODULE_FLT,
            RKISP1_CIF_ISP_MODULE_BDM,
            RKISP1_CIF_ISP_MODULE_CTK,
            RKISP1_CIF_ISP_MODULE_GOC,
            RKISP1_CIF_ISP_MODULE_CPROC,
            RKISP1_CIF_ISP_MODULE_AFC,
            RKISP1_CIF_ISP_MODULE_AWB,
            RKISP1_CIF_ISP_MODULE_IE,
            RKISP1_CIF_ISP_MODULE_AEC,
            RKISP1_CIF_ISP_MODULE_WDR,
            RKISP1_CIF_ISP_MODULE_DPF,
            RKISP1_CIF_ISP_MODULE_DPF_STRENGTH,
        ];
        for &m in &modules {
            assert!(m.is_power_of_two());
        }
        for i in 0..modules.len() {
            for j in (i + 1)..modules.len() {
                assert_ne!(modules[i], modules[j]);
            }
        }
    }

    #[test]
    fn test_module_bits_within_u32() {
        // All module bits must fit within 32 bits (the on-wire
        // module_en_update field). Documents the assumption.
        let all = RKISP1_CIF_ISP_MODULE_DPCC
            | RKISP1_CIF_ISP_MODULE_BLS
            | RKISP1_CIF_ISP_MODULE_SDG
            | RKISP1_CIF_ISP_MODULE_HST
            | RKISP1_CIF_ISP_MODULE_LSC
            | RKISP1_CIF_ISP_MODULE_AWB_GAIN
            | RKISP1_CIF_ISP_MODULE_FLT
            | RKISP1_CIF_ISP_MODULE_BDM
            | RKISP1_CIF_ISP_MODULE_CTK
            | RKISP1_CIF_ISP_MODULE_GOC
            | RKISP1_CIF_ISP_MODULE_CPROC
            | RKISP1_CIF_ISP_MODULE_AFC
            | RKISP1_CIF_ISP_MODULE_AWB
            | RKISP1_CIF_ISP_MODULE_IE
            | RKISP1_CIF_ISP_MODULE_AEC
            | RKISP1_CIF_ISP_MODULE_WDR
            | RKISP1_CIF_ISP_MODULE_DPF
            | RKISP1_CIF_ISP_MODULE_DPF_STRENGTH;
        assert!(all <= u32::MAX);
        assert!(all > 0);
    }

    #[test]
    fn test_dimensions_reasonable() {
        assert!(RKISP1_CIF_ISP_LSC_SECTORS_MAX >= 1);
        assert!(RKISP1_CIF_ISP_HIST_BIN_N_MAX.is_power_of_two());
        assert!(RKISP1_CIF_ISP_AE_MEAN_MAX >= 1);
        assert!(RKISP1_CIF_ISP_AFM_MAX_WINDOWS >= 1);
    }
}
