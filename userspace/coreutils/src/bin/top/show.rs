//! The frame's contents: the summary area ("Tertiary" and "Secondary
//! summary display support") -- the uptime line, the task and CPU states,
//! the memory lines, each optionally a graph -- and the "Main Screen
//! routines" that build a task row and fill a window with them.

use coreutils::procps::pids::{ASCEND, DESCEND, Item, Val};
use coreutils::procps::stat::TicStack;

use crate::cfmt::{Arg, snprintf};
use crate::defs::{
    EU_AGI, EU_AGN, EU_CGN, EU_CGR, EU_CMD, EU_COD, EU_CPN, EU_CPU, EU_CUC, EU_CUU, EU_DAT, EU_DRT,
    EU_ENV, EU_EXE, EU_FL1, EU_FL2, EU_FLG, EU_FV1, EU_FV2, EU_GID, EU_GRP, EU_IRB, EU_IRO, EU_IWB,
    EU_IWO, EU_LID, EU_LXC, EU_MEM, EU_NCE, EU_NMA, EU_NS1, EU_NS2, EU_NS3, EU_NS4, EU_NS5, EU_NS6,
    EU_NS7, EU_NS8, EU_OOA, EU_OOM, EU_PGD, EU_PID, EU_PPD, EU_PRI, EU_PSS, EU_PZA, EU_PZF, EU_PZS,
    EU_RES, EU_RSS, EU_RZA, EU_RZF, EU_RZL, EU_RZS, EU_SGD, EU_SGN, EU_SHR, EU_SID, EU_STA, EU_SWP,
    EU_TGD, EU_THD, EU_TICS_ALL_C, EU_TM2, EU_TM3, EU_TM4, EU_TME, EU_TPG, EU_TREE_ADD,
    EU_TREE_HID, EU_TTY, EU_UED, EU_UEN, EU_URD, EU_URN, EU_USD, EU_USE, EU_USN, EU_USS, EU_VRT,
    EU_WCH, EU_XOF, EU_XON, MEDBUFSIZ, NOPRINT_XXX, QSRT_NORMAL, ROWMINSIZ, SHOW_CMDLIN,
    SHOW_CTIMES, SHOW_FOREST, SHOW_HIROWS, SHOW_IDLEPS, SHOW_JRNUMS, SHOW_JRSTRS, SHOW_TASKON,
    SMLBUFSIZ, VIEW_CPUNOD, VIEW_CPUSUM, VIEW_LOADAV, VIEW_MEMORY, VIEW_STATES,
};
use crate::fmt::{
    AUTOX_NO, Scale, TICS_AS_HOUR, TICS_AS_MINS, TICS_AS_SECS, hex_make, justify_pad, make_chr,
    make_num, make_str, make_str_utf8, scale_mem, scale_num, scale_pcnt, scale_tics,
};
use crate::wins::strstr_at;
use crate::{Graph, MemItem, Top, c_float, c_float_i, nls, utf8};

/// `Graph_bars` and `Graph_blks`: what the graphs are drawn with.
const GRAPH_BARS: &[u8] = &[b'|'; 100];
const GRAPH_BLKS: &[u8] = &[b' '; 100];
/// `Adjoin_sp`: what separates two summaries side by side.
const ADJOIN_SP: &[u8] = b" ~6 ~1";
/// `P_CORE`, `E_CORE`; `P_CORES_ONLY`, `E_CORES_ONLY`.
const E_CORE: i32 = 1;
const P_CORE: i32 = 2;
const P_CORES_ONLY: i32 = 2;
const E_CORES_ONLY: i32 = 3;
/// `STAT_NODE_INVALID`.
const STAT_NODE_INVALID: i32 = -22222;

/// `struct rx_st`: a graph and its percentages.
pub struct Rx {
    pub pcnt_one: f32,
    pub pcnt_two: f32,
    pub pcnt_tot: f32,
    pub graph: Vec<u8>,
}

/// `(int) d` for a `double` the graph keeps small.
// Truncation toward zero, as C's conversion; the operands are percentages
// times a length of at most 100.
#[allow(clippy::cast_possible_truncation)]
fn c_int(d: f64) -> i32 {
    d as i32
}

/// `sum_rx (these)`: a graph of two parts of a total, and the percentages.
#[must_use]
pub fn sum_rx(these: &Graph) -> Rx {
    let parts: [(&[u8], &[u8], &[u8]); 2] = [
        (b"%-.*s~7", b"%-.*s~8", GRAPH_BARS),
        (b"%-.*s~4", b"%-.*s~6", GRAPH_BLKS),
    ];
    // `float scale = 100.0 / these->total`, in `double`.
    let scale = if these.total > 0 {
        c_float(100.0 / these.total as f64)
    } else {
        0.0
    };
    let pcnt_one = scale * c_float_i(these.part1);
    let mut pcnt_two = scale * c_float_i(these.part2);
    if f64::from(pcnt_one + pcnt_two) > 100.0 || pcnt_two < 0.0 {
        pcnt_two = 0.0;
    }
    let pcnt_tot = pcnt_one + pcnt_two;

    let mut num1 = c_int(f64::from(pcnt_one * these.adjust) + 0.5);
    let mut num2 = c_int(f64::from(pcnt_two * these.adjust) + 0.5);
    if num1.wrapping_add(num2) > these.length {
        if num1 > these.length {
            num1 = these.length;
        }
        num2 = these.length.wrapping_sub(num1);
    }
    let mut width = these.length;
    let ix = usize::try_from(these.style.wrapping_sub(1))
        .unwrap_or(0)
        .min(1);
    let (p1, p2, style) = if ix == 0 { parts[0] } else { parts[1] };
    let mut buf1 = Vec::new();
    let mut buf2 = Vec::new();
    if num1 != 0 {
        buf1 = snprintf(SMLBUFSIZ, p1, &[Arg::I(i64::from(num1)), Arg::S(style)]).0;
        width = width.wrapping_add(2);
    }
    if num2 != 0 {
        buf2 = snprintf(SMLBUFSIZ, p2, &[Arg::I(i64::from(num2)), Arg::S(style)]).0;
        width = width.wrapping_add(2);
    }
    let buf3 = snprintf(MEDBUFSIZ, b"%s%s", &[Arg::S(&buf1), Arg::S(&buf2)]).0;
    let graph = snprintf(
        MEDBUFSIZ,
        b"[~1%-*.*s] ~1",
        &[
            Arg::I(i64::from(width)),
            Arg::I(i64::from(width)),
            Arg::S(&buf3),
        ],
    )
    .0;
    Rx {
        pcnt_one,
        pcnt_two,
        pcnt_tot,
        graph,
    }
}

