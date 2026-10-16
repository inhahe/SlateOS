//! `comp_parse.c`: every entry of a source read into the list, and their
//! `use=` clauses resolved -- against the other entries of the source,
//! then the compiled database -- and merged in.
//!
//! Two places differ from upstream on purpose:
//!
//! - **Duplicate names.** Upstream checks the list for two entries sharing
//!   a name only for pairs where the later entry's memory happens to lie
//!   below the earlier's (`qp > rp` compares heap addresses). With glibc's
//!   allocator that never happens -- measured on small entries and on
//!   twelve copies of `xterm-256color` -- so the reference never reports
//!   it, and neither does this.
//! - **`use=` cycles.** Upstream merges until no entry has a `use=` left,
//!   and an entry that uses itself through others never runs out of them:
//!   it spins forever, printing nothing. Here, a pass that merges nothing
//!   ends it: each `use=` still waiting is reported as upstream reports one
//!   it cannot find (`resolution of use=NAME failed`), and resolution fails
//!   as it then does.

use super::caps::s::{ACS_CHARS, ENTER_ALT_CHARSET_MODE, EXIT_ALT_CHARSET_MODE};
use super::entry::{Entry, Link, merge_entry, wrap_entry};
use super::parse::{Compiler, Parsed, VT_ACSC};
use super::scan::cstr;
use super::tables::{first_name, name_match};
use super::{Abort, MAX_NAME_SIZE};
use crate::termtype::{Str, TermType};

/// `force_bar (dst, src)`: a name field with no `|` given one at its end
/// -- at most `MAX_NAME_SIZE` bytes of it first.
fn force_bar(src: &[u8]) -> Vec<u8> {
    if src.contains(&b'|') {
        return src.to_vec();
    }
    let mut v = src
        .get(..src.len().min(MAX_NAME_SIZE))
        .unwrap_or(src)
        .to_vec();
    v.push(b'|');
    v
}

/// `check_collisions (n1, n2, 0)`: whether two name fields share a name --
/// comparing the names each `|` ends, so a field's last part, its
/// description, takes part only where it ends in a `|`.
fn check_collisions(n1: &[u8], n2: &[u8]) -> bool {
    let n1 = force_bar(n1);
    let n2 = force_bar(n2);
    let names = |f: &[u8]| -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let mut rest = f;
        while let Some(bar) = rest.iter().position(|&c| c == b'|') {
            out.push(rest.get(..bar).unwrap_or_default().to_vec());
            rest = rest.get(bar.saturating_add(1)..).unwrap_or_default();
        }
        out
    };
    let a = names(&n1);
    let b = names(&n2);
    a.iter().any(|p| b.contains(p))
}

/// `_nc_entry_match (n1, n2)`: whether two name fields share a name.
#[must_use]
pub fn entry_match(n1: &[u8], n2: &[u8]) -> bool {
    check_collisions(n1, n2)
}

/// Where the compiler reads a source from: `_nc_reset_input`'s file or
/// string.
pub enum Source {
    /// A file, at `offset` (`None` where it cannot seek).
    File(Box<dyn std::io::Read>, Option<u64>),
    /// A string.
    Buffer(Vec<u8>),
}

