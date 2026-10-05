//! argp's help and usage messages, glibc's argp-help.
//!
//! The options of the whole tree are gathered into one list -- an entry for
//! each option and the aliases after it, a short option kept only where no
//! earlier one has its letter -- and sorted: by group (0 and up, then the
//! negative ones, -1 last), a child with a header or a group of its own
//! making a cluster that sorts by that group and comes after the plain
//! entries of it (clusters of one group, the later child first, as glibc
//! has them); within a group, documentation entries after the options,
//! and the options by their first short letter or long name, ignoring
//! case, a lower-case letter before its capital. Each entry is printed in
//! columns -- short options from column 2, long ones from 6, the text from
//! 29 -- every number of which `ARGP_HELP_FMT` may change, as glibc's
//! does, read here at each message (glibc's once a process). The usage
//! line lists the short options without arguments in one bracket, those
//! with in one each, then every long option, then the arguments.
//!
//! Every word goes through [`FmtStream`], glibc's line filler, so lines
//! break where glibc's do.

use super::fmt::FmtStream;
use super::{
    ARGP_HELP_BUG_ADDR, ARGP_HELP_LONG, ARGP_HELP_POST_DOC, ARGP_HELP_PRE_DOC, ARGP_HELP_SEE,
    ARGP_HELP_SHORT_USAGE, ARGP_HELP_USAGE, ARGP_KEY_HELP_ARGS_DOC, ARGP_KEY_HELP_DUP_ARGS_NOTE,
    ARGP_KEY_HELP_EXTRA, ARGP_KEY_HELP_HEADER, ARGP_KEY_HELP_POST_DOC, ARGP_KEY_HELP_PRE_DOC, Argp,
    ArgpOption, ArgpState, OPTION_ALIAS, OPTION_ARG_OPTIONAL, OPTION_DOC, OPTION_HIDDEN,
    OPTION_NO_USAGE, is_end, is_short, text,
};
use crate::list::{List, NoMem};
use core::ffi::c_void;

// ---------------------------------------------------------------------------
// ARGP_HELP_FMT.
// ---------------------------------------------------------------------------

/// The help's layout: glibc's `struct uparams`.
#[derive(Clone, Copy)]
struct Params {
    dup_args: i32,
    dup_args_note: i32,
    short_opt_col: i32,
    long_opt_col: i32,
    doc_opt_col: i32,
    opt_doc_col: i32,
    header_col: i32,
    usage_indent: i32,
    rmargin: i32,
}

/// glibc's defaults.
const DEFAULTS: Params = Params {
    dup_args: 0,
    dup_args_note: 1,
    short_opt_col: 2,
    long_opt_col: 6,
    doc_opt_col: 2,
    opt_doc_col: 29,
    header_col: 1,
    usage_indent: 12,
    rmargin: 79,
};

/// Where a parameter goes in [`Params`].
type Field = fn(&mut Params) -> &mut i32;

/// The parameters' names, whether each is a flag, and where it goes.
const NAMES: [(&[u8], bool, Field); 9] = [
    (b"dup-args", true, |p| &mut p.dup_args),
    (b"dup-args-note", true, |p| &mut p.dup_args_note),
    (b"short-opt-col", false, |p| &mut p.short_opt_col),
    (b"long-opt-col", false, |p| &mut p.long_opt_col),
    (b"doc-opt-col", false, |p| &mut p.doc_opt_col),
    (b"opt-doc-col", false, |p| &mut p.opt_doc_col),
    (b"header-col", false, |p| &mut p.header_col),
    (b"usage-indent", false, |p| &mut p.usage_indent),
    (b"rmargin", false, |p| &mut p.rmargin),
];

/// A column, as a count: a negative one -- a number too big for an `int`,
/// wrapped -- is 0.
fn col(v: i32) -> usize {
    usize::try_from(v).unwrap_or(0)
}

/// The defaults, as `ARGP_HELP_FMT` changes them: glibc's
/// `fill_in_uparams`, its complaints through `argp_failure`.
///
/// # Safety
///
/// `state` is NULL or a parse's state.
unsafe fn params(state: *const ArgpState) -> Params {
    let mut p = DEFAULTS;
    // SAFETY: a C string.
    let var = unsafe { crate::environ::getenv(c"ARGP_HELP_FMT".as_ptr().cast()) };
    if var.is_null() {
        return p;
    }
    // SAFETY: the environment's C string.
    let s = unsafe { text(var) };
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let space = |c: u8| crate::ctype::isspace(i32::from(c)) != 0;
    let mut i = 0;
    loop {
        while space(at(i)) {
            i = i.saturating_add(1);
        }
        if at(i) == 0 {
            break;
        }
        if crate::ctype::isalpha(i32::from(at(i))) == 0 {
            // SAFETY: the caller's state.
            unsafe {
                super::failure_message(
                    state,
                    0,
                    0,
                    &[b"Garbage in ARGP_HELP_FMT: ", s.get(i..).unwrap_or(b"")],
                );
            }
            break;
        }
        let mut start = i;
        let mut a = i;
        while crate::ctype::isalnum(i32::from(at(a))) != 0 || at(a) == b'-' || at(a) == b'_' {
            a = a.saturating_add(1);
        }
        let mut len = a.saturating_sub(start);
        while space(at(a)) {
            a = a.saturating_add(1);
        }
        let unspec = at(a) == 0 || at(a) == b',';
        if at(a) == b'=' {
            a = a.saturating_add(1);
            while space(at(a)) {
                a = a.saturating_add(1);
            }
        }
        let mut val = 0i32;
        if unspec {
            if s.get(start..start.saturating_add(3)) == Some(b"no-") {
                start = start.saturating_add(3);
                len = len.saturating_sub(3);
            } else {
                val = 1;
            }
        } else if at(a).is_ascii_digit() {
            // atoi: strtol's value, as an int.
            let mut v: i64 = 0;
            while at(a).is_ascii_digit() {
                v = v
                    .saturating_mul(10)
                    .saturating_add(i64::from(at(a).wrapping_sub(b'0')));
                a = a.saturating_add(1);
            }
            #[allow(clippy::cast_possible_truncation)] // atoi's own conversion
            {
                val = v as i32;
            }
            while space(at(a)) {
                a = a.saturating_add(1);
            }
        }
        let name = s.get(start..start.saturating_add(len)).unwrap_or(b"");
        match NAMES.iter().find(|(n, _, _)| *n == name) {
            Some(&(_, is_bool, field)) => {
                if unspec && !is_bool {
                    // SAFETY: the caller's state.
                    unsafe {
                        super::failure_message(
                            state,
                            0,
                            0,
                            &[name, b": ARGP_HELP_FMT parameter requires a value"],
                        );
                    }
                } else {
                    *field(&mut p) = val;
                }
            }
            None => {
                // SAFETY: the caller's state.
                unsafe {
                    super::failure_message(
                        state,
                        0,
                        0,
                        &[name, b": Unknown ARGP_HELP_FMT parameter"],
                    )
                };
            }
        }
        i = a;
        if at(i) == b',' {
            i = i.saturating_add(1);
        }
    }
    p
}