impl Top {
    /// The scaling functions' view of the rcfile.
    #[must_use]
    pub fn scale(&self) -> Scale {
        Scale {
            zero_suppress: self.rc.zero_suppress != 0,
            tics_scaled: self.rc.tics_scaled,
            hertz: self.hertz,
        }
    }

    /// `sum_see (str, nobuf)`: summary pieces gathered side by side (`4`),
    /// a row written when full -- or now, with `nobuf` or an empty `str`.
    /// How many rows were written.
    fn sum_see(&mut self, s: &[u8], nobuf: bool) -> i32 {
        self.st.see_row.extend_from_slice(s);
        if !s.is_empty() {
            let du = self.cw().rc.double_up;
            if du != 0 && !nobuf {
                self.st.see_tog = self.st.see_tog.wrapping_add(1);
                if self.st.see_tog <= du {
                    self.st.see_row.extend_from_slice(ADJOIN_SP);
                    return 0;
                }
            }
        }
        if self.st.see_row.is_empty() {
            return 0;
        }
        self.st.see_row.push(b'\n');
        let row = std::mem::take(&mut self.st.see_row);
        self.show_special(false, &row);
        self.st.see_tog = 0;
        1
    }

    /// `sum_tics (this, pfx, nobuf)`: one CPU's (or node's, or all's) states
    /// as percentages, or as a graph.
    fn sum_tics(&mut self, this: &TicStack, pfx: &[u8], nobuf: bool) -> i32 {
        let ct = self.cw().rc.core_types;
        if ct == P_CORES_ONLY && this.type_core != P_CORE {
            return 0;
        }
        if ct == E_CORES_ONLY && this.type_core != E_CORE {
            return 0;
        }
        let d = this.delta;
        let mut idl_frme = d[3];
        let mut tot_frme = this.sum_delta[4];
        if 1 > tot_frme {
            idl_frme = 1;
            tot_frme = 1;
        }
        let scale = c_float(100.0 / f64::from(c_float_i(tot_frme)));
        // "account for VM tics not otherwise provided for"
        let sy = d[2].wrapping_add(d[8]).wrapping_add(d[9]);
        let sum_sys = this.sum_delta[1].wrapping_add(d[8]).wrapping_add(d[9]);

        if self.cw().rc.graph_cpus != 0 {
            self.graph_cpus.total = tot_frme;
            self.graph_cpus.part1 = this.sum_delta[0];
            self.graph_cpus.part2 = sum_sys;
            let rx = sum_rx(&self.graph_cpus);
            let line = if self.cw().rc.double_up > 1 {
                Self::fmtmk(
                    "%s~3%3.0f%s",
                    &[
                        Arg::S(pfx),
                        Arg::F(f64::from(rx.pcnt_tot)),
                        Arg::S(&rx.graph),
                    ],
                )
            } else {
                Self::fmtmk(
                    "%s ~3%#5.1f~2/%-#5.1f~3 %3.0f%s",
                    &[
                        Arg::S(pfx),
                        Arg::F(f64::from(rx.pcnt_one)),
                        Arg::F(f64::from(rx.pcnt_two)),
                        Arg::F(f64::from(rx.pcnt_tot)),
                        Arg::S(&rx.graph),
                    ],
                )
            };
            self.sum_see(&line, nobuf)
        } else {
            let pc = |v: i64| Arg::F(f64::from(c_float_i(v) * scale));
            let line = Self::fmtmk(
                self.cpu_states_fmts,
                &[
                    Arg::S(pfx),
                    pc(d[0]),
                    pc(sy),
                    pc(d[1]),
                    pc(idl_frme),
                    pc(d[4]),
                    pc(d[5]),
                    pc(d[6]),
                    pc(d[7]),
                ],
            );
            self.sum_see(&line, nobuf)
        }
    }

    /// `sum_unify (this, nobuf)`: CPUs added up `combine_cpus` at a time
    /// (`!`), shown as one.
    fn sum_unify(&mut self, this: &TicStack, nobuf: bool) -> i32 {
        let acc = &mut self.st.unify;
        for (a, v) in acc.delta.iter_mut().zip(this.delta) {
            *a = a.wrapping_add(v);
        }
        for k in [0usize, 1, 4] {
            if let (Some(a), Some(v)) = (acc.sum_delta.get_mut(k), this.sum_delta.get(k)) {
                *a = a.wrapping_add(*v);
            }
        }
        if self.st.unify_ix == 0 {
            self.st.unify_beg = this.id;
        }
        if nobuf || self.st.unify_ix >= self.cw().rc.combine_cpus.wrapping_sub(1) {
            let span = Self::fmtmk(
                "%d-%d",
                &[
                    Arg::I(i64::from(self.st.unify_beg)),
                    Arg::I(i64::from(this.id)),
                ],
            );
            let pfx = snprintf(16, b"%-7.7s:", &[Arg::S(&span)]).0;
            let acc = self.st.unify;
            let n = self.sum_tics(&acc, &pfx, nobuf);
            self.st.unify = TicStack::default();
            self.st.unify_ix = 0;
            return n;
        }
        self.st.unify_ix = self.st.unify_ix.wrapping_add(1);
        0
    }

