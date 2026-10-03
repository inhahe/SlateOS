//! `select.c`: which processes are shown.
//!
//! Three mechanisms, combined in `want_this_proc`: "all" (`-e`, `-A`, and BSD
//! `ax`), a sixteen-bit table indexed by four yes-or-no facts about a process
//! (is it ours, is it a session leader, has it no terminal, is it on our
//! terminal) that the simple options choose, and lists (`-p`, `-u`, `-t`,
//! ...) that a process matches by any member. Then `r` drops what is not
//! running, and `-N` inverts the answer.

use crate::items::Item;
use crate::items::Stack;
use crate::{Ps, per, sel, ss};

/// `strncmp (a, b, n) == 0` on strings that end where their slices do.
fn strncmp_eq(a: &[u8], b: &[u8], n: usize) -> bool {
    for k in 0..n {
        let x = a.get(k).copied().unwrap_or(0);
        let y = b.get(k).copied().unwrap_or(0);
        if x != y {
            return false;
        }
        if x == 0 {
            return true;
        }
    }
    true
}

impl Ps {
    /// `select_bits_setup`.
    pub fn select_bits_setup(&mut self) -> Result<(), &'static str> {
        if self.simple_select == 0 && !self.prefer_bsd_defaults {
            self.select_bits = 0xaa00;
            return Ok(());
        }
        let switch_val = if self.personality & per::NO_DEFAULT_G == 0
            && self.simple_select & (ss::U_A | ss::U_D) == 0
        {
            self.simple_select | ss::B_G
        } else {
            self.simple_select
        };
        self.select_bits = match switch_val {
            v if v == ss::U_A | ss::U_D => 0x3f3f,
            v if v == ss::U_A => 0x0303,
            v if v == ss::U_D => 0x3333,
            0 => 0x0202,
            v if v == ss::B_A => 0x0303,
            v if v == ss::B_X => 0x2222,
            v if v == ss::B_X | ss::B_A => 0x3333,
            v if v == ss::B_G => 0x0a0a,
            v if v == ss::B_G | ss::B_A => 0x0f0f,
            v if v == ss::B_G | ss::B_X => 0xaaaa,
            v if v == ss::B_G | ss::B_X | ss::B_A => {
                self.all_processes = true;
                self.simple_select = 0;
                self.select_bits
            }
            _ => return Err("process selection options conflict"),
        };
        Ok(())
    }

    /// `table_accept`.
    fn table_accept(&self, buf: &Stack) -> bool {
        let euid = self.rsv(Item::IdEuid, buf).u_int() == self.cached_euid;
        let leader = self.rsv(Item::IdSession, buf).s_int() == self.rsv(Item::IdTgid, buf).s_int();
        let tty = self.rsv(Item::Tty, buf).s_int();
        let index = u32::from(euid)
            | (u32::from(leader) << 1)
            | (u32::from(tty == 0) << 2)
            | (u32::from(tty == self.cached_tty) << 3);
        self.select_bits & (1u32 << index) != 0
    }

    /// `proc_was_listed`: whether any list names this process.
    fn proc_was_listed(&self, buf: &Stack) -> bool {
        let as_u32 = |v: i32| u32::from_le_bytes(v.to_le_bytes());
        let low = |v: u64| u32::try_from(v & 0xffff_ffff).unwrap_or(0);
        for node in &self.selection_list {
            let field = match node.typecode {
                sel::RUID => self.rsv(Item::IdRuid, buf).u_int(),
                sel::EUID => self.rsv(Item::IdEuid, buf).u_int(),
                sel::SUID => self.rsv(Item::IdSuid, buf).u_int(),
                sel::FUID => self.rsv(Item::IdFuid, buf).u_int(),
                sel::RGID => self.rsv(Item::IdRgid, buf).u_int(),
                sel::EGID => self.rsv(Item::IdEgid, buf).u_int(),
                sel::SGID => self.rsv(Item::IdSgid, buf).u_int(),
                sel::FGID => self.rsv(Item::IdFgid, buf).u_int(),
                sel::PGRP => as_u32(self.rsv(Item::IdPgrp, buf).s_int()),
                sel::PID | sel::PID_QUICK => as_u32(self.rsv(Item::IdTgid, buf).s_int()),
                sel::PPID => as_u32(self.rsv(Item::IdPpid, buf).s_int()),
                sel::TTY => as_u32(self.rsv(Item::Tty, buf).s_int()),
                sel::SESS => as_u32(self.rsv(Item::IdSession, buf).s_int()),
                sel::COMM => {
                    let cmd = self.rsv(Item::Cmd, buf).str().unwrap_or_default();
                    for want in node.u.iter().rev() {
                        if cmd.len() == 15 && want.cmd.len() >= 15 && strncmp_eq(cmd, &want.cmd, 15)
                        {
                            return true;
                        }
                        if strncmp_eq(cmd, &want.cmd, 63) {
                            return true;
                        }
                    }
                    continue;
                }
                _ => continue,
            };
            if node.u.iter().any(|v| low(v.num) == field) {
                return true;
            }
        }
        false
    }

    /// `want_this_proc`.
    pub fn want_this_proc(&self, buf: &Stack) -> bool {
        let mut accepted = self.all_processes
            || ((self.simple_select != 0 || self.selection_list.is_empty())
                && self.table_accept(buf))
            || self.proc_was_listed(buf);
        if self.running_only {
            let state = self.rsv(Item::State, buf).s_ch();
            if state != b'R' && state != b'D' {
                accepted = false;
            }
        }
        if self.negate_selection {
            return !accepted;
        }
        accepted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strncmp_stops_at_n_or_the_end() {
        assert!(strncmp_eq(b"abc", b"abc", 63));
        assert!(!strncmp_eq(b"abc", b"abcd", 63));
        assert!(strncmp_eq(b"abcdef", b"abcxyz", 3));
        assert!(!strncmp_eq(b"ab", b"abc", 3));
        assert!(strncmp_eq(b"", b"", 5));
    }
}
