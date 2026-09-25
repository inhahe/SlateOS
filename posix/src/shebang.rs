//! Interpreter scripts — the `#!` line, read the way Linux reads it.
//!
//! # Why this lives in libc
//!
//! On Linux the kernel runs scripts: `execve` on a file that starts with `#!`
//! loads the named interpreter instead (`fs/binfmt_script.c`). Our native
//! `SYS_PROCESS_EXEC` and `SYS_PROCESS_SPAWN_EX` take the *bytes* of an ELF
//! image, which libc reads itself (`spawn.rs`'s `load_elf`), so the step that
//! decides what those bytes are is ours. Until 2026-09-24 nothing took it:
//! `execve("./configure", …)` handed a shell script to the kernel's ELF loader
//! and got `ENOEXEC`, so no C or Rust program on SlateOS could run a script by
//! name, and `posix_spawn` could not either.
//!
//! This module is the pure half — parsing the line and rewriting the argument
//! list — so that every rule below is testable on the host, where there is no
//! kernel to exec anything. `spawn.rs` drives it.
//!
//! # The rules, all of them Linux's
//!
//! * Only the first `SHEBANG_BUF` (256) bytes are examined, as `BINPRM_BUF_SIZE`.
//! * The line ends at the first `\n`. With no `\n` in the window, the
//!   interpreter's name must still end inside it — a truncated *path* is
//!   `ENOEXEC` — while a truncated *argument* is allowed: the interpreter can
//!   re-read its own script.
//! * Spaces and tabs around the name are skipped. The name ends at the next
//!   space, tab or NUL; everything after that, less leading and trailing
//!   blanks, is **one** optional argument — `#!/usr/bin/env python3 -u` passes
//!   `python3 -u` as a single word, which is the classic surprise and is
//!   reproduced deliberately.
//! * The new argument list is `interpreter [argument] script arg1 …`: the
//!   caller's `argv[0]` is dropped and replaced by the script's path as the
//!   caller named it.
//! * A script may name another script, up to `MAX_INTERP_DEPTH` rewrites;
//!   the one after that is `ELOOP`.
//!
//! A carriage return is *not* stripped, again as Linux: a script saved with
//! CRLF names an interpreter ending in `\r`, which does not exist, and fails
//! with `ENOENT` rather than working here and nowhere else.

use crate::errno;
use core::ops::Range;

/// How much of a file the `#!` line may occupy — Linux's `BINPRM_BUF_SIZE`.
pub(crate) const SHEBANG_BUF: usize = 256;

/// Interpreter rewrites allowed before `ELOOP`.
///
/// Linux's `exec_binprm` fails once `depth > 5`, having tried depths 0 to 5:
/// six loads, so up to five rewrites followed by a real binary. Its comment
/// says "4 levels"; the code allows five, and the code is what programs meet.
pub(crate) const MAX_INTERP_DEPTH: usize = 5;

/// A parsed `#!` line: the interpreter and its one optional argument, as
/// ranges into the window they were parsed from.
///
/// Owning its window rather than borrowing the file is what lets the caller
/// release the script's image before loading the interpreter — the two are
/// never held at once.
#[derive(Clone)]
pub(crate) struct Shebang {
    window: [u8; SHEBANG_BUF],
    interp: Range<usize>,
    arg: Option<Range<usize>>,
}

impl Shebang {
    /// The interpreter path, exactly as written: no NUL, no surrounding
    /// blanks, never empty unless the line itself named a NUL.
    pub(crate) fn interp(&self) -> &[u8] {
        self.window.get(self.interp.clone()).unwrap_or(&[])
    }

    /// The optional argument, as one word, blanks inside it preserved.
    pub(crate) fn arg(&self) -> Option<&[u8]> {
        self.arg.clone().and_then(|r| self.window.get(r))
    }
}

impl core::fmt::Debug for Shebang {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Shebang")
            .field("interp", &self.interp())
            .field("arg", &self.arg())
            .finish()
    }
}

fn is_blank(c: u8) -> bool {
    c == b' ' || c == b'\t'
}

