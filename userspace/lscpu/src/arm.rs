//! `lscpu-arm.c`: ARM implementer and part numbers into names, and the
//! `rXpY` stepping -- run for every architecture, as upstream runs it, where
//! on anything but ARM it finds no `0x` implementer and changes nothing.
//!
//! The tables are upstream's, generated from `lscpu-arm.c` 2.39.3 and kept
//! in its order, with the five parts Ubuntu 24.04 adds to it by backporting
//! later upstream commits (LP #2111723, #2123886): a lookup takes the first
//! entry with the number and stops at the `-1` that ends each table.

use crate::dmi;
use crate::types::{CpuType, Cxt};

/// `struct id_part`.
struct IdPart {
    id: i32,
    name: &'static str,
}

/// `struct hw_impl`.
struct HwImpl {
    id: i32,
    parts: &'static [IdPart],
    name: &'static str,
}

/// `-EINVAL`.
const NEG_EINVAL: i32 = -22;

/// `parse_id(str)`: a number written `0x...`, read by `strtol` with base 0
/// and kept as an `int`; `-EINVAL` without the `0x`, or when `strtol`
/// fails.
fn parse_id(s: Option<&[u8]>) -> i32 {
    let Some(s) = s else {
        return NEG_EINVAL;
    };
    if !s.starts_with(b"0x") {
        return NEG_EINVAL;
    }
    let Some(sc) = ulstrutils::scan_integer(s, 0) else {
        return NEG_EINVAL;
    };
    // Past `long` is ERANGE.
    let limit = if sc.negative {
        1u128 << 63
    } else {
        (1u128 << 63) - 1
    };
    if sc.saturated || sc.magnitude > limit {
        return NEG_EINVAL;
    }
    let value = i128::try_from(sc.magnitude).unwrap_or(0);
    let value = if sc.negative {
        value.wrapping_neg()
    } else {
        value
    };
    // `(int) strtol(...)`.
    value as i32
}

/// `parse_implementer_id(ct)`: parsed once, and remembered -- `-EINVAL`
/// included.
fn implementer_id(ct: &mut CpuType) -> i32 {
    if ct.vendor_id != 0 {
        return ct.vendor_id;
    }
    ct.vendor_id = parse_id(ct.vendor.as_deref());
    ct.vendor_id
}

/// `arm_ids_decode(ct)`: the vendor's name, and the model's.
fn ids_decode(ct: &mut CpuType) {
    let imp = implementer_id(ct);
    if imp <= 0 {
        return;
    }
    let mut parts: Option<&'static [IdPart]> = None;
    for hw in HW_IMPLEMENTER.iter().take_while(|h| h.id != -1) {
        if hw.id == imp {
            parts = Some(hw.parts);
            ct.vendor = Some(hw.name.as_bytes().to_vec());
            break;
        }
    }
    let Some(parts) = parts else {
        return;
    };
    let part = parse_id(ct.model.as_deref());
    if part <= 0 {
        return;
    }
    if let Some(p) = parts
        .iter()
        .take_while(|p| p.id != -1)
        .find(|p| p.id == part)
    {
        ct.modelname = Some(p.name.as_bytes().to_vec());
    }
}

/// `strtol(s, &end, base)` as `arm_rXpY_decode` checks it: the value cut to
/// `int`, or `None` when nothing was converted or it overflowed `long`.
fn strtol_int(s: &[u8], base: u32) -> Option<i32> {
    let sc = ulstrutils::scan_integer(s, base)?;
    let limit = if sc.negative {
        1u128 << 63
    } else {
        (1u128 << 63) - 1
    };
    if sc.saturated || sc.magnitude > limit {
        return None;
    }
    let value = i128::try_from(sc.magnitude).ok()?;
    Some(
        (if sc.negative {
            value.wrapping_neg()
        } else {
            value
        }) as i32,
    )
}

