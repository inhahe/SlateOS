//! util-linux's `include/optutils.h`: `err_exclusive_options`, the check
//! every util-linux program runs on each option it reads, refusing one that
//! a group of mutually exclusive options already has a member of.

/// `err_exclusive_options(c, longopts, excl, status)`: the refusal to print
/// -- the whole line, newline included -- when option `c` belongs to a
/// group in `excl` that another option already claimed, else `None`.
///
/// `excl` is upstream's `ul_excl_t` array: groups of option values, each in
/// ASCII order and the groups ordered by their first member, since
/// upstream's scan stops at the first group -- and the first member -- past
/// `c`. `status` holds, for each group, the option that claimed it (0 for
/// none), and has one entry per group.
///
/// The message names the group's members in `excl`'s order, each by its
/// first long option (`longopt`, upstream's `option_to_longopt`) or else
/// as `-c` when it is a graphic character, and the program as `short` with
/// its unprintable bytes escaped (design-decisions §370). Upstream prints
/// it to stderr and exits with `OPTUTILS_EXIT_CODE`, 1 unless the program
/// defines its own.
pub fn err_exclusive_options<'n>(
    c: i32,
    excl: &[&[i32]],
    status: &mut [i32],
    longopt: impl Fn(i32) -> Option<&'n str>,
    short: &[u8],
) -> Option<String> {
    for (group, st) in excl.iter().zip(status.iter_mut()) {
        if group.first().is_none_or(|&first| first > c) {
            break;
        }
        for &op in group.iter() {
            if op > c {
                break;
            }
            if op != c {
                continue;
            }
            if *st == 0 {
                *st = c;
            } else if *st != c {
                let mut msg = format!(
                    "{}: mutually exclusive arguments:",
                    quoting::escape_unprintable(short)
                );
                // At most 15 members, as `ul_excl_t` holds.
                for &member in group.iter().take(15) {
                    if let Some(name) = longopt(member) {
                        msg.push_str(" --");
                        msg.push_str(name);
                    } else if let Ok(b) = u8::try_from(member)
                        && b.is_ascii_graphic()
                    {
                        msg.push_str(" -");
                        msg.push(char::from(b));
                    }
                }
                msg.push('\n');
                return Some(msg);
            }
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXCL: [&[i32]; 3] = [
        &[b'C' as i32, b'N' as i32],
        &[b'J' as i32, b'x' as i32],
        &[b't' as i32, b'x' as i32],
    ];

    fn longopt(c: i32) -> Option<&'static str> {
        match u8::try_from(c).ok()? {
            b'C' => Some("table-column"),
            b'N' => Some("table-columns"),
            b'J' => Some("json"),
            b't' => Some("table"),
            b'x' => Some("fillrows"),
            _ => None,
        }
    }

    fn run(opts: &[u8]) -> Option<String> {
        let mut st = [0; 3];
        for &o in opts {
            if let Some(m) = err_exclusive_options(i32::from(o), &EXCL, &mut st, longopt, b"column")
            {
                return Some(m);
            }
        }
        None
    }

    #[test]
    fn an_option_is_refused_after_another_of_its_group() {
        assert_eq!(
            run(b"NC"),
            Some("column: mutually exclusive arguments: --table-column --table-columns\n".into())
        );
        assert_eq!(
            run(b"xJ"),
            Some("column: mutually exclusive arguments: --json --fillrows\n".into())
        );
        assert_eq!(
            run(b"tx"),
            Some("column: mutually exclusive arguments: --table --fillrows\n".into())
        );
    }

    #[test]
    fn repeating_an_option_or_mixing_groups_is_allowed() {
        assert_eq!(run(b"CCtJ"), None);
        assert_eq!(run(b"NNdLh"), None);
    }

    #[test]
    fn a_member_without_a_long_option_is_named_by_its_letter() {
        let mut st = [0; 1];
        let excl: [&[i32]; 1] = [&[b'a' as i32, b'b' as i32]];
        assert_eq!(
            err_exclusive_options(i32::from(b'a'), &excl, &mut st, |_| None, b"p"),
            None
        );
        assert_eq!(
            err_exclusive_options(i32::from(b'b'), &excl, &mut st, |_| None, b"p"),
            Some("p: mutually exclusive arguments: -a -b\n".into())
        );
    }
}