    /// `do_cpus`: the CPU lines -- all CPUs as one, each CPU, a NUMA node
    /// and its CPUs, or CPUs combined -- as many as the screen has room for.
    fn do_cpus(&mut self) {
        let no_mas =
            |top: &Self| top.msg_row.wrapping_add(1) >= top.screen_rows_bot().wrapping_sub(1);
        if self.cw().chk(VIEW_CPUNOD) {
            let mut sel_ok = false;
            if self.numa_node_sel >= 0 {
                let nodes = self.stat_reap.numa.clone();
                if nodes
                    .iter()
                    .any(|n| n.id == self.numa_node_sel && n.numa_node != STAT_NODE_INVALID)
                {
                    sel_ok = true;
                } else {
                    self.numa_node_sel = -1;
                }
            }
            if sel_ok {
                let sel = self.numa_node_sel;
                let tmp = snprintf(
                    MEDBUFSIZ,
                    nls::NUMA_NODENAM_FMT.as_bytes(),
                    &[Arg::I(i64::from(sel))],
                )
                .0;
                let node = usize::try_from(sel)
                    .ok()
                    .and_then(|s| self.stat_reap.numa.get(s).copied())
                    .unwrap_or_default();
                self.msg_row = self.msg_row.wrapping_add(self.sum_tics(&node, &tmp, true));
                let cpus = self.stat_reap.cpus.clone();
                for c in cpus.iter().take(usize::try_from(self.cpu_cnt).unwrap_or(0)) {
                    if c.numa_node == sel {
                        if no_mas(self) {
                            break;
                        }
                        let tmp = snprintf(
                            MEDBUFSIZ,
                            nls::WORD_EACHCPU_FMT.as_bytes(),
                            &[Arg::C(b'u'), Arg::I(i64::from(c.id))],
                        )
                        .0;
                        self.msg_row = self.msg_row.wrapping_add(self.sum_tics(c, &tmp, true));
                    }
                }
            } else {
                let summary = self.stat_reap.summary;
                self.msg_row = self.msg_row.wrapping_add(self.sum_tics(
                    &summary,
                    nls::WORD_ALLCPUS_TXT.as_bytes(),
                    true,
                ));
                let nodes = self.stat_reap.numa.clone();
                for n in &nodes {
                    if n.numa_node == STAT_NODE_INVALID {
                        continue;
                    }
                    if no_mas(self) {
                        break;
                    }
                    let tmp = snprintf(
                        MEDBUFSIZ,
                        nls::NUMA_NODENAM_FMT.as_bytes(),
                        &[Arg::I(i64::from(n.id))],
                    )
                    .0;
                    self.msg_row = self.msg_row.wrapping_add(self.sum_tics(n, &tmp, true));
                }
            }
        } else if !self.cw().chk(VIEW_CPUSUM) {
            let cpus = self.stat_reap.cpus.clone();
            let cnt = usize::try_from(self.cpu_cnt).unwrap_or(0);
            if self.cw().rc.combine_cpus != 0 {
                for (i, c) in cpus.iter().take(cnt).enumerate() {
                    let n = self.sum_unify(c, i.saturating_add(1) >= cnt);
                    self.msg_row = self.msg_row.wrapping_add(n);
                    if no_mas(self) {
                        break;
                    }
                }
            } else {
                for (i, c) in cpus.iter().take(cnt).enumerate() {
                    let ctab = *b"uEP";
                    let chr = if self.cw().rc.core_types != 0 {
                        usize::try_from(c.type_core)
                            .ok()
                            .and_then(|t| ctab.get(t))
                            .copied()
                            .unwrap_or(b'u')
                    } else {
                        b'u'
                    };
                    let tmp = snprintf(
                        MEDBUFSIZ,
                        nls::WORD_EACHCPU_FMT.as_bytes(),
                        &[Arg::C(chr), Arg::I(i64::from(c.id))],
                    )
                    .0;
                    let n = self.sum_tics(c, &tmp, i.saturating_add(1) >= cnt);
                    self.msg_row = self.msg_row.wrapping_add(n);
                    if no_mas(self) {
                        break;
                    }
                }
            }
        } else {
            let summary = self.stat_reap.summary;
            self.msg_row = self.msg_row.wrapping_add(self.sum_tics(
                &summary,
                nls::WORD_ALLCPUS_TXT.as_bytes(),
                true,
            ));
        }
        // "coax this guy into flushing any pending cpu stuff ..."
        self.msg_row = self.msg_row.wrapping_add(self.sum_see(b"", true));
    }