/// `arm_rXpY_decode(ct)`: an ARM Ltd. core's stepping as `r<variant>p<revision>`,
/// cut to 7 bytes as upstream's 8-byte buffer cuts it.
fn rxpy_decode(ct: &mut CpuType) {
    let imp = implementer_id(ct);
    if imp != 0x41 {
        return;
    }
    let (Some(revision), Some(stepping)) = (ct.revision.as_deref(), ct.stepping.as_deref()) else {
        return;
    };
    let Some(revision) = strtol_int(revision, 10) else {
        return;
    };
    let Some(variant) = strtol_int(stepping, 0) else {
        return;
    };
    let mut text = format!("r{variant}p{revision}").into_bytes();
    text.truncate(7);
    ct.stepping = Some(text);
}

/// `arm_decode(cxt, ct)`.
fn decode(cxt_noalive: bool, is_cluster: bool, ct: &mut CpuType) {
    if !cxt_noalive && dmi::readable() {
        dmi::decode_cputype(ct);
    }
    ids_decode(ct);
    rxpy_decode(ct);
    if !cxt_noalive && is_cluster {
        ct.nr_socket_on_cluster = dmi::physical_sockets();
    }
}

/// `is_cluster_arm(cxt)`: an aarch64 machine, described live, with one CPU
/// type and no ACPI PPTT table -- whose "sockets" are then really clusters.
fn is_cluster_arm(cxt: &Cxt) -> bool {
    !cxt.noalive
        && cxt.arch.name == b"aarch64"
        && std::fs::metadata("/sys/firmware/acpi/tables/PPTT").is_err()
        && cxt.cputypes.len() == 1
}

/// `lscpu_decode_arm(cxt)`.
pub fn decode_arm(cxt: &mut Cxt) {
    cxt.is_cluster = is_cluster_arm(cxt);
    let (noalive, is_cluster) = (cxt.noalive, cxt.is_cluster);
    for ct in &mut cxt.cputypes {
        decode(noalive, is_cluster, ct);
    }
}