impl Compiler<'_> {
    /// `_nc_read_entry_source (fp, buf, literal, silent, NULLHOOK)`: every
    /// entry of the source onto the list.
    ///
    /// # Errors
    ///
    /// An error that ends the compile, its message written.
    pub fn read_entry_source(
        &mut self,
        source: Source,
        literal: bool,
        silent: bool,
    ) -> Result<(), Abort> {
        let oldsuppress = self.scan.suppress_warnings;
        if silent {
            // "shut the lexer up, too"
            self.scan.suppress_warnings = true;
        }
        match source {
            Source::File(reader, offset) => self.scan.reset_input_file(reader, offset),
            Source::Buffer(buf) => self.scan.reset_input_buffer(&buf),
        }
        let result = (|| -> Result<(), Abort> {
            loop {
                let mut thisentry = Entry::default();
                match self.parse_entry(&mut thisentry, literal, silent)? {
                    Parsed::Ok => {}
                    // `EOF` is `ERR`: either ends the source.
                    Parsed::Eof | Parsed::Err => break,
                }
                if !thisentry
                    .tterm
                    .term_names
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                {
                    return Err(self
                        .scan
                        .err_abort(b"terminal names must start with letter or digit"));
                }
                self.entries.push(thisentry);
            }
            Ok(())
        })();
        self.scan.suppress_warnings = oldsuppress;
        result
    }

    /// The description `link` names.
    fn linked(&self, link: Link) -> Option<&Entry> {
        match link {
            Link::Core(i) => self.entries.get(i),
            Link::Disk(i) => self.disk.get(i),
        }
    }

    /// `_nc_resolve_uses2 (fullresolve, literal)`: every `use=` linked to
    /// the entry it names; with `fullresolve`, merged in, and each entry
    /// checked. False if one could not be.
    ///
    /// # Errors
    ///
    /// An error that ends the compile, its message written.
    #[allow(
        clippy::too_many_lines,
        reason = "upstream's _nc_resolve_uses2, in one piece so it reads against it"
    )]
    pub fn resolve_uses2(&mut self, fullresolve: bool, literal: bool) -> Result<bool, Abort> {
        // "First resolution stage: compute link pointers corresponding to
        // names."
        let mut total_unresolved = 0usize;
        self.scan.curr_col = -1;
        for qp in 0..self.entries.len() {
            let nuses = self.entries.get(qp).map_or(0, |e| e.nuses);
            for i in 0..nuses {
                let Some(entry) = self.entries.get(qp) else {
                    break;
                };
                let child = first_name(&entry.tterm.term_names);
                let Some(use_) = entry.uses.get(i) else {
                    continue;
                };
                let Some(lookfor) = use_.name.clone() else {
                    continue;
                };
                let lookline = use_.line;
                let mut foundit = false;
                self.scan.set_type(&child);

                // "first, try to resolve from in-core records"
                for rp in 0..self.entries.len() {
                    let matched = rp != qp
                        && self
                            .entries
                            .get(rp)
                            .is_some_and(|r| name_match(&r.tterm.term_names, &lookfor, b"|"));
                    if matched {
                        self.link_use(qp, i, Link::Core(rp), &lookfor);
                        foundit = true;
                    }
                }

                // "if that didn't work, try to merge in a compiled entry"
                if !foundit
                    && let Ok(thisterm) = crate::read_termtype_entry(
                        &lookfor,
                        &self.env,
                        self.tic_dir.as_deref(),
                        self.user_definable,
                    )
                {
                    self.disk.push(Entry {
                        tterm: thisterm,
                        ..Entry::default()
                    });
                    let at = self.disk.len().saturating_sub(1);
                    self.link_use(qp, i, Link::Disk(at), &lookfor);
                    foundit = true;
                }

                // "no good, mark this one unresolvable and complain"
                if !foundit {
                    total_unresolved = total_unresolved.saturating_add(1);
                    self.scan.curr_line = i32::try_from(lookline).unwrap_or(i32::MAX);
                    let mut m = b"resolution of use=".to_vec();
                    m.extend_from_slice(&lookfor);
                    m.extend_from_slice(b" failed");
                    self.scan.warning(&m);
                    if let Some(e) = self.entries.get_mut(qp) {
                        e.use_mut(i).link = None;
                    }
                }
            }
        }
        if total_unresolved != 0 {
            // "free entries read in off disk"
            self.disk.clear();
            return Ok(false);
        }

        // "Time to do the actual merges."
        if fullresolve {
            loop {
                let mut keepgoing = false;
                let mut merged_any = false;
                for qp in 0..self.entries.len() {
                    let Some(entry) = self.entries.get(qp) else {
                        break;
                    };
                    if entry.nuses == 0 {
                        continue;
                    }
                    keepgoing = true;
                    // "If any of the use entries we're looking for is
                    // incomplete, punt."
                    let incomplete = entry.uses.iter().take(entry.nuses).any(|u| {
                        u.link
                            .and_then(|l| self.linked(l))
                            .is_some_and(|r| r.nuses != 0)
                    });
                    if incomplete {
                        continue;
                    }
                    let mut merged: TermType = entry.tterm.clone();
                    // "Now merge in each use entry in the proper (reverse)
                    // order."
                    for n in (0..entry.nuses).rev() {
                        if let Some(r) = entry
                            .uses
                            .get(n)
                            .and_then(|u| u.link)
                            .and_then(|l| self.linked(l))
                        {
                            merge_entry(&mut merged, &r.tterm);
                        }
                    }
                    // "Now merge in the original entry."
                    merge_entry(&mut merged, &entry.tterm);
                    let Some(entry) = self.entries.get_mut(qp) else {
                        break;
                    };
                    entry.nuses = 0;
                    entry.tterm = merged;
                    let mut taken = std::mem::take(entry);
                    let wrapped = wrap_entry(&mut self.strbuf, &mut self.scan, &mut taken, true);
                    if let Some(slot) = self.entries.get_mut(qp) {
                        *slot = taken;
                    }
                    wrapped?;
                    merged_any = true;
                }
                if !keepgoing {
                    break;
                }
                if !merged_any {
                    self.report_cycles();
                    self.disk.clear();
                    return Ok(false);
                }
            }
        }

        if fullresolve {
            self.scan.curr_col = -1;
            for qp in 0..self.entries.len() {
                let Some(entry) = self.entries.get(qp) else {
                    break;
                };
                self.scan.curr_line = i32::try_from(entry.startline).unwrap_or(i32::MAX);
                let first = first_name(&entry.tterm.term_names);
                self.scan.set_type(&first);
                let Some(slot) = self.entries.get_mut(qp) else {
                    break;
                };
                let mut taken = std::mem::take(slot);
                if let Some(check) = self.check_termtype.as_mut() {
                    check.check(&mut self.scan, &mut taken.tterm, literal);
                } else {
                    fixup_acsc(&mut taken.tterm, literal);
                }
                if let Some(slot) = self.entries.get_mut(qp) {
                    *slot = taken;
                }
            }
        }
        Ok(true)
    }

    /// Link `use=` `i` of entry `qp` to `link`, warning when an earlier
    /// `use=` of the entry already names the same terminal.
    fn link_use(&mut self, qp: usize, i: usize, link: Link, lookfor: &[u8]) {
        let rp_names = self.linked(link).map(|r| r.tterm.term_names.clone());
        if let Some(e) = self.entries.get_mut(qp) {
            e.use_mut(i).link = Some(link);
        }
        // "verify that there are no earlier uses"
        let Some(entry) = self.entries.get(qp) else {
            return;
        };
        let duplicate = (0..i).any(|j| {
            entry
                .uses
                .get(j)
                .and_then(|u| u.link)
                .and_then(|l| self.linked(l))
                .is_some_and(|r| Some(&r.tterm.term_names) == rp_names.as_ref())
        });
        if duplicate {
            let mut m = b"duplicate use=".to_vec();
            m.extend_from_slice(lookfor);
            self.scan.warning(&m);
        }
    }

    /// Each `use=` a pass could not merge -- every entry still waiting is in
    /// a cycle or waits on one -- reported as unresolvable.
    fn report_cycles(&mut self) {
        for qp in 0..self.entries.len() {
            let Some(entry) = self.entries.get(qp) else {
                break;
            };
            let child = first_name(&entry.tterm.term_names);
            let waiting: Vec<(Vec<u8>, i64)> = entry
                .uses
                .iter()
                .take(entry.nuses)
                .filter(|u| {
                    u.link
                        .and_then(|l| self.linked(l))
                        .is_some_and(|r| r.nuses != 0)
                })
                .map(|u| (u.name.clone().unwrap_or_default(), u.line))
                .collect();
            if waiting.is_empty() {
                continue;
            }
            self.scan.set_type(&child);
            for (name, line) in waiting {
                self.scan.curr_line = i32::try_from(line).unwrap_or(i32::MAX);
                let mut m = b"resolution of use=".to_vec();
                m.extend_from_slice(cstr(&name));
                m.extend_from_slice(b" failed");
                self.scan.warning(&m);
            }
        }
    }
}