    /// `do_memory`: the two memory lines, as figures or as graphs.
    fn do_memory(&mut self) {
        const DIVS: [f32; 6] = [
            1.0,
            1024.0,
            1024.0 * 1024.0,
            1024.0 * 1024.0 * 1024.0,
            1024.0 * 1024.0 * 1024.0 * 1024.0,
            1024.0 * 1024.0 * 1024.0 * 1024.0 * 1024.0,
        ];
        const LABELS: [&str; 6] = [
            nls::AMT_KILOBYTE_TXT,
            nls::AMT_MEGABYTE_TXT,
            nls::AMT_GIGABYTE_TXT,
            nls::AMT_TERABYTE_TXT,
            nls::AMT_PETABYTE_TXT,
            nls::AMT_EXXABYTE_TXT,
        ];
        let sc = usize::try_from(self.rc.summ_mscale).unwrap_or(0).min(5);
        let div = DIVS.get(sc).copied().unwrap_or(1.0);
        let fmts: &[u8] = if sc == 0 { b"%.0f " } else { b"%#.1f " };
        let label = LABELS.get(sc).map_or(&b""[..], |l| l.as_bytes());
        // `mkM (x)` then `prT (b, z)`: the figure in its unit, in a buffer of
        // ten, a `+` where it was cut.
        let pr = |x: f32| -> Vec<u8> {
            let (mut b, full) = snprintf(10, fmts, &[Arg::F(f64::from(x / div))]);
            if 9 < full
                && let Some(c) = b.get_mut(8)
            {
                *c = b'+';
            }
            b
        };
        // `(float) x` of an `unsigned long`, and of a `long`.
        #[allow(clippy::cast_precision_loss)]
        let fu = |x: u64| x as f32;
        let mem = self.mem_stack;
        let m = |e| mem_val_of(&mem, e);
        let my_qued = m(MemItem::Buf).wrapping_add(m(MemItem::Que)).cast_signed();
        let abv_mem = nls::WORD_ABV_MEM_TXT.as_bytes();
        let abv_swp = nls::WORD_ABV_SWP_TXT.as_bytes();

        if self.cw().rc.graph_mems != 0 {
            let my_used = m(MemItem::Tot)
                .wrapping_sub(m(MemItem::Fre))
                .wrapping_sub(my_qued.cast_unsigned())
                .cast_signed();
            let my_misc = m(MemItem::Tot)
                .wrapping_sub(m(MemItem::Avl))
                .wrapping_sub(my_used.cast_unsigned())
                .cast_signed();
            self.graph_mems.total = m(MemItem::Tot).cast_signed();
            self.graph_mems.part1 = my_used;
            self.graph_mems.part2 = my_misc;
            let rx = sum_rx(&self.graph_mems);
            let row = if self.cw().rc.double_up > 1 {
                snprintf(
                    ROWMINSIZ,
                    b"%s %s~3%3.0f%s",
                    &[
                        Arg::S(label),
                        Arg::S(abv_mem),
                        Arg::F(f64::from(rx.pcnt_tot)),
                        Arg::S(&rx.graph),
                    ],
                )
                .0
            } else {
                let b0 = pr(fu(m(MemItem::Tot)));
                snprintf(
                    ROWMINSIZ,
                    b"%s %s:~3%#5.1f~2/%-9.9s~3%s",
                    &[
                        Arg::S(label),
                        Arg::S(abv_mem),
                        Arg::F(f64::from(rx.pcnt_tot)),
                        Arg::S(&b0),
                        Arg::S(&rx.graph),
                    ],
                )
                .0
            };
            self.msg_row = self.msg_row.wrapping_add(self.sum_see(&row, false));

            self.graph_mems.total = m(MemItem::SwpTot).cast_signed();
            self.graph_mems.part1 = 0;
            self.graph_mems.part2 = m(MemItem::SwpUse).cast_signed();
            let rx = sum_rx(&self.graph_mems);
            let row = if self.cw().rc.double_up > 1 {
                snprintf(
                    ROWMINSIZ,
                    b"%s %s~3%3.0f%s",
                    &[
                        Arg::S(label),
                        Arg::S(abv_swp),
                        Arg::F(f64::from(rx.pcnt_two)),
                        Arg::S(&rx.graph),
                    ],
                )
                .0
            } else {
                let b1 = pr(fu(m(MemItem::SwpTot)));
                snprintf(
                    ROWMINSIZ,
                    b"%s %s:~3%#5.1f~2/%-9.9s~3%s",
                    &[
                        Arg::S(label),
                        Arg::S(abv_swp),
                        Arg::F(f64::from(rx.pcnt_two)),
                        Arg::S(&b1),
                        Arg::S(&rx.graph),
                    ],
                )
                .0
            };
            self.msg_row = self.msg_row.wrapping_add(self.sum_see(&row, true));
        } else {
            #[allow(clippy::cast_precision_loss)]
            let qued = my_qued as f32;
            let b = [
                pr(fu(m(MemItem::Tot))),
                pr(fu(m(MemItem::Fre))),
                pr(fu(m(MemItem::Use))),
                pr(qued),
                pr(fu(m(MemItem::SwpTot))),
                pr(fu(m(MemItem::SwpFre))),
                pr(fu(m(MemItem::SwpUse))),
                pr(fu(m(MemItem::Avl))),
            ];
            let row = snprintf(
                ROWMINSIZ,
                nls::MEMORY_LINE1_FMT.as_bytes(),
                &[
                    Arg::S(label),
                    Arg::S(abv_mem),
                    Arg::S(&b[0]),
                    Arg::S(&b[1]),
                    Arg::S(&b[2]),
                    Arg::S(&b[3]),
                ],
            )
            .0;
            self.msg_row = self.msg_row.wrapping_add(self.sum_see(&row, false));
            let row = snprintf(
                ROWMINSIZ,
                nls::MEMORY_LINE2_FMT.as_bytes(),
                &[
                    Arg::S(label),
                    Arg::S(abv_swp),
                    Arg::S(&b[4]),
                    Arg::S(&b[5]),
                    Arg::S(&b[6]),
                    Arg::S(&b[7]),
                    Arg::S(abv_mem),
                ],
            )
            .0;
            self.msg_row = self.msg_row.wrapping_add(self.sum_see(&row, true));
        }
    }