/// `arm_part[]`.
const ARM_PART: &[IdPart] = &[
    IdPart {
        id: 0x810,
        name: "ARM810",
    },
    IdPart {
        id: 0x920,
        name: "ARM920",
    },
    IdPart {
        id: 0x922,
        name: "ARM922",
    },
    IdPart {
        id: 0x926,
        name: "ARM926",
    },
    IdPart {
        id: 0x940,
        name: "ARM940",
    },
    IdPart {
        id: 0x946,
        name: "ARM946",
    },
    IdPart {
        id: 0x966,
        name: "ARM966",
    },
    IdPart {
        id: 0xa20,
        name: "ARM1020",
    },
    IdPart {
        id: 0xa22,
        name: "ARM1022",
    },
    IdPart {
        id: 0xa26,
        name: "ARM1026",
    },
    IdPart {
        id: 0xb02,
        name: "ARM11 MPCore",
    },
    IdPart {
        id: 0xb36,
        name: "ARM1136",
    },
    IdPart {
        id: 0xb56,
        name: "ARM1156",
    },
    IdPart {
        id: 0xb76,
        name: "ARM1176",
    },
    IdPart {
        id: 0xc05,
        name: "Cortex-A5",
    },
    IdPart {
        id: 0xc07,
        name: "Cortex-A7",
    },
    IdPart {
        id: 0xc08,
        name: "Cortex-A8",
    },
    IdPart {
        id: 0xc09,
        name: "Cortex-A9",
    },
    IdPart {
        id: 0xc0d,
        name: "Cortex-A17",
    },
    IdPart {
        id: 0xc0f,
        name: "Cortex-A15",
    },
    IdPart {
        id: 0xc0e,
        name: "Cortex-A17",
    },
    IdPart {
        id: 0xc14,
        name: "Cortex-R4",
    },
    IdPart {
        id: 0xc15,
        name: "Cortex-R5",
    },
    IdPart {
        id: 0xc17,
        name: "Cortex-R7",
    },
    IdPart {
        id: 0xc18,
        name: "Cortex-R8",
    },
    IdPart {
        id: 0xc20,
        name: "Cortex-M0",
    },
    IdPart {
        id: 0xc21,
        name: "Cortex-M1",
    },
    IdPart {
        id: 0xc23,
        name: "Cortex-M3",
    },
    IdPart {
        id: 0xc24,
        name: "Cortex-M4",
    },
    IdPart {
        id: 0xc27,
        name: "Cortex-M7",
    },
    IdPart {
        id: 0xc60,
        name: "Cortex-M0+",
    },
    IdPart {
        id: 0xd01,
        name: "Cortex-A32",
    },
    IdPart {
        id: 0xd02,
        name: "Cortex-A34",
    },
    IdPart {
        id: 0xd03,
        name: "Cortex-A53",
    },
    IdPart {
        id: 0xd04,
        name: "Cortex-A35",
    },
    IdPart {
        id: 0xd05,
        name: "Cortex-A55",
    },
    IdPart {
        id: 0xd06,
        name: "Cortex-A65",
    },
    IdPart {
        id: 0xd07,
        name: "Cortex-A57",
    },
    IdPart {
        id: 0xd08,
        name: "Cortex-A72",
    },
    IdPart {
        id: 0xd09,
        name: "Cortex-A73",
    },
    IdPart {
        id: 0xd0a,
        name: "Cortex-A75",
    },
    IdPart {
        id: 0xd0b,
        name: "Cortex-A76",
    },
    IdPart {
        id: 0xd0c,
        name: "Neoverse-N1",
    },
    IdPart {
        id: 0xd0d,
        name: "Cortex-A77",
    },
    IdPart {
        id: 0xd0e,
        name: "Cortex-A76AE",
    },
    IdPart {
        id: 0xd13,
        name: "Cortex-R52",
    },
    IdPart {
        id: 0xd15,
        name: "Cortex-R82",
    },
    IdPart {
        id: 0xd16,
        name: "Cortex-R52+",
    },
    IdPart {
        id: 0xd20,
        name: "Cortex-M23",
    },
    IdPart {
        id: 0xd21,
        name: "Cortex-M33",
    },
    IdPart {
        id: 0xd22,
        name: "Cortex-M55",
    },
    IdPart {
        id: 0xd23,
        name: "Cortex-M85",
    },
    IdPart {
        id: 0xd40,
        name: "Neoverse-V1",
    },
    IdPart {
        id: 0xd41,
        name: "Cortex-A78",
    },
    IdPart {
        id: 0xd42,
        name: "Cortex-A78AE",
    },
    IdPart {
        id: 0xd43,
        name: "Cortex-A65AE",
    },
    IdPart {
        id: 0xd44,
        name: "Cortex-X1",
    },
    IdPart {
        id: 0xd46,
        name: "Cortex-A510",
    },
    IdPart {
        id: 0xd47,
        name: "Cortex-A710",
    },
    IdPart {
        id: 0xd48,
        name: "Cortex-X2",
    },
    IdPart {
        id: 0xd49,
        name: "Neoverse-N2",
    },
    IdPart {
        id: 0xd4a,
        name: "Neoverse-E1",
    },
    IdPart {
        id: 0xd4b,
        name: "Cortex-A78C",
    },
    IdPart {
        id: 0xd4c,
        name: "Cortex-X1C",
    },
    IdPart {
        id: 0xd4d,
        name: "Cortex-A715",
    },
    IdPart {
        id: 0xd4e,
        name: "Cortex-X3",
    },
    IdPart {
        id: 0xd4f,
        name: "Neoverse-V2",
    },
    IdPart {
        id: 0xd80,
        name: "Cortex-A520",
    },
    IdPart {
        id: 0xd81,
        name: "Cortex-A720",
    },
    IdPart {
        id: 0xd82,
        name: "Cortex-X4",
    },
    // Ubuntu's backport of upstream 7a136d59 (LP #2111723).
    IdPart {
        id: 0xd84,
        name: "Neoverse-V3",
    },
    IdPart {
        id: 0xd85,
        name: "Cortex-X925",
    },
    IdPart {
        id: 0xd87,
        name: "Cortex-A725",
    },
    IdPart {
        id: 0xd8e,
        name: "Neoverse-N3",
    },
    IdPart {
        id: -1,
        name: "unknown",
    },
];