/// `fixup_acsc (tp, literal)`: a terminal that can switch to its alternate
/// characters but does not say what they are is given a VT100's.
pub fn fixup_acsc(tp: &mut TermType, literal: bool) {
    if !literal {
        let get = |i: usize| tp.strings.get(i).cloned().unwrap_or_default();
        if get(ACS_CHARS) == Str::Absent
            && get(ENTER_ALT_CHARSET_MODE).present()
            && get(EXIT_ALT_CHARSET_MODE).present()
            && let Some(slot) = tp.strings.get_mut(ACS_CHARS)
        {
            *slot = Str::Value(VT_ACSC.to_vec());
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::compile::caps::s;

    fn compile(source: &[u8]) -> (Vec<Entry>, bool, String) {
        let mut diag = Vec::new();
        let (entries, ok) = {
            let mut c = Compiler::new(&mut diag);
            c.scan.set_source(Some(b"t.src"));
            c.read_entry_source(
                Source::File(Box::new(std::io::Cursor::new(source.to_vec())), Some(0)),
                false,
                false,
            )
            .unwrap();
            let ok = c.resolve_uses2(true, false).unwrap();
            (c.entries, ok)
        };
        (entries, ok, String::from_utf8(diag).unwrap())
    }

    #[test]
    fn a_use_is_merged_under_the_entrys_own_values() {
        let (e, ok, diag) = compile(
            b"base|base terminal,\n\tam, cols#80, bel=^G,\n\
              top|top terminal,\n\tcols#132, bel@, use=base,\n",
        );
        assert!(ok);
        assert_eq!(diag, "");
        let t = &e[1].tterm;
        assert_eq!(t.booleans[1], 1);
        assert_eq!(t.numbers[0], 132);
        assert_eq!(t.strings[s::BELL], Str::Cancelled);
        assert_eq!(e[1].nuses, 0);
    }

    #[test]
    fn an_unknown_use_fails_resolution() {
        let (_, ok, diag) = compile(b"top|top terminal,\n\tuse=nonesuch-terminal,\n");
        assert!(!ok);
        assert_eq!(
            diag,
            "\"t.src\", line 2, terminal 'top': resolution of use=nonesuch-terminal failed\n"
        );
    }

    #[test]
    fn a_use_cycle_is_reported_not_spun_on() {
        let (_, ok, diag) =
            compile(b"aa|first entry,\n\tam, use=bb,\nbb|second entry,\n\tbw, use=aa,\n");
        assert!(!ok);
        assert_eq!(
            diag,
            "\"t.src\", line 2, terminal 'aa': resolution of use=bb failed\n\
             \"t.src\", line 4, terminal 'bb': resolution of use=aa failed\n"
        );
    }

    #[test]
    fn names_collide_on_any_shared_name() {
        assert!(entry_match(b"a|b|desc", b"c|b|other"));
        assert!(!entry_match(b"a|desc", b"desc|x"));
        assert!(entry_match(b"solo", b"solo"));
    }
}
