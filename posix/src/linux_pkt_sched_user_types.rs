//! `<linux/pkt_sched.h>` — packet-scheduler (qdisc) ABI.
//!
//! `tc qdisc add` builds the kernel's outbound queueing hierarchy
//! from the qdisc-kind strings and `TC_PRIO_*` priorities defined
//! here. The kernel still keeps the eight-band pfifo_fast as the
//! default qdisc, but modern systems install `fq_codel` or `fq` on
//! every interface.

// ---------------------------------------------------------------------------
// IP precedence-to-band mapping (`TC_PRIO_*`)
// ---------------------------------------------------------------------------

pub const TC_PRIO_BESTEFFORT: u32 = 0;
pub const TC_PRIO_FILLER: u32 = 1;
pub const TC_PRIO_BULK: u32 = 2;
pub const TC_PRIO_INTERACTIVE_BULK: u32 = 4;
pub const TC_PRIO_INTERACTIVE: u32 = 6;
pub const TC_PRIO_CONTROL: u32 = 7;
pub const TC_PRIO_MAX: u32 = 15;

// ---------------------------------------------------------------------------
// Well-known qdisc kinds (`struct nlattr` IFLA_QDISC text value)
// ---------------------------------------------------------------------------

pub const TC_KIND_PFIFO: &str = "pfifo";
pub const TC_KIND_PFIFO_FAST: &str = "pfifo_fast";
pub const TC_KIND_BFIFO: &str = "bfifo";
pub const TC_KIND_PRIO: &str = "prio";
pub const TC_KIND_TBF: &str = "tbf";
pub const TC_KIND_SFQ: &str = "sfq";
pub const TC_KIND_RED: &str = "red";
pub const TC_KIND_HTB: &str = "htb";
pub const TC_KIND_HFSC: &str = "hfsc";
pub const TC_KIND_NETEM: &str = "netem";
pub const TC_KIND_CODEL: &str = "codel";
pub const TC_KIND_FQ_CODEL: &str = "fq_codel";
pub const TC_KIND_FQ: &str = "fq";
pub const TC_KIND_CAKE: &str = "cake";
pub const TC_KIND_NOQUEUE: &str = "noqueue";
pub const TC_KIND_MQ: &str = "mq";
pub const TC_KIND_MQPRIO: &str = "mqprio";
pub const TC_KIND_INGRESS: &str = "ingress";
pub const TC_KIND_CLSACT: &str = "clsact";

// ---------------------------------------------------------------------------
// pfifo_fast and prio band defaults
// ---------------------------------------------------------------------------

pub const TC_PRIO_DEFAULT_BANDS: u32 = 3;
pub const TC_PRIO_MAX_BANDS: u32 = 16;

// ---------------------------------------------------------------------------
// HTB attribute ids (`enum tc_htb_xstats_attrs`)
// ---------------------------------------------------------------------------

pub const TCA_HTB_UNSPEC: u32 = 0;
pub const TCA_HTB_PARMS: u32 = 1;
pub const TCA_HTB_INIT: u32 = 2;
pub const TCA_HTB_CTAB: u32 = 3;
pub const TCA_HTB_RTAB: u32 = 4;
pub const TCA_HTB_DIRECT_QLEN: u32 = 5;
pub const TCA_HTB_RATE64: u32 = 6;
pub const TCA_HTB_CEIL64: u32 = 7;
pub const TCA_HTB_PAD: u32 = 8;
pub const TCA_HTB_OFFLOAD: u32 = 9;
pub const TCA_HTB_MAX: u32 = 9;

// ---------------------------------------------------------------------------
// fq_codel attribute ids
// ---------------------------------------------------------------------------