/// `brcm_part[]`.
const BRCM_PART: &[IdPart] = &[
    IdPart {
        id: 0xf,
        name: "Brahma-B15",
    },
    IdPart {
        id: 0x100,
        name: "Brahma-B53",
    },
    IdPart {
        id: 0x516,
        name: "ThunderX2",
    },
    IdPart {
        id: -1,
        name: "unknown",
    },
];

/// `dec_part[]`.
const DEC_PART: &[IdPart] = &[
    IdPart {
        id: 0xa10,
        name: "SA110",
    },
    IdPart {
        id: 0xa11,
        name: "SA1100",
    },
    IdPart {
        id: -1,
        name: "unknown",
    },
];

/// `cavium_part[]`.
const CAVIUM_PART: &[IdPart] = &[
    IdPart {
        id: 0xa0,
        name: "ThunderX",
    },
    IdPart {
        id: 0xa1,
        name: "ThunderX-88XX",
    },
    IdPart {
        id: 0xa2,
        name: "ThunderX-81XX",
    },
    IdPart {
        id: 0xa3,
        name: "ThunderX-83XX",
    },
    IdPart {
        id: 0xaf,
        name: "ThunderX2-99xx",
    },
    IdPart {
        id: 0xb0,
        name: "OcteonTX2",
    },
    IdPart {
        id: 0xb1,
        name: "OcteonTX2-98XX",
    },
    IdPart {
        id: 0xb2,
        name: "OcteonTX2-96XX",
    },
    IdPart {
        id: 0xb3,
        name: "OcteonTX2-95XX",
    },
    IdPart {
        id: 0xb4,
        name: "OcteonTX2-95XXN",
    },
    IdPart {
        id: 0xb5,
        name: "OcteonTX2-95XXMM",
    },
    IdPart {
        id: 0xb6,
        name: "OcteonTX2-95XXO",
    },
    IdPart {
        id: 0xb8,
        name: "ThunderX3-T110",
    },
    IdPart {
        id: -1,
        name: "unknown",
    },
];

/// `apm_part[]`.
const APM_PART: &[IdPart] = &[
    IdPart {
        id: 0x0,
        name: "X-Gene",
    },
    IdPart {
        id: -1,
        name: "unknown",
    },
];

/// `qcom_part[]`.
const QCOM_PART: &[IdPart] = &[
    IdPart {
        id: 0xf,
        name: "Scorpion",
    },
    IdPart {
        id: 0x2d,
        name: "Scorpion",
    },
    IdPart {
        id: 0x4d,
        name: "Krait",
    },
    IdPart {
        id: 0x6f,
        name: "Krait",
    },
    IdPart {
        id: 0x201,
        name: "Kryo",
    },
    IdPart {
        id: 0x205,
        name: "Kryo",
    },
    IdPart {
        id: 0x211,
        name: "Kryo",
    },
    IdPart {
        id: 0x800,
        name: "Falkor-V1/Kryo",
    },
    IdPart {
        id: 0x801,
        name: "Kryo-V2",
    },
    IdPart {
        id: 0x802,
        name: "Kryo-3XX-Gold",
    },
    IdPart {
        id: 0x803,
        name: "Kryo-3XX-Silver",
    },
    IdPart {
        id: 0x804,
        name: "Kryo-4XX-Gold",
    },
    IdPart {
        id: 0x805,
        name: "Kryo-4XX-Silver",
    },
    IdPart {
        id: 0xc00,
        name: "Falkor",
    },
    IdPart {
        id: 0xc01,
        name: "Saphira",
    },
    IdPart {
        id: -1,
        name: "unknown",
    },
];

/// `samsung_part[]`.
const SAMSUNG_PART: &[IdPart] = &[
    IdPart {
        id: 0x1,
        name: "exynos-m1",
    },
    IdPart {
        id: 0x2,
        name: "exynos-m3",
    },
    IdPart {
        id: 0x3,
        name: "exynos-m4",
    },
    IdPart {
        id: 0x4,
        name: "exynos-m5",
    },
    IdPart {
        id: -1,
        name: "unknown",
    },
];