// ---------------------------------------------------------------------------
// The help option list: glibc's `struct hol`.
// ---------------------------------------------------------------------------

/// A child's options set apart: glibc's `struct hol_cluster`.
struct Cluster {
    header: *const u8,
    /// Its place among its parent's children.
    index: usize,
    group: i32,
    parent: Option<usize>,
    /// The parent argp, whose filter its header goes through.
    argp: *const Argp,
    depth: usize,
}

/// An option and its aliases: glibc's `struct hol_entry`.
struct Entry {
    opt: *const ArgpOption,
    num: usize,
    /// Its short option letters: from here in the list's letters.
    short_at: usize,
    group: i32,
    cluster: Option<usize>,
    argp: *const Argp,
}

struct Hol {
    entries: List<Entry>,
    clusters: List<Cluster>,
    /// Every entry's short letters, in turn, each letter once.
    shorts: List<u8>,
}

/// The `k`th option of an entry.
///
/// # Safety
///
/// `k < e.num`.
unsafe fn opt_of(e: &Entry, k: usize) -> &ArgpOption {
    // SAFETY: the entry's options are consecutive in their table.
    unsafe { &*e.opt.add(k) }
}

const fn visible(o: &ArgpOption) -> bool {
    o.flags & OPTION_HIDDEN == 0
}

const fn is_alias(o: &ArgpOption) -> bool {
    o.flags & OPTION_ALIAS != 0
}

const fn is_doc(o: &ArgpOption) -> bool {
    o.flags & OPTION_DOC != 0
}

impl Hol {
    /// The whole tree's list, sorted: glibc's `argp_hol`, then
    /// `hol_set_group` for `help` and `version`, then `hol_sort`.
    ///
    /// # Safety
    ///
    /// `argp` is a valid argp tree.
    unsafe fn of(argp: *const Argp) -> Result<Self, NoMem> {
        let mut hol = Self {
            entries: List::new(),
            clusters: List::new(),
            shorts: List::new(),
        };
        // SAFETY: the caller's tree.
        unsafe { hol.add(argp, None)? };
        for name in [&b"help"[..], b"version"] {
            if let Some(i) = hol.find(name)
                && let Some(e) = hol.entries.as_mut_slice().get_mut(i)
            {
                e.group = -1;
            }
        }
        hol.sort();
        Ok(hol)
    }

    /// An argp's entries, then its children's: glibc's `make_hol` and
    /// `hol_append`.
    ///
    /// # Safety
    ///
    /// `argp` is a valid argp tree.
    unsafe fn add(&mut self, argp: *const Argp, cluster: Option<usize>) -> Result<(), NoMem> {
        // SAFETY: the caller's tree.
        let a = unsafe { &*argp };
        if !a.options.is_null() {
            let mut cur_group = 0i32;
            let mut o = a.options;
            // SAFETY: a table ended by its end entry.
            unsafe {
                while !is_end(&*o) {
                    let first = &*o;
                    let group = if first.group != 0 {
                        first.group
                    } else if first.name.is_null() && first.key == 0 {
                        cur_group.wrapping_add(1)
                    } else {
                        cur_group
                    };
                    cur_group = group;
                    let short_at = self.shorts.len();
                    let start = o;
                    let mut num = 0usize;
                    loop {
                        num = num.saturating_add(1);
                        if is_short(&*o) {
                            let c = u8::try_from((*o).key).unwrap_or(0);
                            if !self.shorts.contains(&c) {
                                self.shorts.push(c)?;
                            }
                        }
                        o = o.add(1);
                        if is_end(&*o) || !is_alias(&*o) {
                            break;
                        }
                    }
                    self.entries.push(Entry {
                        opt: start,
                        num,
                        short_at,
                        group,
                        cluster,
                        argp,
                    })?;
                }
            }
        }
        if !a.children.is_null() {
            let mut index = 0usize;
            // SAFETY: a list ended by an entry whose argp is NULL.
            unsafe {
                let mut c = a.children;
                while !(*c).argp.is_null() {
                    let child = &*c;
                    let child_cluster = if child.group != 0 || !child.header.is_null() {
                        let depth = cluster
                            .and_then(|p| self.clusters.get(p))
                            .map_or(0, |p| p.depth.saturating_add(1));
                        self.clusters.push(Cluster {
                            header: child.header,
                            index,
                            group: child.group,
                            parent: cluster,
                            argp,
                            depth,
                        })?;
                        Some(self.clusters.len().saturating_sub(1))
                    } else {
                        cluster
                    };
                    self.add(child.argp, child_cluster)?;
                    index = index.saturating_add(1);
                    c = c.add(1);
                }
            }
        }
        Ok(())
    }