/// Parse the start of a file as `binfmt_script` does.
///
/// * `None` — not a script: the file does not begin with `#!`.
/// * `Some(Err(ENOEXEC))` — a `#!` line that names no usable interpreter.
/// * `Some(Ok(_))` — the interpreter, and its argument if there is one.
///
/// `head` may be the whole file or any prefix of at least `SHEBANG_BUF`
/// bytes; beyond that nothing is read. A file shorter than the window is
/// treated as NUL-padded, as the kernel's buffer is.
pub(crate) fn parse(head: &[u8]) -> Option<Result<Shebang, i32>> {
    if !head.starts_with(b"#!") {
        return None;
    }
    let mut window = [0u8; SHEBANG_BUF];
    let n = head.len().min(SHEBANG_BUF);
    if let (Some(dst), Some(src)) = (window.get_mut(..n), head.get(..n)) {
        dst.copy_from_slice(src);
    }
    Some(parse_window(window))
}

/// The body of [`parse`], over the zero-padded window. Every index here is in
/// `0..SHEBANG_BUF`; the helpers below take an inclusive `first..=last` range,
/// the shape of the C they transcribe (`next_non_spacetab`,
/// `next_terminator`).
fn parse_window(window: [u8; SHEBANG_BUF]) -> Result<Shebang, i32> {
    let at = |i: usize| window.get(i).copied().unwrap_or(0);
    let next_non_blank = |first: usize, last: usize| (first..=last).find(|&i| !is_blank(at(i)));
    let next_terminator =
        |first: usize, last: usize| (first..=last).find(|&i| is_blank(at(i)) || at(i) == 0);

    let buf_end = SHEBANG_BUF - 1;

    // `strnchr(buf, sizeof(buf), '\n')` — which stops at a NUL, so a NUL
    // before any newline means "no newline".
    let newline = window
        .iter()
        .take_while(|&&c| c != 0)
        .position(|&c| c == b'\n');

    let mut line_end = match newline {
        Some(i) => i,
        None => {
            // No newline in the window. The interpreter's name must still be
            // complete: find where it starts, and require a blank or NUL after
            // it somewhere in the window, or the path is truncated.
            let start = next_non_blank(2, buf_end).ok_or(errno::ENOEXEC)?;
            if next_terminator(start, buf_end).is_none() {
                return Err(errno::ENOEXEC);
            }
            buf_end
        }
    };

    // Trim trailing blanks. `#!` occupies 0..2 and is not blank, so this stops
    // there at the latest; the bound says so rather than relying on it.
    while line_end > 2 && is_blank(at(line_end - 1)) {
        line_end -= 1;
    }

    let name_start = next_non_blank(2, line_end).ok_or(errno::ENOEXEC)?;
    if name_start == line_end {
        return Err(errno::ENOEXEC);
    }

    // The name ends at the first blank or NUL. If that is a blank, whatever
    // non-blank follows (up to the line's end) is the one argument.
    let sep = next_terminator(name_start, line_end);
    let arg_start = match sep {
        Some(s) if at(s) != 0 => next_non_blank(s, line_end),
        _ => None,
    };
    let name_end = sep.unwrap_or(line_end);

    // The argument is a C string in the kernel's buffer: it runs to the line's
    // end, or to an embedded NUL if one comes first.
    let arg = arg_start.map(|a| {
        let end = (a..line_end).find(|&i| at(i) == 0).unwrap_or(line_end);
        a..end
    });

    Ok(Shebang {
        window,
        interp: name_start..name_end,
        arg,
    })
}