/// `nvidia_part[]`.
const NVIDIA_PART: &[IdPart] = &[
    IdPart {
        id: 0x0,
        name: "Denver",
    },
    IdPart {
        id: 0x3,
        name: "Denver 2",
    },
    IdPart {
        id: 0x4,
        name: "Carmel",
    },
    // Ubuntu's backport of upstream 90877747 (LP #2123886).
    IdPart {
        id: 0x10,
        name: "Olympus",
    },
    IdPart {
        id: -1,
        name: "unknown",
    },
];

/// `marvell_part[]`.
const MARVELL_PART: &[IdPart] = &[
    IdPart {
        id: 0x131,
        name: "Feroceon-88FR131",
    },
    IdPart {
        id: 0x581,
        name: "PJ4/PJ4b",
    },
    IdPart {
        id: 0x584,
        name: "PJ4B-MP",
    },
    IdPart {
        id: -1,
        name: "unknown",
    },
];

/// `apple_part[]`.
const APPLE_PART: &[IdPart] = &[
    IdPart {
        id: 0x0,
        name: "Swift",
    },
    IdPart {
        id: 0x1,
        name: "Cyclone",
    },
    IdPart {
        id: 0x2,
        name: "Typhoon",
    },
    IdPart {
        id: 0x3,
        name: "Typhoon/Capri",
    },
    IdPart {
        id: 0x4,
        name: "Twister",
    },
    IdPart {
        id: 0x5,
        name: "Twister/Elba/Malta",
    },
    IdPart {
        id: 0x6,
        name: "Hurricane",
    },
    IdPart {
        id: 0x7,
        name: "Hurricane/Myst",
    },
    IdPart {
        id: 0x8,
        name: "Monsoon",
    },
    IdPart {
        id: 0x9,
        name: "Mistral",
    },
    IdPart {
        id: 0xb,
        name: "Vortex",
    },
    IdPart {
        id: 0xc,
        name: "Tempest",
    },
    IdPart {
        id: 0xf,
        name: "Tempest-M9",
    },
    IdPart {
        id: 0x10,
        name: "Vortex/Aruba",
    },
    IdPart {
        id: 0x11,
        name: "Tempest/Aruba",
    },
    IdPart {
        id: 0x12,
        name: "Lightning",
    },
    IdPart {
        id: 0x13,
        name: "Thunder",
    },
    IdPart {
        id: 0x20,
        name: "Icestorm-A14",
    },
    IdPart {
        id: 0x21,
        name: "Firestorm-A14",
    },
    IdPart {
        id: 0x22,
        name: "Icestorm-M1",
    },
    IdPart {
        id: 0x23,
        name: "Firestorm-M1",
    },
    IdPart {
        id: 0x24,
        name: "Icestorm-M1-Pro",
    },
    IdPart {
        id: 0x25,
        name: "Firestorm-M1-Pro",
    },
    IdPart {
        id: 0x26,
        name: "Thunder-M10",
    },
    IdPart {
        id: 0x28,
        name: "Icestorm-M1-Max",
    },
    IdPart {
        id: 0x29,
        name: "Firestorm-M1-Max",
    },
    IdPart {
        id: 0x30,
        name: "Blizzard-A15",
    },
    IdPart {
        id: 0x31,
        name: "Avalanche-A15",
    },
    IdPart {
        id: 0x32,
        name: "Blizzard-M2",
    },
    IdPart {
        id: 0x33,
        name: "Avalanche-M2",
    },
    IdPart {
        id: 0x34,
        name: "Blizzard-M2-Pro",
    },
    IdPart {
        id: 0x35,
        name: "Avalanche-M2-Pro",
    },
    IdPart {
        id: 0x36,
        name: "Sawtooth-A16",
    },
    IdPart {
        id: 0x37,
        name: "Everest-A16",
    },
    IdPart {
        id: 0x38,
        name: "Blizzard-M2-Max",
    },
    IdPart {
        id: 0x39,
        name: "Avalanche-M2-Max",
    },
    IdPart {
        id: -1,
        name: "unknown",
    },
];

/// `faraday_part[]`.
const FARADAY_PART: &[IdPart] = &[
    IdPart {
        id: 0x526,
        name: "FA526",
    },
    IdPart {
        id: 0x626,
        name: "FA626",
    },
    IdPart {
        id: -1,
        name: "unknown",
    },
];