    /// The entry one of whose visible options has this long name.
    fn find(&self, name: &[u8]) -> Option<usize> {
        self.entries.iter().position(|e| {
            (0..e.num).any(|k| {
                // SAFETY: `k < num`.
                let o = unsafe { opt_of(e, k) };
                // SAFETY: a C string.
                !o.name.is_null() && visible(o) && unsafe { text(o.name) } == name
            })
        })
    }

    /// The letter at `i` in the list's letters, 0 past them (the NUL
    /// glibc's string ends with).
    fn short_at(&self, i: usize) -> u8 {
        self.shorts.get(i).copied().unwrap_or(0)
    }

    /// Calls `f(opt, real)` for each visible short option of an entry -- one
    /// whose letter is still its own -- with the option it is an alias of,
    /// until `f` answers true: glibc's `hol_entry_short_iterate`.
    fn shorts_of(&self, e: &Entry, mut f: impl FnMut(&ArgpOption, &ArgpOption) -> bool) {
        let mut so = e.short_at;
        // SAFETY: the entry's first option.
        let mut real = unsafe { opt_of(e, 0) };
        for k in 0..e.num {
            // SAFETY: `k < num`.
            let o = unsafe { opt_of(e, k) };
            if is_short(o) && i32::from(self.short_at(so)) == o.key {
                if !is_alias(o) {
                    real = o;
                }
                if visible(o) && f(o, real) {
                    return;
                }
                so = so.saturating_add(1);
            }
        }
    }

    /// The same for each visible long option: `hol_entry_long_iterate`.
    fn longs_of(e: &Entry, mut f: impl FnMut(&ArgpOption, &ArgpOption) -> bool) {
        // SAFETY: the entry's first option.
        let mut real = unsafe { opt_of(e, 0) };
        for k in 0..e.num {
            // SAFETY: `k < num`.
            let o = unsafe { opt_of(e, k) };
            if !o.name.is_null() {
                if !is_alias(o) {
                    real = o;
                }
                if visible(o) && f(o, real) {
                    return;
                }
            }
        }
    }

    /// An entry's first visible short letter, or 0.
    fn first_short(&self, e: &Entry) -> u8 {
        let mut c = 0;
        self.shorts_of(e, |o, _| {
            c = u8::try_from(o.key).unwrap_or(0);
            true
        });
        c
    }

    /// An entry's first visible long name.
    fn first_long(e: &Entry) -> Option<&[u8]> {
        (0..e.num).find_map(|k| {
            // SAFETY: `k < num`.
            let o = unsafe { opt_of(e, k) };
            // SAFETY: a C string.
            (!o.name.is_null() && visible(o)).then(|| unsafe { text(o.name) })
        })
    }

    /// The group of the cluster at the top of `c`'s ancestry.
    fn base_group(&self, mut c: usize) -> i32 {
        while let Some(p) = self.clusters.get(c).and_then(|cl| cl.parent) {
            c = p;
        }
        self.clusters.get(c).map_or(0, |cl| cl.group)
    }

    /// glibc's `hol_cluster_cmp`.
    fn cluster_cmp(&self, mut a: usize, mut b: usize) -> core::cmp::Ordering {
        let depth = |c: usize| self.clusters.get(c).map_or(0, |cl| cl.depth);
        let parent = |c: usize| self.clusters.get(c).and_then(|cl| cl.parent);
        while depth(a) > depth(b) {
            a = parent(a).unwrap_or(a);
        }
        while depth(b) > depth(a) {
            b = parent(b).unwrap_or(b);
        }
        while parent(a) != parent(b) {
            match (parent(a), parent(b)) {
                (Some(pa), Some(pb)) => {
                    a = pa;
                    b = pb;
                }
                _ => break,
            }
        }
        let (Some(ca), Some(cb)) = (self.clusters.get(a), self.clusters.get(b)) else {
            return core::cmp::Ordering::Equal;
        };
        // The later child first, among clusters of one group: glibc's.
        let eq = isize::try_from(cb.index)
            .unwrap_or(0)
            .saturating_sub(isize::try_from(ca.index).unwrap_or(0));
        group_cmp(ca.group, cb.group, eq.signum())
    }