/// Replace `argv[0]` of the packed argument list in `buf[..len]` with
/// `prefix`, in place, returning the new length.
///
/// The packed form is what `SYS_PROCESS_EXEC` and `SYS_PROCESS_SPAWN_EX`
/// consume — each argument followed by its NUL, back to back — so rewriting it
/// directly avoids building a pointer array for every level of interpreter.
///
/// An empty list (`len == 0`, a caller that passed no `argv[0]`) has nothing to
/// drop, and gets `prefix` alone — which is what Linux does for an `argc == 0`
/// `execve` of a script.
///
/// `None` when the result does not fit in `buf`: `E2BIG`, exactly as an
/// argument list that was too long to begin with.
///
/// The strings of `prefix` must not contain NUL; each gets one appended. They
/// cannot alias `buf` — the borrow checker guarantees it — so a caller whose
/// next prefix is a path currently inside `buf` must copy it out first.
pub(crate) fn splice_argv(buf: &mut [u8], len: usize, prefix: &[&[u8]]) -> Option<usize> {
    let used = buf.get(..len)?;
    // The first string and its NUL. A list without any NUL cannot come out of
    // the packer; treat it as one unterminated argv[0] and drop all of it.
    let arg0_len = used.iter().position(|&b| b == 0).map_or(len, |p| p + 1);
    let tail_len = len - arg0_len;

    let mut prefix_len = 0usize;
    for s in prefix {
        prefix_len = prefix_len.checked_add(s.len())?.checked_add(1)?;
    }
    let new_len = prefix_len.checked_add(tail_len)?;
    if new_len > buf.len() {
        return None;
    }

    buf.copy_within(arg0_len..len, prefix_len);
    let mut pos = 0usize;
    for s in prefix {
        let end = pos + s.len();
        buf.get_mut(pos..end)?.copy_from_slice(s);
        *buf.get_mut(end)? = 0;
        pos = end + 1;
    }
    Some(new_len)
}

