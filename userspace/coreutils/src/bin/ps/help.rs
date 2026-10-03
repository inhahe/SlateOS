//! `help.c`: `--help` and its sections, and the short usage an error ends
//! with.

use crate::formats;
use crate::{Exit, Ps};

/// The sections, and the words and letters that ask for them.
const SECTIONS: [(&str, &str); 6] = [
    ("simple", "s"),
    ("list", "l"),
    ("output", "o"),
    ("threads", "t"),
    ("misc", "m"),
    ("all", "a"),
];

const SIMPLE: &str = "\nBasic options:\n \
-A, -e               all processes\n \
-a                   all with tty, except session leaders\n  \
a                   all with tty, including other users\n \
-d                   all except session leaders\n \
-N, --deselect       negate selection\n  \
r                   only running processes\n  \
T                   all processes on this terminal\n  \
x                   processes without controlling ttys\n";

const LIST: &str = "\nSelection by list:\n \
-C <command>         command name\n \
-G, --Group <GID>    real group id or name\n \
-g, --group <group>  session or effective group name\n \
-p, p, --pid <PID>   process id\n        \
--ppid <PID>  parent process id\n \
-q, q, --quick-pid <PID>\n                      \
process id (quick mode)\n \
-s, --sid <session>  session id\n \
-t, t, --tty <tty>   terminal\n \
-u, U, --user <UID>  effective user id or name\n \
-U, --User <UID>     real user id or name\n\n  \
The selection options take as their argument either:\n    \
a comma-separated list e.g. '-u root,nobody' or\n    \
a blank-separated list e.g. '-p 123 4567'\n";

const OUTPUT: &str = "\nOutput formats:\n \
-D <format>          date format for lstart\n \
-F                   extra full\n \
-f                   full-format, including command lines\n  \
f, --forest         ascii art process tree\n \
-H                   show process hierarchy\n \
-j                   jobs format\n  \
j                   BSD job control format\n \
-l                   long format\n  \
l                   BSD long format\n \
-M, Z                add security data (for SELinux)\n \
-O <format>          preloaded with default columns\n  \
O <format>          as -O, with BSD personality\n \
-o, o, --format <format>\n                      \
user-defined format\n  \
-P                  add psr column\n  \
s                   signal format\n  \
u                   user-oriented format\n  \
v                   virtual memory format\n  \
X                   register format\n \
-y                   do not show flags, show rss vs. addr (used with -l)\n     \
--context        display security context (for SELinux)\n     \
--headers        repeat header lines, one per page\n     \
--no-headers     do not print header at all\n     \
--cols, --columns, --width <num>\n                      \
set screen width\n     \
--rows, --lines <num>\n                      \
set screen height\n     \
--signames       display signal masks using signal names\n";

const THREADS: &str = "\nShow threads:\n  \
H                   as if they were processes\n \
-L                   possibly with LWP and NLWP columns\n \
-m, m                after processes\n \
-T                   possibly with SPID column\n";

const MISC: &str = "\nMiscellaneous options:\n \
-c                   show scheduling class with -l option\n  \
c                   show true command name\n  \
e                   show the environment after command\n  \
k,    --sort        specify sort order as: [+|-]key[,[+|-]key[,...]]\n  \
L                   show format specifiers\n  \
n                   display numeric uid and wchan\n  \
S,    --cumulative  include some dead child process data\n \
-y                   do not show flags, show rss (only with -l)\n \
-V, V, --version     display version information and exit\n \
-w, w                unlimited output width\n";

impl Ps {
    /// `do_help`: the usage, the section asked for (or the list of sections),
    /// on standard output for `--help` and standard error after an error;
    /// then exit `rc`.
    pub fn do_help(&mut self, opt: Option<&[u8]>, rc: i32) -> Exit {
        let section = opt.and_then(|o| {
            SECTIONS
                .iter()
                .position(|(word, abbrev)| o == word.as_bytes() || o == abbrev.as_bytes())
        });
        let mut text = b"\nUsage:\n ".to_vec();
        text.extend_from_slice(&self.myname);
        text.extend_from_slice(b" [options]\n");
        let all = section == Some(5);
        let words: Vec<&str> = SECTIONS.iter().map(|(w, _)| *w).collect();
        if section == Some(0) || all {
            text.extend_from_slice(SIMPLE.as_bytes());
        }
        if section == Some(1) || all {
            text.extend_from_slice(LIST.as_bytes());
        }
        if section == Some(2) || all {
            text.extend_from_slice(OUTPUT.as_bytes());
        }
        if section == Some(3) || all {
            text.extend_from_slice(THREADS.as_bytes());
        }
        if section == Some(4) || all {
            text.extend_from_slice(MISC.as_bytes());
            text.extend_from_slice(
                format!(
                    "\n        --help <{}>\n                      display help and exit\n",
                    words.join("|")
                )
                .as_bytes(),
            );
        }
        if section.is_none() {
            let letters: Vec<&str> = SECTIONS.iter().map(|(_, a)| *a).collect();
            text.extend_from_slice(b"\n Try '");
            text.extend_from_slice(&self.myname);
            text.extend_from_slice(format!(" --help <{}>'\n  or '", words.join("|")).as_bytes());
            text.extend_from_slice(&self.myname);
            text.extend_from_slice(
                format!(
                    " --help <{}>'\n for additional help text.\n",
                    letters.join("|")
                )
                .as_bytes(),
            );
        }
        text.extend_from_slice(b"\nFor more details see ps(1).\n");
        if rc == 0 {
            self.out.extend_from_slice(&text);
        } else {
            self.eprint(&text);
        }
        Exit::Status(rc)
    }

    /// `print_format_specifiers`: BSD `L`, every specifier that prints
    /// something, `%-12.12s %-8.8s`.
    pub fn print_format_specifiers(&mut self) {
        for f in formats::FORMATS.iter().take_while(|f| f.spec != b"~") {
            if f.pr == formats::Pr::Nop {
                continue;
            }
            let spec = f.spec.get(..f.spec.len().min(12)).unwrap_or_default();
            let head = f.head.get(..f.head.len().min(8)).unwrap_or_default();
            self.out.extend_from_slice(spec);
            self.out.extend(std::iter::repeat_n(
                b' ',
                12usize.saturating_sub(spec.len()).saturating_add(1),
            ));
            self.out.extend_from_slice(head);
            self.out
                .extend(std::iter::repeat_n(b' ', 8usize.saturating_sub(head.len())));
            self.out.push(b'\n');
        }
    }
}