/// `intel_part[]`.
const INTEL_PART: &[IdPart] = &[
    IdPart {
        id: 0x200,
        name: "i80200",
    },
    IdPart {
        id: 0x210,
        name: "PXA250A",
    },
    IdPart {
        id: 0x212,
        name: "PXA210A",
    },
    IdPart {
        id: 0x242,
        name: "i80321-400",
    },
    IdPart {
        id: 0x243,
        name: "i80321-600",
    },
    IdPart {
        id: 0x290,
        name: "PXA250B/PXA26x",
    },
    IdPart {
        id: 0x292,
        name: "PXA210B",
    },
    IdPart {
        id: 0x2c2,
        name: "i80321-400-B0",
    },
    IdPart {
        id: 0x2c3,
        name: "i80321-600-B0",
    },
    IdPart {
        id: 0x2d0,
        name: "PXA250C/PXA255/PXA26x",
    },
    IdPart {
        id: 0x2d2,
        name: "PXA210C",
    },
    IdPart {
        id: 0x411,
        name: "PXA27x",
    },
    IdPart {
        id: 0x41c,
        name: "IPX425-533",
    },
    IdPart {
        id: 0x41d,
        name: "IPX425-400",
    },
    IdPart {
        id: 0x41f,
        name: "IPX425-266",
    },
    IdPart {
        id: 0x682,
        name: "PXA32x",
    },
    IdPart {
        id: 0x683,
        name: "PXA930/PXA935",
    },
    IdPart {
        id: 0x688,
        name: "PXA30x",
    },
    IdPart {
        id: 0x689,
        name: "PXA31x",
    },
    IdPart {
        id: 0xb11,
        name: "SA1110",
    },
    IdPart {
        id: 0xc12,
        name: "IPX1200",
    },
    IdPart {
        id: -1,
        name: "unknown",
    },
];

/// `fujitsu_part[]`.
const FUJITSU_PART: &[IdPart] = &[
    IdPart {
        id: 0x1,
        name: "A64FX",
    },
    IdPart {
        id: -1,
        name: "unknown",
    },
];

/// `hisi_part[]`.
const HISI_PART: &[IdPart] = &[
    IdPart {
        id: 0xd01,
        name: "Kunpeng-920",
    },
    IdPart {
        id: 0xd40,
        name: "Cortex-A76",
    },
    IdPart {
        id: -1,
        name: "unknown",
    },
];

/// `ampere_part[]`.
const AMPERE_PART: &[IdPart] = &[
    IdPart {
        id: 0xac3,
        name: "Ampere-1",
    },
    IdPart {
        id: 0xac4,
        name: "Ampere-1a",
    },
    IdPart {
        id: -1,
        name: "unknown",
    },
];

/// `ft_part[]`.
const FT_PART: &[IdPart] = &[
    IdPart {
        id: 0x303,
        name: "FTC310",
    },
    IdPart {
        id: 0x660,
        name: "FTC660",
    },
    IdPart {
        id: 0x661,
        name: "FTC661",
    },
    IdPart {
        id: 0x662,
        name: "FTC662",
    },
    IdPart {
        id: 0x663,
        name: "FTC663",
    },
    IdPart {
        id: 0x664,
        name: "FTC664",
    },
    IdPart {
        id: 0x862,
        name: "FTC862",
    },
    IdPart {
        id: -1,
        name: "unknown",
    },
];

/// `unknown_part[]`.
const UNKNOWN_PART: &[IdPart] = &[IdPart {
    id: -1,
    name: "unknown",
}];