pub const TCA_FQ_CODEL_UNSPEC: u32 = 0;
pub const TCA_FQ_CODEL_TARGET: u32 = 1;
pub const TCA_FQ_CODEL_LIMIT: u32 = 2;
pub const TCA_FQ_CODEL_INTERVAL: u32 = 3;
pub const TCA_FQ_CODEL_ECN: u32 = 4;
pub const TCA_FQ_CODEL_FLOWS: u32 = 5;
pub const TCA_FQ_CODEL_QUANTUM: u32 = 6;
pub const TCA_FQ_CODEL_CE_THRESHOLD: u32 = 7;
pub const TCA_FQ_CODEL_DROP_BATCH_SIZE: u32 = 8;
pub const TCA_FQ_CODEL_MEMORY_LIMIT: u32 = 9;
pub const TCA_FQ_CODEL_CE_THRESHOLD_SELECTOR: u32 = 10;
pub const TCA_FQ_CODEL_CE_THRESHOLD_MASK: u32 = 11;
pub const TCA_FQ_CODEL_MAX: u32 = 11;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_anchors() {
        // 0 is best-effort (default); 7 is the control class; everything
        // above 7 is private/experimental.
        assert_eq!(TC_PRIO_BESTEFFORT, 0);
        assert_eq!(TC_PRIO_CONTROL, 7);
        // The field fits in 4 bits (0..=15).
        assert_eq!(TC_PRIO_MAX, 15);
    }

    #[test]
    fn test_priorities_monotonic_where_specified() {
        // The named bands are monotonically non-decreasing.
        let p = [
            TC_PRIO_BESTEFFORT,
            TC_PRIO_FILLER,
            TC_PRIO_BULK,
            TC_PRIO_INTERACTIVE_BULK,
            TC_PRIO_INTERACTIVE,
            TC_PRIO_CONTROL,
        ];
        for w in p.windows(2) {
            assert!(w[0] <= w[1]);
        }
    }

    #[test]
    fn test_kind_strings_distinct() {
        let k = [
            TC_KIND_PFIFO,
            TC_KIND_PFIFO_FAST,
            TC_KIND_BFIFO,
            TC_KIND_PRIO,
            TC_KIND_TBF,
            TC_KIND_SFQ,
            TC_KIND_RED,
            TC_KIND_HTB,
            TC_KIND_HFSC,
            TC_KIND_NETEM,
            TC_KIND_CODEL,
            TC_KIND_FQ_CODEL,
            TC_KIND_FQ,
            TC_KIND_CAKE,
            TC_KIND_NOQUEUE,
            TC_KIND_MQ,
            TC_KIND_MQPRIO,
            TC_KIND_INGRESS,
            TC_KIND_CLSACT,
        ];
        for i in 0..k.len() {
            for j in (i + 1)..k.len() {
                assert_ne!(k[i], k[j]);
            }
        }
    }

    #[test]
    fn test_pfifo_band_defaults() {
        // pfifo_fast ships 3 bands and supports up to 16.
        assert_eq!(TC_PRIO_DEFAULT_BANDS, 3);
        assert_eq!(TC_PRIO_MAX_BANDS, 16);
    }

    #[test]
    fn test_htb_attrs_dense_0_to_9() {
        let h = [
            TCA_HTB_UNSPEC,
            TCA_HTB_PARMS,
            TCA_HTB_INIT,
            TCA_HTB_CTAB,
            TCA_HTB_RTAB,
            TCA_HTB_DIRECT_QLEN,
            TCA_HTB_RATE64,
            TCA_HTB_CEIL64,
            TCA_HTB_PAD,
            TCA_HTB_OFFLOAD,
        ];
        for (i, &v) in h.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(TCA_HTB_MAX, 9);
    }

    #[test]
    fn test_fq_codel_attrs_dense_0_to_11() {
        let f = [
            TCA_FQ_CODEL_UNSPEC,
            TCA_FQ_CODEL_TARGET,
            TCA_FQ_CODEL_LIMIT,
            TCA_FQ_CODEL_INTERVAL,
            TCA_FQ_CODEL_ECN,
            TCA_FQ_CODEL_FLOWS,
            TCA_FQ_CODEL_QUANTUM,
            TCA_FQ_CODEL_CE_THRESHOLD,
            TCA_FQ_CODEL_DROP_BATCH_SIZE,
            TCA_FQ_CODEL_MEMORY_LIMIT,
            TCA_FQ_CODEL_CE_THRESHOLD_SELECTOR,
            TCA_FQ_CODEL_CE_THRESHOLD_MASK,
        ];
        for (i, &v) in f.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(TCA_FQ_CODEL_MAX, 11);
    }
}