    /// glibc's `hol_entry_cmp`.
    fn entry_cmp(&self, e1: &Entry, e2: &Entry) -> core::cmp::Ordering {
        use core::cmp::Ordering;
        if e1.cluster != e2.cluster {
            return match (e1.cluster, e2.cluster) {
                // A plain entry before a cluster of its group.
                (None, Some(c2)) => group_cmp(e1.group, self.base_group(c2), -1),
                (Some(c1), None) => group_cmp(self.base_group(c1), e2.group, 1),
                (Some(c1), Some(c2)) => self.cluster_cmp(c1, c2),
                (None, None) => Ordering::Equal,
            };
        }
        if e1.group != e2.group {
            return group_cmp(e1.group, e2.group, 0);
        }
        let short1 = self.first_short(e1);
        let short2 = self.first_short(e2);
        // SAFETY: each entry's first option.
        let (o1, o2) = unsafe { (opt_of(e1, 0), opt_of(e2, 0)) };
        let mut long1 = Self::first_long(e1);
        let mut long2 = Self::first_long(e2);
        let mut doc1 = is_doc(o1);
        let mut doc2 = is_doc(o2);
        if doc1 {
            doc1 = long1.is_some_and(|l| {
                let (canon, non_opt) = canon_doc(l);
                long1 = Some(canon);
                non_opt
            });
        }
        if doc2 {
            doc2 = long2.is_some_and(|l| {
                let (canon, non_opt) = canon_doc(l);
                long2 = Some(canon);
                non_opt
            });
        }
        if doc1 != doc2 {
            // Documentation after the options.
            return doc1.cmp(&doc2);
        }
        if short1 == 0
            && short2 == 0
            && let (Some(l1), Some(l2)) = (long1, long2)
        {
            return strcase_cmp(l1, l2);
        }
        let first = |s: u8, l: Option<&[u8]>| {
            if s != 0 {
                s
            } else {
                l.and_then(|l| l.first().copied()).unwrap_or(0)
            }
        };
        let f1 = first(short1, long1);
        let f2 = first(short2, long2);
        // `char`s, signed, as glibc compares them.
        #[allow(clippy::cast_possible_wrap)]
        let (s1, s2) = (i32::from(f1 as i8), i32::from(f2 as i8));
        let lower = crate::ctype::tolower(s1).saturating_sub(crate::ctype::tolower(s2));
        if lower != 0 {
            return lower.cmp(&0);
        }
        // The same letter: the lower-case one first.
        s2.cmp(&s1)
    }

    /// The entries in help order: a stable sort, as glibc's merge sort is.
    fn sort(&mut self) {
        let n = self.entries.len();
        for i in 1..n {
            let mut j = i;
            while j > 0 {
                let (Some(a), Some(b)) =
                    (self.entries.get(j.saturating_sub(1)), self.entries.get(j))
                else {
                    break;
                };
                if self.entry_cmp(a, b) != core::cmp::Ordering::Greater {
                    break;
                }
                self.entries.as_mut_slice().swap(j.saturating_sub(1), j);
                j = j.saturating_sub(1);
            }
        }
    }
}

/// Groups in help order: 0 and up ascending, then the negative ones
/// ascending; `eq`'s sign for equal ones -- glibc's `group_cmp`.
fn group_cmp(g1: i32, g2: i32, eq: isize) -> core::cmp::Ordering {
    if g1 == g2 {
        return eq.cmp(&0);
    }
    if (g1 < 0) == (g2 < 0) {
        g1.cmp(&g2)
    } else {
        g2.cmp(&g1)
    }
}

/// A documentation entry's name as it is sorted: blanks, then anything not
/// a letter or digit, skipped -- and whether it does not look like an
/// option (no `-` first): glibc's `canon_doc_option`.
fn canon_doc(name: &[u8]) -> (&[u8], bool) {
    let from = |s: &'_ [u8], keep: fn(u8) -> bool| -> usize {
        s.iter().position(|&c| keep(c)).unwrap_or(s.len())
    };
    let s = name
        .get(from(name, |c| crate::ctype::isspace(i32::from(c)) == 0)..)
        .unwrap_or(&[]);
    let non_opt = s.first() != Some(&b'-');
    let s = s
        .get(from(s, |c| crate::ctype::isalnum(i32::from(c)) != 0)..)
        .unwrap_or(&[]);
    (s, non_opt)
}

/// `strcasecmp`, in the C locale.
fn strcase_cmp(a: &[u8], b: &[u8]) -> core::cmp::Ordering {
    let lower = |c: u8| crate::ctype::tolower(i32::from(c));
    let n = a.len().max(b.len());
    for i in 0..=n {
        let x = a.get(i).map_or(0, |&c| lower(c));
        let y = b.get(i).map_or(0, |&c| lower(c));
        if x != y || x == 0 {
            return x.cmp(&y);
        }
    }
    core::cmp::Ordering::Equal
}

// ---------------------------------------------------------------------------
// Printing.
// ---------------------------------------------------------------------------

/// What one message carries from entry to entry: glibc's `hol_help_state`
/// and the parts of `pentry_state` that last.
struct Help<'a> {
    hol: &'a Hol,
    params: Params,
    state: *const ArgpState,
    fs: FmtStream,
    /// The entry printed before, if any.
    prev: Option<usize>,
    /// Set once a header is printed: groups are set apart after that.
    sep_groups: bool,
    /// An option's argument was left out of its short form.
    suppressed_dup_arg: bool,
}

/// `text` through `argp`'s help filter, if it has one: the text to print,
/// and whether it is the filter's own, to be freed.
///
/// # Safety
///
/// `argp` is NULL or an argp; `state` NULL or a parse's state; `text` NULL
/// or a C string.
unsafe fn filter(
    text: *const u8,
    key: i32,
    argp: *const Argp,
    state: *const ArgpState,
) -> (*const u8, bool) {
    // SAFETY: the caller's argp.
    match unsafe { argp.as_ref() }.and_then(|a| a.help_filter) {
        Some(f) => {
            // SAFETY: the parse's input for the argp, and the program's filter.
            let input = unsafe { super::parse::input_of(argp, state) };
            // SAFETY: as above.
            let out = unsafe { f(key, text, input) }.cast_const();
            (out, out != text && !out.is_null())
        }
        None => (text, false),
    }
}