/// `hw_implementer[]`.
const HW_IMPLEMENTER: &[HwImpl] = &[
    HwImpl {
        id: 0x41,
        parts: ARM_PART,
        name: "ARM",
    },
    HwImpl {
        id: 0x42,
        parts: BRCM_PART,
        name: "Broadcom",
    },
    HwImpl {
        id: 0x43,
        parts: CAVIUM_PART,
        name: "Cavium",
    },
    HwImpl {
        id: 0x44,
        parts: DEC_PART,
        name: "DEC",
    },
    HwImpl {
        id: 0x46,
        parts: FUJITSU_PART,
        name: "FUJITSU",
    },
    HwImpl {
        id: 0x48,
        parts: HISI_PART,
        name: "HiSilicon",
    },
    HwImpl {
        id: 0x49,
        parts: UNKNOWN_PART,
        name: "Infineon",
    },
    HwImpl {
        id: 0x4d,
        parts: UNKNOWN_PART,
        name: "Motorola/Freescale",
    },
    HwImpl {
        id: 0x4e,
        parts: NVIDIA_PART,
        name: "NVIDIA",
    },
    HwImpl {
        id: 0x50,
        parts: APM_PART,
        name: "APM",
    },
    HwImpl {
        id: 0x51,
        parts: QCOM_PART,
        name: "Qualcomm",
    },
    HwImpl {
        id: 0x53,
        parts: SAMSUNG_PART,
        name: "Samsung",
    },
    HwImpl {
        id: 0x56,
        parts: MARVELL_PART,
        name: "Marvell",
    },
    HwImpl {
        id: 0x61,
        parts: APPLE_PART,
        name: "Apple",
    },
    HwImpl {
        id: 0x66,
        parts: FARADAY_PART,
        name: "Faraday",
    },
    HwImpl {
        id: 0x69,
        parts: INTEL_PART,
        name: "Intel",
    },
    HwImpl {
        id: 0x70,
        parts: FT_PART,
        name: "Phytium",
    },
    HwImpl {
        id: 0xc0,
        parts: AMPERE_PART,
        name: "Ampere",
    },
    HwImpl {
        id: -1,
        parts: UNKNOWN_PART,
        name: "unknown",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    fn arm(vendor: &str, part: &str, variant: &str, revision: &str) -> CpuType {
        let mut ct = CpuType::new();
        ct.vendor = Some(vendor.as_bytes().to_vec());
        ct.model = Some(part.as_bytes().to_vec());
        ct.stepping = Some(variant.as_bytes().to_vec());
        ct.revision = Some(revision.as_bytes().to_vec());
        ct
    }

    #[test]
    fn arm_cores_are_named() {
        let mut ct = arm("0x41", "0xd08", "0x0", "3");
        ids_decode(&mut ct);
        rxpy_decode(&mut ct);
        assert_eq!(ct.vendor.as_deref(), Some(&b"ARM"[..]));
        assert_eq!(ct.modelname.as_deref(), Some(&b"Cortex-A72"[..]));
        assert_eq!(ct.stepping.as_deref(), Some(&b"r0p3"[..]));
    }

    #[test]
    fn other_vendors_keep_their_stepping() {
        let mut ct = arm("0x51", "0x800", "0x7", "4");
        ids_decode(&mut ct);
        rxpy_decode(&mut ct);
        assert_eq!(ct.vendor.as_deref(), Some(&b"Qualcomm"[..]));
        assert_eq!(ct.stepping.as_deref(), Some(&b"0x7"[..]));
    }

    #[test]
    fn x86_is_left_alone() {
        let mut ct = arm("GenuineIntel", "142", "10", "x");
        ids_decode(&mut ct);
        rxpy_decode(&mut ct);
        assert_eq!(ct.vendor.as_deref(), Some(&b"GenuineIntel"[..]));
        assert_eq!(ct.vendor_id, NEG_EINVAL);
        assert!(ct.modelname.is_none());
    }

    #[test]
    fn ids_are_strtol_base_0() {
        assert_eq!(parse_id(Some(b"0x")), 0);
        assert_eq!(parse_id(Some(b"0x41")), 0x41);
        assert_eq!(parse_id(Some(b"41")), NEG_EINVAL);
        assert_eq!(parse_id(None), NEG_EINVAL);
        assert_eq!(parse_id(Some(b"0x100000000")), 0);
        assert_eq!(strtol_int(b"010", 0), Some(8));
        assert_eq!(strtol_int(b"x", 10), None);
    }

    #[test]
    fn long_steppings_are_cut() {
        let mut ct = arm("0x41", "0xd08", "123456", "7890");
        rxpy_decode(&mut ct);
        assert_eq!(ct.stepping.as_deref(), Some(&b"r123456"[..]));
    }
}