    /// `procps_uptime_sprint`: ` HH:MM:SS up ..., N users,  load average:
    /// ...`, or nothing when the uptime cannot be read.
    fn uptime_sprint(&mut self) -> Vec<u8> {
        let Ok(text) = std::fs::read(coreutils::procps::UPTIME_FILE) else {
            return Vec::new();
        };
        let Some(up) = coreutils::procps::uptime_secs(&text) else {
            return Vec::new();
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        let zone = self.zone.get_or_insert_with(localtime::Zone::from_env);
        let tm = zone.local(now, 0);
        let users = std::fs::read(coreutils::utmp::UTMP_FILE).map_or(0, |data| {
            coreutils::procps::count_users(&utmpfile::parse(&data))
        });
        let load = std::fs::read(coreutils::procps::LOADAVG_FILE)
            .map_or((0.0, 0.0, 0.0), |t| coreutils::procps::load_averages(&t));
        coreutils::procps::status_line(&tm, up, users, load).into_bytes()
    }

    /// `summary_show`: the summary area -- uptime, task and CPU states,
    /// memory -- each where its toggle is on and the screen has room.
    pub fn summary_show(&mut self) {
        let is_room = |top: &Self, f: i32, n: i32| {
            top.cw().chk(f) && top.msg_row.wrapping_add(n) < top.screen_rows_bot().wrapping_sub(1)
        };
        let state_line = |top: &Self| {
            let c = top.counts;
            Self::fmtmk(
                nls::STATE_LINE_1_FMT,
                &[
                    Arg::S(if top.thread_mode {
                        nls::WORD_THREADS_TXT.as_bytes()
                    } else {
                        nls::WORD_PROCESS_TXT.as_bytes()
                    }),
                    Arg::I(i64::from(top.pidsmaxt())),
                    Arg::I(i64::from(c.running)),
                    Arg::I(i64::from(c.sleeping.wrapping_add(c.other))),
                    Arg::I(i64::from(c.stopped)),
                    Arg::I(i64::from(c.zombied)),
                ],
            )
        };
        if self.restrict_some {
            if is_room(self, VIEW_STATES, 1) {
                let l = state_line(self);
                self.show_special(false, &l);
                self.msg_row = self.msg_row.wrapping_add(1);
            }
            return;
        }
        if is_room(self, VIEW_LOADAV, 1) {
            let up = self.uptime_sprint();
            let line = if self.rc.mode_altscr == 0 {
                let me = self.myname.clone();
                Self::fmtmk("%s -%s\n", &[Arg::S(&me), Arg::S(&up)])
            } else {
                let fmt = if self.cw().chk(SHOW_TASKON) {
                    "%s~6 -%s\n"
                } else {
                    "%s -%s\n"
                };
                let g = self.cw().grpname.clone();
                Self::fmtmk(fmt, &[Arg::S(&g), Arg::S(&up)])
            };
            self.show_special(false, &line);
            self.msg_row = self.msg_row.wrapping_add(1);
        }
        if is_room(self, VIEW_STATES, 2) {
            let l = state_line(self);
            self.show_special(false, &l);
            self.msg_row = self.msg_row.wrapping_add(1);
            self.do_cpus();
        }
        if is_room(self, VIEW_MEMORY, 2) {
            self.do_memory();
        }
    }

    /// `PID_VAL (e, str, p)` as `%s` prints it: `(null)` for a NULL.
    fn pv_cstr(&self, q: usize, idx: i32, e: usize) -> Vec<u8> {
        match self
            .stack_of(q, idx)
            .and_then(|s| s.get(e))
            .and_then(Val::str)
        {
            Some(s) => s.to_vec(),
            None => b"(null)".to_vec(),
        }
    }

    /// A slot of row `idx`'s stack.
    fn pv(&self, q: usize, idx: i32, e: usize) -> Val {
        self.stack_of(q, idx)
            .and_then(|s| s.get(e))
            .cloned()
            .unwrap_or(Val::Zero)
    }

    /// `find_ofs (q, buf)`: where window `q`'s search string is in `buf`.
    #[must_use]
    pub fn find_ofs(&self, q: usize, buf: &[u8]) -> Option<usize> {
        let f = &self.winstk[q].findstr;
        if f.is_empty() {
            return None;
        }
        strstr_at(buf, f)
    }

    /// `task_show (q, idx)`: row `idx` of window `q`, built -- and written,
    /// unless the window is only looking (`NOPRINT_xxx`). Empty when a
    /// filter or a collapsed branch hides it.
    #[allow(clippy::too_many_lines)]
    pub fn task_show(&mut self, q: usize, idx: i32) -> Vec<u8> {
        let win_forest = self.winstk[q].chk(SHOW_FOREST);
        if q == self.curwin && win_forest && self.pv(q, idx, EU_TREE_HID).s_ch() == b'z' {
            return Vec::new();
        }
        let mut rbuf: Vec<u8> = Vec::new();
        if self.rc.mode_altscr != 0 {
            rbuf.push(b' ');
        }
        let sc = self.scale();
        let n = usize::try_from(self.winstk[q].maxpflgs).unwrap_or(0);
        for x in 0..n {
            let i = self.winstk[q].procflgs.get(x).copied().unwrap_or(0);
            let Ok(iu) = usize::try_from(i) else { continue };
            let (s, w) = self
                .fieldstab
                .get(iu)
                .map_or((0, 0), |f| (f.scale, f.width));
            let js = self.winstk[q].chk(SHOW_JRSTRS);
            let jn = self.winstk[q].chk(SHOW_JRNUMS);
            let sint = |top: &Self, e: usize| top.pv(q, idx, e).s_int();
            let ul = |top: &Self, e: usize| top.pv(q, idx, e).ul();
            // `(float)` of an `unsigned long`.
            #[allow(clippy::cast_precision_loss)]
            let fl = |v: u64| v as f32;
            #[allow(clippy::cast_precision_loss)]
            let fi = |v: i32| v as f32;
            let cp: Option<Vec<u8>> = match iu {
                EU_XOF | EU_XON => {
                    let win = &self.winstk[q];
                    if win.chk(NOPRINT_XXX)
                        || (self.pv(q, idx, EU_STA).s_ch() == b'R' && win.chk(SHOW_HIROWS))
                    {
                        None
                    } else if iu == EU_XON {
                        Some(win.capclr_rowhigh.clone())
                    } else {
                        Some(win.capclr_rownorm.clone())
                    }
                }
                EU_STA => Some(make_chr(self.pv(q, idx, EU_STA).s_ch(), w, js)),
                EU_LID => {
                    let v = sint(self, EU_LID);
                    Some(make_num(
                        &sc,
                        &mut self.autox,
                        i64::from(v),
                        w,
                        jn,
                        i,
                        false,
                    ))
                }
                EU_AGI | EU_CPN | EU_NMA | EU_PGD | EU_PID | EU_PPD | EU_SID | EU_TGD | EU_THD
                | EU_TPG => {
                    let v = sint(self, iu);
                    Some(make_num(
                        &sc,
                        &mut self.autox,
                        i64::from(v),
                        w,
                        jn,
                        AUTOX_NO,
                        false,
                    ))
                }
                EU_AGN | EU_NCE | EU_OOA | EU_OOM => {
                    let v = sint(self, iu);
                    Some(make_num(
                        &sc,
                        &mut self.autox,
                        i64::from(v),
                        w,
                        jn,
                        AUTOX_NO,
                        true,
                    ))
                }
                EU_FV1 | EU_FV2 => Some(scale_num(&sc, fi(sint(self, iu)), w, jn)),
                EU_PRI => {
                    let v = sint(self, EU_PRI);
                    if -99 > v || 999 < v {
                        Some(make_str(&mut self.autox, b"rt", w, jn, AUTOX_NO))
                    } else {
                        Some(make_num(
                            &sc,
                            &mut self.autox,
                            i64::from(v),
                            w,
                            jn,
                            AUTOX_NO,
                            false,
                        ))
                    }
                }
                EU_CPU => {
                    if self.restrict_some {
                        Some(justify_pad(b"?", w, jn))
                    } else {
                        #[allow(clippy::cast_precision_loss)]
                        let mut u = self.pv(q, idx, EU_CPU).u_int() as f32;
                        let nthr = sint(self, EU_THD);
                        if q == self.curwin && win_forest {
                            #[allow(clippy::cast_precision_loss)]
                            let add = self.pv(q, idx, EU_TREE_ADD).u_int() as f32;
                            u += add;
                        }
                        u *= self.frame_etscale;
                        let cap = 100.0 * f64::from(nthr);
                        if self.pv(q, idx, EU_TREE_HID).s_ch() != b'x' && f64::from(u) > cap {
                            u = c_float(cap);
                        }
                        if u > self.cpu_pmax {
                            u = self.cpu_pmax;
                        }
                        Some(scale_pcnt(&sc, u, w, jn, false))
                    }
                }
                EU_CUU | EU_CUC => {
                    if self.restrict_some {
                        Some(justify_pad(b"?", w, jn))
                    } else {
                        let v = c_float(self.pv(q, idx, iu).real());
                        Some(scale_pcnt(&sc, v, w, jn, true))
                    }
                }
                EU_GID | EU_UED | EU_URD | EU_USD => {
                    let v = self.pv(q, idx, iu).u_int();
                    Some(make_num(
                        &sc,
                        &mut self.autox,
                        i64::from(v),
                        w,
                        jn,
                        i,
                        false,
                    ))
                }
                EU_NS1 | EU_NS2 | EU_NS3 | EU_NS4 | EU_NS5 | EU_NS6 | EU_NS7 | EU_NS8 => {
                    let v = ul(self, iu).cast_signed();
                    Some(make_num(&sc, &mut self.autox, v, w, jn, i, true))
                }
                EU_COD | EU_DAT | EU_DRT | EU_PZA | EU_PZF | EU_PZS | EU_PSS | EU_RES | EU_RSS
                | EU_RZA | EU_RZF | EU_RZL | EU_RZS | EU_SHR | EU_SWP | EU_USE | EU_USS
                | EU_VRT => Some(scale_mem(&sc, s, fl(ul(self, iu)), w, jn)),
                EU_FL1 | EU_FL2 | EU_IRB | EU_IRO | EU_IWB | EU_IWO => {
                    Some(scale_num(&sc, fl(ul(self, iu)), w, jn))
                }
                EU_MEM => {
                    if self.restrict_some {
                        Some(justify_pad(b"?", w, jn))
                    } else {
                        let tot = fl(self.mem_val(MemItem::Tot));
                        let v = fl(ul(self, EU_MEM)) * 100.0 / tot;
                        Some(scale_pcnt(&sc, v, w, jn, false))
                    }
                }
                EU_FLG => {
                    let h = hex_make(ul(self, EU_FLG).cast_signed(), true);
                    Some(make_str(&mut self.autox, &h, w, js, AUTOX_NO))
                }
                EU_TM2 | EU_TME => {
                    let t = if self.winstk[q].chk(SHOW_CTIMES) {
                        ul(self, EU_TICS_ALL_C)
                    } else {
                        ul(self, iu)
                    };
                    Some(scale_tics(&sc, t, w, jn, TICS_AS_SECS))
                }
                EU_TM3 => Some(scale_tics(&sc, ul(self, EU_TM3), w, jn, TICS_AS_MINS)),
                EU_TM4 => {
                    let t = self.pv(q, idx, EU_TM4).real() * self.hertz as f64;
                    Some(scale_tics(&sc, double_to_tic(t), w, jn, TICS_AS_HOUR))
                }
                EU_LXC | EU_TTY | EU_WCH => {
                    let v = self.pv_cstr(q, idx, iu);
                    Some(make_str(&mut self.autox, &v, w, js, i))
                }
                EU_GRP | EU_UEN | EU_URN | EU_USN => {
                    let v = self.pv_cstr(q, idx, iu);
                    Some(make_str_utf8(&mut self.autox, &v, w, js, i))
                }
                EU_CGN | EU_CGR | EU_ENV | EU_EXE | EU_SGN => {
                    let v = self.pv_cstr(q, idx, iu);
                    Some(self.var_utf8(q, &v, js))
                }
                EU_SGD => {
                    let v = self.pv_cstr(q, idx, EU_SGD);
                    Some(self.make_var(q, &v, js))
                }
                EU_CMD => {
                    let v = self.forest_display(q, idx);
                    Some(self.var_utf8(q, &v, js))
                }
                _ => continue,
            };
            if let Some(cp) = cp {
                if !self.winstk[q].osel.is_empty() && !self.osel_matched(q, iu, &cp) {
                    return Vec::new();
                }
                rbuf.extend_from_slice(&cp);
            }
        }

        if !self.winstk[q].chk(NOPRINT_XXX) {
            let win = &self.winstk[q];
            let cap = if win.chk(SHOW_HIROWS) && self.pv(q, idx, EU_STA).s_ch() == b'R' {
                win.capclr_rowhigh.clone()
            } else {
                win.capclr_rownorm.clone()
            };
            let hdr = win.capclr_hdr.clone();
            let findstr = win.findstr.clone();
            let findlen = usize::try_from(win.findlen).unwrap_or(0);
            let rows = self.screen_rows;
            if let Some(mut ofs) = self.find_ofs(q, &rbuf) {
                self.out.poof(rows, b"\n", &cap);
                let mut row = rbuf.clone();
                loop {
                    let head = row.get(..ofs).unwrap_or_default().to_vec();
                    self.out.putt(
                        b"%s%s%s%s",
                        &[Arg::S(&head), Arg::S(&hdr), Arg::S(&findstr), Arg::S(&cap)],
                    );
                    row = row
                        .get(ofs.saturating_add(findlen)..)
                        .unwrap_or_default()
                        .to_vec();
                    match self.find_ofs(q, &row) {
                        Some(o) => ofs = o,
                        None => break,
                    }
                }
                let endl = self.caps.caps_endline.clone();
                self.out.putt(b"%s%s", &[Arg::S(&row), Arg::S(&endl)]);
                // "with a corrupted rbuf, ensure row is 'counted' by
                // window_show"
                return b"!".to_vec();
            }
            let endl = self.caps.caps_endline.clone();
            self.out.puff(
                rows,
                b"\n%s%s%s",
                &[Arg::S(&cap), Arg::S(&rbuf), Arg::S(&endl)],
            );
        }
        rbuf
    }

    /// `makeVAR (S)`: a variable-width column, scrolled (`varcolbeg`).
    fn make_var(&mut self, q: usize, pv: &[u8], js: bool) -> Vec<u8> {
        let beg = self.winstk[q].varcolbeg;
        let sz = self.winstk[q].varcolsz;
        if beg == 0 {
            return make_str(&mut self.autox, pv, sz, js, AUTOX_NO);
        }
        let from = usize::try_from(beg).unwrap_or(0);
        let s = if from < pv.len() {
            pv.get(from..).unwrap_or_default()
        } else {
            b""
        };
        make_str(&mut self.autox, s, sz, js, AUTOX_NO)
    }

    /// `varUTF8 (S)`: [`Self::make_var`] in columns.
    fn var_utf8(&mut self, q: usize, pv: &[u8], js: bool) -> Vec<u8> {
        let beg = self.winstk[q].varcolbeg;
        let sz = self.winstk[q].varcolsz;
        if beg == 0 {
            return make_str_utf8(&mut self.autox, pv, sz, js, AUTOX_NO);
        }
        let cols = i32::try_from(pv.len())
            .unwrap_or(i32::MAX)
            .wrapping_sub(utf8::delta(pv));
        let s = if beg < cols {
            let at = usize::try_from(utf8::embody(pv, beg)).unwrap_or(0);
            pv.get(at..).unwrap_or_default()
        } else {
            b""
        };
        make_str_utf8(&mut self.autox, s, sz, js, AUTOX_NO)
    }

    /// Whether row `i` of the current window shows: through its user filter
    /// and not hidden.
    fn visible(&mut self, w: usize, i: i32) -> bool {
        self.wins_usrselect(w, i) && !self.task_show(w, i).is_empty()
    }

    /// `window_hlp`: the current window's first row made one that shows,
    /// scrolling the way the user last moved.
    pub fn window_hlp(&mut self) {
        let w = self.curwin;
        let (beg, end) = if self.winstk[w].focus_pid != 0 {
            (self.winstk[w].focus_beg, self.winstk[w].focus_end)
        } else {
            (0, self.pidsmaxt())
        };
        self.winstk[w].set(NOPRINT_XXX);
        let win = &mut self.winstk[w];
        win.begtask = win.begtask.wrapping_add(win.begnext);
        if win.begtask <= beg {
            win.begtask = beg;
            win.begnext = 1;
        } else if win.begtask >= end {
            win.begtask = end.wrapping_sub(1);
        }
        let mut reversed = false;
        let mut forward = self.winstk[w].begnext > 0;
        'redux: loop {
            if forward {
                let mut i = self.winstk[w].begtask;
                while i < end {
                    if self.visible(w, i) {
                        break;
                    }
                    i = i.wrapping_add(1);
                }
                if i < end {
                    self.winstk[w].begtask = i;
                    break 'redux;
                }
                self.winstk[w].begtask = end.wrapping_sub(1);
            }
            let mut i = self.winstk[w].begtask;
            while i > beg {
                if self.visible(w, i) {
                    break;
                }
                i = i.wrapping_sub(1);
            }
            self.winstk[w].begtask = i;
            if i == beg && !reversed && !self.visible(w, beg) {
                reversed = true;
                forward = true;
                continue 'redux;
            }
            break;
        }
        self.winstk[w].begnext = 0;
        self.winstk[w].off(NOPRINT_XXX);
    }