/// How many arguments a packed list holds — the number of NULs in it.
pub(crate) fn packed_count(packed: &[u8]) -> usize {
    packed.iter().filter(|&&b| b == 0).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::vec::Vec;

    fn ok(head: &[u8]) -> (Vec<u8>, Option<Vec<u8>>) {
        let s = parse(head)
            .expect("a #! line")
            .unwrap_or_else(|e| panic!("errno {e} for {head:?}"));
        (s.interp().to_vec(), s.arg().map(<[u8]>::to_vec))
    }

    fn err(head: &[u8]) -> i32 {
        match parse(head) {
            Some(Err(e)) => e,
            other => panic!("expected an error for {head:?}, got {other:?}"),
        }
    }

    #[test]
    fn not_a_script_is_none() {
        assert!(parse(b"\x7fELF\x02\x01\x01").is_none());
        assert!(parse(b"").is_none());
        assert!(parse(b"#").is_none());
        assert!(parse(b" #!/bin/sh\n").is_none(), "#! must be at byte 0");
    }

    #[test]
    fn plain_interpreter() {
        assert_eq!(ok(b"#!/bin/sh\necho hi\n"), (b"/bin/sh".to_vec(), None));
    }

    #[test]
    fn blanks_around_the_name_are_skipped() {
        assert_eq!(ok(b"#! \t /bin/sh \t \nx"), (b"/bin/sh".to_vec(), None));
    }

    #[test]
    fn one_argument_keeps_its_inner_blanks() {
        // The classic surprise, reproduced: one word, not two.
        assert_eq!(
            ok(b"#!/usr/bin/env python3 -u\n"),
            (b"/usr/bin/env".to_vec(), Some(b"python3 -u".to_vec()))
        );
        assert_eq!(
            ok(b"#!/bin/sh   -e   \n"),
            (b"/bin/sh".to_vec(), Some(b"-e".to_vec()))
        );
    }

    #[test]
    fn a_file_with_no_newline_still_names_its_interpreter() {
        // Shorter than the window, so NUL-padded: the NUL ends the name.
        assert_eq!(ok(b"#!/bin/sh"), (b"/bin/sh".to_vec(), None));
        assert_eq!(
            ok(b"#!/bin/sh -x"),
            (b"/bin/sh".to_vec(), Some(b"-x".to_vec()))
        );
    }

    #[test]
    fn crlf_is_not_forgiven() {
        // Linux passes "/bin/sh\r", which then fails to exist. So do we.
        assert_eq!(ok(b"#!/bin/sh\r\n"), (b"/bin/sh\r".to_vec(), None));
    }

    #[test]
    fn no_interpreter_is_enoexec() {
        assert_eq!(err(b"#!\n"), errno::ENOEXEC);
        assert_eq!(err(b"#!   \t\n"), errno::ENOEXEC);
        // All blanks to the end of the window, no newline at all.
        let mut all_blank = [b' '; SHEBANG_BUF + 10];
        all_blank[0] = b'#';
        all_blank[1] = b'!';
        assert_eq!(err(&all_blank), errno::ENOEXEC);
    }

    #[test]
    fn a_truncated_interpreter_path_is_enoexec() {
        // No newline and no terminator after the name inside the window: the
        // path runs off the end, so it cannot be trusted.
        let mut long = std::vec![b'a'; SHEBANG_BUF + 50];
        long[0] = b'#';
        long[1] = b'!';
        long[2] = b'/';
        assert_eq!(err(&long), errno::ENOEXEC);
    }

    #[test]
    fn a_truncated_argument_is_allowed() {
        // The name fits and ends in a blank; the argument runs off the window.
        let mut head = std::vec![b'x'; SHEBANG_BUF + 50];
        head[..10].copy_from_slice(b"#!/bin/sh ");
        let (interp, arg) = ok(&head);
        assert_eq!(interp, b"/bin/sh");
        let arg = arg.expect("an argument");
        // Linux trims the window's last byte off and takes the rest.
        assert_eq!(arg.len(), SHEBANG_BUF - 1 - 10);
        assert!(arg.iter().all(|&b| b == b'x'));
    }

    #[test]
    fn only_the_first_line_matters() {
        assert_eq!(
            ok(b"#!/bin/awk -f\n#!/bin/sh\nBEGIN{}"),
            (b"/bin/awk".to_vec(), Some(b"-f".to_vec()))
        );
    }

    #[test]
    fn an_embedded_nul_ends_the_argument() {
        assert_eq!(
            ok(b"#!/bin/x ab\0cd\n"),
            (b"/bin/x".to_vec(), Some(b"ab".to_vec()))
        );
    }

    fn pack(args: &[&[u8]]) -> Vec<u8> {
        let mut v = Vec::new();
        for a in args {
            v.extend_from_slice(a);
            v.push(0);
        }
        v
    }

    #[test]
    fn splice_replaces_argv0() {
        let packed = pack(&[b"./run.sh", b"one", b"two"]);
        let mut buf = [0u8; 128];
        buf[..packed.len()].copy_from_slice(&packed);
        let n = splice_argv(&mut buf, packed.len(), &[b"/bin/sh", b"./run.sh"]).unwrap();
        assert_eq!(
            &buf[..n],
            pack(&[b"/bin/sh", b"./run.sh", b"one", b"two"]).as_slice()
        );
        assert_eq!(packed_count(&buf[..n]), 4);
    }

    #[test]
    fn splice_with_an_argument_and_a_shorter_prefix() {
        // The prefix is shorter than the argv[0] it replaces, so the tail moves
        // left: the in-place copy must handle both directions.
        let packed = pack(&[b"a-very-long-argv0-indeed", b"x"]);
        let mut buf = [0u8; 64];
        buf[..packed.len()].copy_from_slice(&packed);
        let n = splice_argv(&mut buf, packed.len(), &[b"/i", b"-f", b"s"]).unwrap();
        assert_eq!(&buf[..n], pack(&[b"/i", b"-f", b"s", b"x"]).as_slice());
    }

    #[test]
    fn splice_of_an_empty_list_is_the_prefix() {
        let mut buf = [0u8; 32];
        let n = splice_argv(&mut buf, 0, &[b"/bin/sh", b"s"]).unwrap();
        assert_eq!(&buf[..n], pack(&[b"/bin/sh", b"s"]).as_slice());
    }

    #[test]
    fn splice_that_does_not_fit_is_none_and_leaves_the_list() {
        let packed = pack(&[b"s", b"arg"]);
        let mut buf = [0u8; 8];
        buf[..packed.len()].copy_from_slice(&packed);
        assert!(splice_argv(&mut buf, packed.len(), &[b"/bin/interpreter"]).is_none());
        assert_eq!(
            &buf[..packed.len()],
            packed.as_slice(),
            "untouched on refusal"
        );
    }

    #[test]
    fn splice_at_exactly_full() {
        let packed = pack(&[b"s", b"ab"]); // 2 + 3 = 5 bytes
        let mut buf = [0u8; 9]; // "/x\0s\0ab\0" = 3 + 2 + 3 = 8 ... plus one spare
        buf[..packed.len()].copy_from_slice(&packed);
        let n = splice_argv(&mut buf, packed.len(), &[b"/xy", b"s"]).unwrap();
        assert_eq!(n, 9);
        assert_eq!(&buf[..n], pack(&[b"/xy", b"s", b"ab"]).as_slice());
    }
}