/// A filter's own string given back.
fn free_filtered(p: *const u8, own: bool) {
    if own {
        // SAFETY: the filter's answer, `malloc`ed as the interface has it,
        // and not the text it was given.
        unsafe { crate::malloc::free(p.cast_mut()) };
    }
}

impl Help<'_> {
    /// Blanks to `col`: glibc's `indent_to`.
    fn indent_to(&mut self, col: usize) {
        let point = self.fs.point();
        for _ in point..col {
            self.fs.putc(b' ');
        }
    }

    /// A newline if the next `ensure` bytes would reach the margin, else a
    /// blank: glibc's `space`, which keeps an item with a blank in it from
    /// being broken there.
    fn space(&mut self, ensure: usize) {
        if self.fs.point().saturating_add(ensure) >= self.fs.rmargin() {
            self.fs.putc(b'\n');
        } else {
            self.fs.putc(b' ');
        }
    }

    /// The option's argument, in `req` or `opt`'s shape: glibc's `arg`.
    fn arg(&mut self, real: &ArgpOption, req: (&[u8], &[u8]), opt: (&[u8], &[u8])) {
        if !real.arg.is_null() {
            // SAFETY: a C string.
            let a = unsafe { text(real.arg) };
            let (before, after) = if real.flags & OPTION_ARG_OPTIONAL != 0 {
                opt
            } else {
                req
            };
            self.fs.print(&[before, a, after]);
        }
    }

    /// A header line: glibc's `print_header`.
    ///
    /// # Safety
    ///
    /// `header` a C string; `argp` the argp whose filter it goes through.
    unsafe fn header(&mut self, header: *const u8, argp: *const Argp) {
        // SAFETY: the caller's contract.
        let (s, own) = unsafe { filter(header, ARGP_KEY_HELP_HEADER, argp, self.state) };
        if !s.is_null() {
            // SAFETY: the filter's answer, a C string.
            let t = unsafe { text(s) };
            if !t.is_empty() {
                if self.prev.is_some() {
                    self.fs.putc(b'\n');
                }
                let hc = col(self.params.header_col);
                self.indent_to(hc);
                self.fs.set_lmargin(hc);
                self.fs.set_wmargin(hc);
                self.fs.write(t);
                self.fs.set_lmargin(0);
                self.fs.putc(b'\n');
            }
            self.sep_groups = true;
        }
        free_filtered(s, own);
    }

    /// Before an entry's first name: the blank between groups and the
    /// cluster's header; before any other, a comma -- then blanks to `col`:
    /// glibc's `comma`.
    fn comma(&mut self, col: usize, entry: usize, first: &mut bool) {
        if *first {
            let pe = self.prev.and_then(|p| self.hol.entries.get(p));
            let e = self.hol.entries.get(entry);
            if self.sep_groups
                && let (Some(pe), Some(e)) = (pe, e)
                && e.group != pe.group
            {
                self.fs.putc(b'\n');
            }
            if let Some(cl_i) = e.and_then(|e| e.cluster)
                && let Some(cl) = self.hol.clusters.get(cl_i)
                && !cl.header.is_null()
                // SAFETY: a C string.
                && !unsafe { text(cl.header) }.is_empty()
            {
                let pe_cluster = pe.and_then(|pe| pe.cluster);
                let changing = match pe {
                    None => true,
                    Some(_) => pe_cluster != Some(cl_i) && !self.is_child(pe_cluster, cl_i),
                };
                if changing {
                    let old_wm = self.fs.wmargin();
                    let (h, a) = (cl.header, cl.argp);
                    // SAFETY: the cluster's header and its argp.
                    unsafe { self.header(h, a) };
                    self.fs.set_wmargin(old_wm);
                }
            }
            *first = false;
        } else {
            self.fs.write(b", ");
        }
        self.indent_to(col);
    }

    /// Whether `cl` is `of` or one of its ancestors: glibc's
    /// `hol_cluster_is_child(of, cl)`.
    fn is_child(&self, mut of: Option<usize>, cl: usize) -> bool {
        while let Some(c) = of {
            if c == cl {
                return true;
            }
            of = self.hol.clusters.get(c).and_then(|x| x.parent);
        }
        false
    }

    /// One entry: its options in their columns and its text: glibc's
    /// `hol_entry_help`.
    fn entry(&mut self, ei: usize) {
        let hol = self.hol;
        let Some(e) = hol.entries.get(ei) else {
            return;
        };
        // SAFETY: the entry's first option.
        let real = unsafe { opt_of(e, 0) };
        let old_lm = self.fs.set_lmargin(0);
        let old_wm = self.fs.wmargin();
        let mut first = true;
        let p = self.params;
        let have_long = !is_doc(real)
            && (0..e.num).any(|k| {
                // SAFETY: `k < num`.
                let o = unsafe { opt_of(e, k) };
                !o.name.is_null() && visible(o)
            });

        // The short options.
        self.fs.set_wmargin(col(p.short_opt_col));
        let mut so = e.short_at;
        for k in 0..e.num {
            // SAFETY: `k < num`.
            let o = unsafe { opt_of(e, k) };
            if is_short(o) && o.key == i32::from(hol.short_at(so)) {
                if visible(o) {
                    self.comma(col(p.short_opt_col), ei, &mut first);
                    self.fs.putc(b'-');
                    self.fs.putc(hol.short_at(so));
                    if !have_long || p.dup_args != 0 {
                        self.arg(real, (b" ", b""), (b"[", b"]"));
                    } else if !real.arg.is_null() {
                        self.suppressed_dup_arg = true;
                    }
                }
                so = so.saturating_add(1);
            }
        }

        // The long options, or a documentation entry's names.
        if is_doc(real) {
            self.fs.set_wmargin(col(p.doc_opt_col));
            for k in 0..e.num {
                // SAFETY: `k < num`.
                let o = unsafe { opt_of(e, k) };
                // SAFETY: a C string.
                if !o.name.is_null() && !unsafe { text(o.name) }.is_empty() && visible(o) {
                    self.comma(col(p.doc_opt_col), ei, &mut first);
                    // SAFETY: as above.
                    self.fs.write(unsafe { text(o.name) });
                }
            }
        } else {
            self.fs.set_wmargin(col(p.long_opt_col));
            for k in 0..e.num {
                // SAFETY: `k < num`.
                let o = unsafe { opt_of(e, k) };
                if !o.name.is_null() && visible(o) {
                    self.comma(col(p.long_opt_col), ei, &mut first);
                    // SAFETY: a C string.
                    self.fs.print(&[b"--", unsafe { text(o.name) }]);
                    self.arg(real, (b"=", b""), (b"[=", b"]"));
                }
            }
        }

        // The text.
        self.fs.set_lmargin(0);
        if first {
            // Nothing printed: a group's header, or an option hidden whole.
            if !is_short(real) && real.name.is_null() {
                // SAFETY: the header's text, and its argp.
                unsafe { self.header(real.doc, e.argp) };
            } else {
                self.fs.set_lmargin(old_lm);
                self.fs.set_wmargin(old_wm);
                return;
            }
        } else {
            // SAFETY: the option's doc, and its argp's filter.
            let (s, own) = unsafe { filter(real.doc, real.key, e.argp, self.state) };
            // SAFETY: the filter's answer, a C string.
            if !s.is_null() && !unsafe { text(s) }.is_empty() {
                let at = self.fs.point();
                let dc = col(p.opt_doc_col);
                self.fs.set_lmargin(dc);
                self.fs.set_wmargin(dc);
                if at > dc.saturating_add(3) {
                    self.fs.putc(b'\n');
                } else if at >= dc {
                    self.fs.write(b"   ");
                } else {
                    self.indent_to(dc);
                }
                // SAFETY: as above.
                self.fs.write(unsafe { text(s) });
            }
            free_filtered(s, own);
            self.fs.set_lmargin(0);
            self.fs.putc(b'\n');
        }
        self.prev = Some(ei);
        self.fs.set_lmargin(old_lm);
        self.fs.set_wmargin(old_wm);
    }

    /// Every entry, then the note that a short option's argument is its long
    /// one's: glibc's `hol_help`.
    fn options(&mut self) {
        for ei in 0..self.hol.entries.len() {
            self.entry(ei);
        }
        if self.suppressed_dup_arg && self.params.dup_args_note != 0 {
            const NOTE: &core::ffi::CStr = c"Mandatory or optional arguments to long options are also mandatory or optional for any corresponding short options.";
            // SAFETY: NULL or a parse's state.
            let root = unsafe { self.state.as_ref() }.map_or(core::ptr::null(), |s| s.root_argp);
            // SAFETY: the note, the root argp's filter.
            let (s, own) = unsafe {
                filter(
                    NOTE.as_ptr().cast(),
                    ARGP_KEY_HELP_DUP_ARGS_NOTE,
                    root,
                    self.state,
                )
            };
            // SAFETY: a C string.
            if !s.is_null() && !unsafe { text(s) }.is_empty() {
                self.fs.putc(b'\n');
                // SAFETY: as above.
                self.fs.write(unsafe { text(s) });
                self.fs.putc(b'\n');
            }
            free_filtered(s, own);
        }
    }

    /// The options in the usage line: glibc's `hol_usage`.
    fn usage_options(&mut self) {
        let hol = self.hol;
        if hol.entries.is_empty() {
            return;
        }
        // Short options without arguments, in one bracket.
        let mut letters: List<u8> = List::new();
        for e in hol.entries.iter() {
            hol.shorts_of(e, |o, real| {
                if o.arg.is_null()
                    && real.arg.is_null()
                    && (o.flags | real.flags) & OPTION_NO_USAGE == 0
                {
                    let _ = letters.push(u8::try_from(o.key).unwrap_or(0));
                }
                false
            });
        }
        if !letters.is_empty() {
            self.fs.print(&[b" [-", letters.as_slice(), b"]"]);
        }
        // Short options with them, one bracket each.
        for e in hol.entries.iter() {
            hol.shorts_of(e, |o, real| {
                let a = if o.arg.is_null() { real.arg } else { o.arg };
                let flags = o.flags | real.flags;
                if !a.is_null() && flags & OPTION_NO_USAGE == 0 {
                    // SAFETY: a C string.
                    let a = unsafe { text(a) };
                    let c = [u8::try_from(o.key).unwrap_or(0)];
                    if flags & OPTION_ARG_OPTIONAL != 0 {
                        self.fs.print(&[b" [-", &c, b"[", a, b"]]"]);
                    } else {
                        // Wrapped by hand, so that it is not broken at its blank.
                        self.space(6usize.saturating_add(a.len()));
                        self.fs.print(&[b"[-", &c, b" ", a, b"]"]);
                    }
                }
                false
            });
        }
        // Every long option.
        for e in hol.entries.iter() {
            Hol::longs_of(e, |o, real| {
                let a = if o.arg.is_null() { real.arg } else { o.arg };
                let flags = o.flags | real.flags;
                // A documentation entry is no option: not listed (glibc's
                // lists it as one, `[--FILE]`).
                if flags & OPTION_NO_USAGE == 0 && !is_doc(o) {
                    // SAFETY: C strings.
                    let name = unsafe { text(o.name) };
                    if a.is_null() {
                        self.fs.print(&[b" [--", name, b"]"]);
                    } else {
                        // SAFETY: as above.
                        let a = unsafe { text(a) };
                        if flags & OPTION_ARG_OPTIONAL != 0 {
                            self.fs.print(&[b" [--", name, b"[=", a, b"]]"]);
                        } else {
                            self.fs.print(&[b" [--", name, b"=", a, b"]"]);
                        }
                    }
                }
                false
            });
        }
    }

    /// The arguments in the usage line, the `level`s alternative of each
    /// argp's: glibc's `argp_args_usage`. True if the alternatives have not
    /// all been printed.
    ///
    /// # Safety
    ///
    /// `argp` is a valid argp tree.
    unsafe fn usage_args(
        &mut self,
        argp: *const Argp,
        levels: &mut [usize],
        at: &mut usize,
        mut advance: bool,
    ) -> bool {
        // SAFETY: the caller's tree.
        let a = unsafe { &*argp };
        let our = *at;
        let mut multiple = false;
        let mut more = false;
        // SAFETY: the argp's args_doc, through its filter.
        let (s, own) = unsafe { filter(a.args_doc, ARGP_KEY_HELP_ARGS_DOC, argp, self.state) };
        if !s.is_null() {
            // SAFETY: a C string.
            let doc = unsafe { text(s) };
            let mut parts = doc.split(|&c| c == b'\n');
            let mut part = parts.next().unwrap_or(b"");
            if doc.contains(&b'\n') {
                multiple = true;
                let level = levels.get(our).copied().unwrap_or(0);
                for _ in 0..level {
                    part = parts.next().unwrap_or(b"");
                }
                more = parts.next().is_some();
                *at = at.saturating_add(1);
            }
            self.space(part.len().saturating_add(1));
            self.fs.write(part);
        }
        free_filtered(s, own);
        if !a.children.is_null() {
            // SAFETY: a list ended by an entry whose argp is NULL.
            unsafe {
                let mut c = a.children;
                while !(*c).argp.is_null() {
                    advance = !self.usage_args((*c).argp, levels, at, advance);
                    c = c.add(1);
                }
            }
        }
        if advance && multiple {
            if let Some(l) = levels.get_mut(our) {
                if more {
                    *l = l.saturating_add(1);
                    advance = false;
                } else {
                    *l = 0;
                }
            }
        }
        !advance
    }

    /// The text before (`post` false) or after the options, of the argp and
    /// -- before, until one has some; after, all -- its children: glibc's
    /// `argp_doc`.
    ///
    /// # Safety
    ///
    /// `argp` is a valid argp tree.
    unsafe fn doc(
        &mut self,
        argp: *const Argp,
        post: bool,
        pre_blank: bool,
        first_only: bool,
    ) -> bool {
        // SAFETY: the caller's tree.
        let a = unsafe { &*argp };
        let mut anything = false;
        // The part of the doc this asks for, as a C string of its own when
        // it is the part before a `\v`.
        let mut copy: List<u8> = List::new();
        let inp: *const u8 = if a.doc.is_null() {
            core::ptr::null()
        } else {
            // SAFETY: a C string.
            let d = unsafe { text(a.doc) };
            match d.iter().position(|&c| c == 0x0B) {
                Some(vt) if post => {
                    // SAFETY: within the string, or at its NUL.
                    let after = unsafe { a.doc.add(vt.saturating_add(1)) };
                    // SAFETY: as above.
                    if unsafe { *after } == 0 {
                        core::ptr::null()
                    } else {
                        after
                    }
                }
                Some(vt) => {
                    if vt == 0
                        || copy.extend_from_slice(d.get(..vt).unwrap_or(b"")).is_err()
                        || copy.push(0).is_err()
                    {
                        core::ptr::null()
                    } else {
                        copy.as_slice().as_ptr()
                    }
                }
                None if post => core::ptr::null(),
                None => a.doc,
            }
        };
        let key = if post {
            ARGP_KEY_HELP_POST_DOC
        } else {
            ARGP_KEY_HELP_PRE_DOC
        };
        // SAFETY: the doc's part, through the argp's filter.
        let (s, own) = unsafe { filter(inp, key, argp, self.state) };
        if !s.is_null() {
            if pre_blank {
                self.fs.putc(b'\n');
            }
            // SAFETY: a C string.
            self.fs.write(unsafe { text(s) });
            if self.fs.point() > self.fs.lmargin() {
                self.fs.putc(b'\n');
            }
            anything = true;
        }
        free_filtered(s, own);
        if post && let Some(f) = a.help_filter {
            // SAFETY: the parse's input for this argp, the program's filter.
            let input: *mut c_void = unsafe { super::parse::input_of(argp, self.state) };
            // SAFETY: as above.
            let extra = unsafe { f(ARGP_KEY_HELP_EXTRA, core::ptr::null(), input) };
            if !extra.is_null() {
                if anything || pre_blank {
                    self.fs.putc(b'\n');
                }
                // SAFETY: the filter's string.
                self.fs.write(unsafe { text(extra) });
                // SAFETY: as above, `malloc`ed, the filter having been given
                // no text.
                unsafe { crate::malloc::free(extra) };
                if self.fs.point() > self.fs.lmargin() {
                    self.fs.putc(b'\n');
                }
                anything = true;
            }
        }
        if !a.children.is_null() {
            // SAFETY: a list ended by an entry whose argp is NULL.
            unsafe {
                let mut c = a.children;
                while !((*c).argp.is_null() || (first_only && anything)) {
                    anything |= self.doc((*c).argp, post, anything || pre_blank, first_only);
                    c = c.add(1);
                }
            }
        }
        anything
    }
}