    /// `window_show (q, wmax)`: window `q`'s header and as many rows as fit
    /// in `wmax`, sorted (or as a tree); the rows used.
    pub fn window_show(&mut self, q: usize, wmax: i32) -> i32 {
        let hdr = self.winstk[q].capclr_hdr.clone();
        let col = self.winstk[q].columnhdr.clone();
        let endl = self.caps.caps_endline.clone();
        let rows = self.screen_rows;
        self.out.puff(
            rows,
            b"\n%s%s%s",
            &[Arg::S(&hdr), Arg::S(&col), Arg::S(&endl)],
        );
        let n = self.pidsmaxt();
        if n == 0 {
            return 1;
        }
        if self.winstk[q].chk(SHOW_FOREST) {
            self.forest_begin(q);
            if self.winstk[q].focus_pid != 0 {
                self.forest_config(q);
            }
        } else {
            let sortindx = usize::try_from(self.winstk[q].rc.sortindx).unwrap_or(0);
            let mut item = self.fieldstab.get(sortindx).map_or(Item::Noop, |f| f.item);
            if item == Item::Cmd && self.winstk[q].chk(SHOW_CMDLIN) {
                item = Item::Cmdline;
            } else if item == Item::TicsAll && self.winstk[q].chk(SHOW_CTIMES) {
                item = Item::TicsAllC;
            }
            let dir = if self.winstk[q].chk(QSRT_NORMAL) {
                DESCEND
            } else {
                ASCEND
            };
            let total = usize::try_from(n).unwrap_or(0);
            let ok = {
                let pids = &self.pids;
                let ppt = &mut self.winstk[q].ppt;
                let end = total.min(ppt.len());
                ppt.get_mut(..end)
                    .is_some_and(|slice| pids.sort(slice, item, dir))
            };
            if !ok {
                let msg = Self::fmtmk(
                    nls::LIB_ERRORPID_FMT,
                    &[Arg::I(i64::from(line!())), Arg::S(&crate::strerror_no(22))],
                );
                self.error_exit(&msg);
            }
        }

        if self.cw().begnext != 0 {
            self.window_hlp();
        } else {
            self.winstk[q].off(NOPRINT_XXX);
        }

        let mut i = self.winstk[q].begtask;
        let mut lwin = 1;
        let wmax = wmax.min(self.winstk[q].winlines.wrapping_add(1));
        let numtasks = if self.winstk[q].focus_pid != 0 {
            self.winstk[q].focus_end.min(n)
        } else {
            n
        };
        if self.winstk[q].chk(SHOW_IDLEPS) && self.winstk[q].usrseltyp == 0 {
            while i < numtasks && lwin < wmax {
                let shown = !self.task_show(q, i).is_empty();
                i = i.wrapping_add(1);
                if shown {
                    lwin = lwin.wrapping_add(1);
                }
            }
        } else {
            while i < numtasks && lwin < wmax {
                let busy = 0 < self.pv(q, i, EU_CPU).u_int();
                if (self.winstk[q].chk(SHOW_IDLEPS) || busy)
                    && self.wins_usrselect(q, i)
                    && !self.task_show(q, i).is_empty()
                {
                    lwin = lwin.wrapping_add(1);
                }
                i = i.wrapping_add(1);
            }
        }
        lwin
    }
}

/// `MEM_VAL (e)` of a reading.
pub fn mem_val_of(m: &coreutils::procps::meminfo::Mem, e: MemItem) -> u64 {
    match e {
        MemItem::Fre => m.free,
        MemItem::Use => m.used,
        MemItem::Tot => m.total,
        MemItem::Que => m.cached_all,
        MemItem::Buf => m.buffers,
        MemItem::Avl => m.available,
        MemItem::SwpTot => m.swap_total,
        MemItem::SwpFre => m.swap_free,
        MemItem::SwpUse => m.swap_used,
    }
}

/// `(TIC_t) d`: a `double` made `unsigned long long` as x86-64 converts --
/// truncation in range, and what `cvttsd2si` leaves for the rest.
// Each branch is the conversion C compiles; the casts are its arithmetic.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn double_to_tic(d: f64) -> u64 {
    if d.is_nan() {
        return 1 << 63;
    }
    if d < 9_223_372_036_854_775_808.0 {
        if d <= -9_223_372_036_854_775_809.0 {
            return 1 << 63;
        }
        (d as i64).cast_unsigned()
    } else if d < 18_446_744_073_709_551_616.0 {
        ((d - 9_223_372_036_854_775_808.0) as i64).cast_unsigned() ^ (1 << 63)
    } else {
        1 << 63
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_graph_is_bars_in_two_parts() {
        let g = Graph {
            adjust: 0.1,
            length: 10,
            style: 1,
            total: 100,
            part1: 30,
            part2: 20,
        };
        let rx = sum_rx(&g);
        assert_eq!(rx.pcnt_tot.to_bits(), 50.0f32.to_bits(), "an exact sum");
        assert_eq!(rx.graph, b"[~1|||~7||~8     ] ~1");
    }

    #[test]
    fn a_double_converts_to_ticks_as_x86_does() {
        assert_eq!(double_to_tic(12.9), 12);
        assert_eq!(double_to_tic(-1.0), u64::MAX);
        assert_eq!(double_to_tic(f64::NAN), 1 << 63);
    }
}