/// How many argps of the tree have alternatives in their args_doc: glibc's
/// `argp_args_levels`.
///
/// # Safety
///
/// `argp` is a valid argp tree.
unsafe fn levels(argp: *const Argp) -> usize {
    // SAFETY: the caller's tree.
    let a = unsafe { &*argp };
    // SAFETY: a C string.
    let mut n = usize::from(!a.args_doc.is_null() && unsafe { text(a.args_doc) }.contains(&b'\n'));
    if !a.children.is_null() {
        // SAFETY: a list ended by an entry whose argp is NULL.
        unsafe {
            let mut c = a.children;
            while !(*c).argp.is_null() {
                n = n.saturating_add(levels((*c).argp));
                c = c.add(1);
            }
        }
    }
    n
}

/// The help `flags` asks for, of `argp`, on `stream`: glibc's `_help`.
///
/// # Safety
///
/// `argp` is NULL or a valid argp tree; `state` NULL or the parse's;
/// `stream` NULL or a `FILE *`; `name` NULL or a C string.
pub(crate) unsafe fn help(
    argp: *const Argp,
    state: *const ArgpState,
    stream: *mut u8,
    mut flags: u32,
    name: *mut u8,
) {
    if stream.is_null() || argp.is_null() {
        return;
    }
    let _l = super::Locked::new(stream);
    // SAFETY: the caller's state.
    let params = unsafe { params(state) };
    let Some(fs) = FmtStream::new(stream, 0, col(params.rmargin), 0) else {
        return;
    };
    let empty = Hol {
        entries: List::new(),
        clusters: List::new(),
        shorts: List::new(),
    };
    let built;
    let hol = if flags & (ARGP_HELP_USAGE | ARGP_HELP_SHORT_USAGE | ARGP_HELP_LONG) != 0 {
        // SAFETY: the caller's tree.
        match unsafe { Hol::of(argp) } {
            Ok(h) => {
                built = h;
                &built
            }
            Err(NoMem) => {
                fs.finish();
                return;
            }
        }
    } else {
        &empty
    };
    let mut h = Help {
        hol,
        params,
        state,
        fs,
        prev: None,
        sep_groups: false,
        suppressed_dup_arg: false,
    };
    // SAFETY: a C string, or NULL ("(null)").
    let name = unsafe { text(name) };
    let mut anything = false;

    if flags & (ARGP_HELP_USAGE | ARGP_HELP_SHORT_USAGE) != 0 {
        // SAFETY: the caller's tree.
        let n = unsafe { levels(argp) };
        let mut level_list: List<usize> = List::new();
        if level_list.resize(n, 0).is_err() {
            h.fs.finish();
            return;
        }
        let mut first_pattern = true;
        loop {
            let old_wm = h.fs.set_wmargin(col(params.usage_indent));
            h.fs.print(&[
                if first_pattern { b"Usage:" } else { b"  or: " },
                b" ",
                name,
            ]);
            let old_lm = h.fs.set_lmargin(col(params.usage_indent));
            if flags & ARGP_HELP_SHORT_USAGE != 0 {
                if !h.hol.entries.is_empty() {
                    h.fs.write(b" [OPTION...]");
                }
            } else {
                h.usage_options();
                flags |= ARGP_HELP_SHORT_USAGE;
            }
            let mut at = 0usize;
            // SAFETY: the caller's tree.
            let more = unsafe { h.usage_args(argp, level_list.as_mut_slice(), &mut at, true) };
            h.fs.set_wmargin(old_wm);
            h.fs.set_lmargin(old_lm);
            h.fs.putc(b'\n');
            anything = true;
            first_pattern = false;
            if !more {
                break;
            }
        }
    }
    if flags & ARGP_HELP_PRE_DOC != 0 {
        // SAFETY: the caller's tree.
        anything |= unsafe { h.doc(argp, false, false, true) };
    }
    if flags & ARGP_HELP_SEE != 0 {
        h.fs.print(&[
            b"Try `",
            name,
            b" --help' or `",
            name,
            b" --usage' for more information.\n",
        ]);
        anything = true;
    }
    if flags & ARGP_HELP_LONG != 0 && !h.hol.entries.is_empty() {
        if anything {
            h.fs.putc(b'\n');
        }
        h.options();
        anything = true;
    }
    if flags & ARGP_HELP_POST_DOC != 0 {
        // SAFETY: the caller's tree.
        anything |= unsafe { h.doc(argp, true, anything, false) };
    }
    let bug = super::bug_address();
    if flags & ARGP_HELP_BUG_ADDR != 0 && !bug.is_null() {
        if anything {
            h.fs.putc(b'\n');
        }
        // SAFETY: the program's C string.
        h.fs.print(&[b"Report bugs to ", unsafe { text(bug) }, b".\n"]);
    }
    h.fs.finish();
}
