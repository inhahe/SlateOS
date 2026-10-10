"""glibc 2.39's argp, <argp.h>, as the oracle for posix/src/argp.rs:

    python posix/tools/oracle/argp_harness.py
        # writes posix/src/argp_oracle.txt and services/ctest-argp/main.c

The fixture is the same program built for SlateOS against this library
(SLATEOS_FIXTURE defined): every scenario but the deviations, its lines
collected and compared with glibc's, embedded -- exit 42 if each is the
same. It tests what only C can: the variadic argp_error and argp_failure,
and C parsers called through C structures.

A set of argp parsers, described below as data, and scenarios that run one
of them: argp_parse over an argument vector -- in some, under ARGP_HELP_FMT
or COLUMNS, with a version string or hook, a bug address, an exit status --
or argp_help called directly. Each scenario runs in a child of its own,
with standard output, standard error and a log of every call argp makes to
the parsers and help filters each going to a file of its own; how it ended
is the child's return or its exit.

The parsers are generic: each logs its call -- the key, the argument, the
parser state's next, arg_num, input and flags -- and does what its argp's
table says for that key (`ok`, `unknown`, `einval`, `error MSG`,
`failure STATUS ERRNUM MSG`, `usage`, `help FLAGS`, `rest`, `args`,
`argmax N`, `argmin N`; an unlisted key is ARGP_ERR_UNKNOWN). A parent's
parser, at ARGP_KEY_INIT, gives each child the input `<parent>.<i>`. Help
filters likewise log and do what theirs says (`keep`, `drop`, `replace
TEXT`, `append TEXT`; unlisted is `keep`).

posix/src/argp.rs's tests read the same descriptions out of the oracle and
build the same structures, so the two cannot drift. The file's lines, tab
separated, text escaped as C would write it (\\n, \\t, \\v, \\\\, \\xHH):

    A <id> <args_doc or -> <doc or ->          an argp
    o <id> <name or -> <key> <arg or -> <flags> <doc or -> <group>
    c <id> <child id> <flags> <header or -> <group>
    r <id> <key> <action>                      the parser's reaction
    f <id> <key> <action>                      the help filter's (and it has one)
    S <name> <argp> <flags> <how>              a scenario; how is `parse` or `help FLAGS NAME`
    a <arg>...                                 its argument vector
    e <variable> <value or ->                  its environment, set or unset
    g <global> <value>                         argp_program_version, _bug_address,
                                               _version_hook (1), argp_err_exit_status
    C <line>                                   a parser call, or F <line> a filter call
    O <stdout, escaped>
    E <stderr, escaped>
    X return <error_t> <arg_index> | X exit <status> | X signal <n> | X help

Keys are written as `'c'` for a printable character, the special keys by
their names (ARG, ARGS, END, NO_ARGS, INIT, SUCCESS, ERROR, FINI, and the
help filter's PRE_DOC, POST_DOC, HEADER, EXTRA, DUP_ARGS_NOTE, ARGS_DOC),
and any other as a number.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "argp_oracle.txt"

ALIAS, HIDDEN, DOC, NO_USAGE, OPTIONAL = "ALIAS", "HIDDEN", "DOC", "NO_USAGE", "ARG_OPTIONAL"

# --- the parsers -------------------------------------------------------------
# An option: (name, key, arg, flags, doc, group). A key is a one-character
# string or an int.

ARGPS = {}


def argp(ident, options=(), args_doc=None, doc=None, children=(), react=None, filt=None):
    ARGPS[ident] = dict(options=list(options), args_doc=args_doc, doc=doc,
                        children=list(children), react=dict(react or {}),
                        filt=None if filt is None else dict(filt))


argp("basic", [
    ("verbose", "v", None, [], "Produce verbose output", 0),
    ("quiet", "q", None, [], "Don't produce any output", 0),
    ("silent", "s", None, [ALIAS], None, 0),
    ("output", "o", "FILE", [], "Output to FILE instead of standard output", 0),
    ("level", "l", "N", [OPTIONAL], "Set the level, 1 if N is not given", 0),
    ("format", 1000, "FMT", [], "A long option with no short one, and an argument", 0),
    ("hidden", "H", None, [HIDDEN], "Never shown", 0),
    (None, 0, None, [], "Advanced options:", 1),
    ("debug", "d", None, [], "Print what is being done, described here at enough length that "
     "the description has to be filled onto the lines after this one", 1),
    ("max-depth", "m", "DEPTH", [], "Descend at most DEPTH levels", 1),
    (None, "x", None, [], "A short option with no long one", 1),
    ("exclude", "X", "PATTERN", [NO_USAGE], "Leave out what matches PATTERN", 1),
    ("verbose-errors", 1001, None, [], "A long option a prefix of which is another's", 1),
], args_doc="ARG1 ARG2",
    doc="A program with options and arguments.\vThis part comes after the options; "
        "it is filled to the margin, but a line break can be forced:\n<-- here.",
    react={"v": "ok", "q": "ok", "s": "ok", "o": "ok", "l": "ok", 1000: "ok", "H": "ok",
           "d": "ok", "m": "ok", "x": "ok", "X": "ok", 1001: "ok",
           "ARG": "argmax 2", "END": "argmin 2", "INIT": "ok", "SUCCESS": "ok",
           "FINI": "ok", "NO_ARGS": "ok"})

argp("plain", [], react={"ARG": "ok"})

argp("docs", [
    ("all", "a", None, [], "Everything", 0),
    ("FILE", 0, None, [DOC], "A file to read; documentation of an argument, not an option", 0),
    ("-", 0, None, [DOC, NO_USAGE], "Standard input", 0),
    ("zero", "z", None, [], "In group 0", 0),
    ("five", "5", None, [], "In group 5", 5),
    ("minus", "M", None, [], "In group -1, which comes last", -1),
    ("minus-two", "N", None, [], "In group -2, before -1", -2),
    (None, 0, None, [], "Group 3:", 3),
    ("three", "3", "THING", [], "In group 3", 3),
    ("also-three", 0, None, [ALIAS], None, 3),
    ("long-only-alias", 1002, None, [], "First of two names", 3),
    ("second-name", 0, None, [ALIAS], None, 3),
    ("secret-alias", 0, None, [ALIAS, HIDDEN], None, 3),
], args_doc="[FILE...]", doc="Options in groups, and documentation entries.",
    react={"a": "ok", "z": "ok", "5": "ok", "M": "ok", "N": "ok", "3": "ok", 1002: "ok",
           "ARG": "ok"})

# The same, but for OPTION_NO_USAGE on the documentation entry: glibc's
# usage line for this is what this library's is for "docs" (glibc lists
# the entry there as `[--FILE]`; design-decisions §1163).
argp("docs_nu", [(n, k, a, (fl + [NO_USAGE]) if n == "FILE" else fl, d, g)
                 for n, k, a, fl, d, g in ARGPS["docs"]["options"]],
     args_doc=ARGPS["docs"]["args_doc"], doc=ARGPS["docs"]["doc"],
     react=ARGPS["docs"]["react"])

argp("child1", [
    ("child-opt", "c", "VALUE", [], "An option of the first child", 0),
    ("both", "b", None, [], "A flag of the first child", 0),
], react={"c": "ok", "b": "ok", "INIT": "ok", "END": "ok", "SUCCESS": "ok", "FINI": "ok",
          "ARG": "unknown", "NO_ARGS": "ok"})

argp("child2", [
    ("deep", "D", None, [], "An option of the second child", 0),
    ("deeper", 1003, "LEVEL", [OPTIONAL], "Its long option, with an optional argument", 0),
], doc="The second child's own documentation.",
    react={"D": "ok", 1003: "ok", "INIT": "ok", "END": "ok"})

argp("grandchild", [
    ("grand", "g", None, [], "An option two levels down", 0),
], react={"g": "ok", "INIT": "ok"})

argp("child3", [
    ("third", "t", None, [], "An option of a child with a child", 0),
], children=[("grandchild", 0, "Grandchild options:", 0)],
    react={"t": "ok", "INIT": "ok"})

argp("parent", [
    ("top", "T", None, [], "The parent's own option", 0),
], args_doc="ITEM...", doc="A parent with three children.\vAnd after.",
    children=[("child1", 0, "First child's options:", 0),
              ("child2", 0, None, 0),
              ("child3", 0, "Third child's options:", 2)],
    react={"T": "ok", "INIT": "ok", "ARG": "ok", "END": "ok", "SUCCESS": "ok",
           "FINI": "ok", "ERROR": "ok", "NO_ARGS": "ok"})

argp("filtered", [
    ("alpha", "a", None, [], "Alpha's own text", 0),
    ("beta", "b", "B", [OPTIONAL], "Beta's own text", 0),
    (None, 0, None, [], "A header:", 1),
    ("gamma", "g", None, [], "Gamma's own text", 1),
], args_doc="IN OUT", doc="Before the options.\vAfter the options.",
    react={"a": "ok", "b": "ok", "g": "ok", "ARG": "ok"},
    filt={"PRE_DOC": "replace The pre-doc, replaced.", "POST_DOC": "drop",
          "a": "append  (and more)", "g": "drop", "HEADER": "replace A new header:",
          "EXTRA": "replace Extra text from the filter.", "DUP_ARGS_NOTE": "keep",
          "ARGS_DOC": "replace NEW-ARGS"})

argp("errs", [
    ("error", "e", None, [], "argp_error", 0),
    ("failure", "F", None, [], "argp_failure, exit 3, ENOENT", 0),
    ("soft", "f", None, [], "argp_failure, no exit, EINVAL", 0),
    ("usage", "u", None, [], "argp_usage", 0),
    ("help-me", "h", None, [], "argp_state_help, ARGP_HELP_STD_HELP", 0),
    ("short-help", "y", None, [], "argp_state_help, usage and see", 0),
    ("invalid", "i", None, [], "returns EINVAL", 0),
    ("unknown", "k", None, [], "returns ARGP_ERR_UNKNOWN for itself", 0),
    ("value", "w", "W", [], "an option with an argument", 0),
], args_doc="ARG...",
    react={"e": "error something is wrong", "F": "failure 3 2 cannot open",
           "f": "failure 0 22 soft failure", "u": "usage", "h": "help 0x27a",
           "y": "help 0x07", "i": "einval", "k": "unknown", "w": "ok",
           "ARG": "unknown", "ARGS": "args", "NO_ARGS": "ok", "ERROR": "ok",
           "END": "ok", "SUCCESS": "ok", "FINI": "ok", "INIT": "ok"})

argp("rest", [
    ("flag", "f", None, [], "A flag", 0),
], args_doc="COMMAND [ARG...]",
    react={"f": "ok", "ARG": "rest", "END": "ok", "NO_ARGS": "ok"})

argp("multi", [
    ("target", "t", "DIR", [], "Copy into DIR", 0),
], args_doc="SRC DEST\n-t DIR SRC...\n--list",
    doc="Several forms of usage.",
    react={"t": "ok", "ARG": "ok"})

argp("wide", [
    ("a-very-long-option-name-indeed", "a", "AN-ARGUMENT-NAME", [],
     "A long name and argument that run past the column the documentation starts in", 0),
    ("b", "b", None, [], "One-letter long name", 0),
    ("ccc", "c", "C", [OPTIONAL],
     "Supercalifragilisticexpialidocious-and-other-words-longer-than-a-line-can-hold-at-all-"
     "even-at-the-widest-margin-this-harness-tries", 0),
    ("ddd", "d", None, [], "Text with\na line break in it, and\ttabs\tin it.", 0),
    ("eee", "e", None, [], "", 0),
], args_doc="WORDS",
    doc="Documentation long enough to need several lines at any margin, so that the way "
        "it is filled can be seen: words are moved whole onto the next line, and a word "
        "too long for a line is left to run past the margin.",
    react={"a": "ok", "b": "ok", "c": "ok", "d": "ok", "e": "ok", "ARG": "ok"})

argp("noargs", [
    ("one", "1", None, [], "One", 0),
], react={"1": "ok", "ARG": "ok"})

# Clusters: children with headers in one group, in another, and without one.
argp("ca", [("ca-opt", "A", None, [], "From the first child", 0)], react={"A": "ok"})
argp("cb", [("cb-opt", "B", None, [], "From the second child", 0),
            ("zz", "Z", None, [], "The parent's short option letter, shadowed", 0)],
     react={"B": "ok", "Z": "ok"})
argp("cc", [("cc-opt", "C", None, [], "From the third child, a cluster in group 1", 0)],
     react={"C": "ok"})
argp("cd", [("cd-opt", 1010, None, [], "From the fourth child: group -1, no header", 0)],
     react={1010: "ok"})
argp("ce", [("ce-opt", "E", None, [], "From the fifth child: no header, no group", 0)],
     react={"E": "ok", "ARG": "ok"})
argp("clusters", [
    ("zz", "Z", None, [], "Group 0, no header", 0),
    ("one", "o", None, [], "Group 1, no header", 1),
    ("two", "t", None, [], "Group 2, no header", 2),
], args_doc="ARG", children=[("ca", 0, "First:", 0), ("cb", 0, "Second:", 0),
                             ("cc", 0, "Third:", 1), ("cd", 0, None, -1), ("ce", 0, None, 0)],
    react={"Z": "ok", "o": "ok", "t": "ok", "ARG": "take 1"})

# Groups with no header anywhere.
argp("nohead", [
    ("a-one", "a", None, [], "Group 0", 0),
    ("b-one", "b", None, [], "Group 1", 1),
    ("c-one", "c", None, [], "Group 2", 2),
    ("d-one", "d", None, [], "Group -2", -2),
    (None, "j", "J", [OPTIONAL], "Short only, with an optional argument", 0),
    (None, "k", "K", [], "Short only, with an argument", 0),
    ("nodoc", "n", None, [], None, 0),
    ("aliased", "p", None, [], "The first of an option and its alias with a doc", 0),
    ("alias-doc", "P", None, [ALIAS], "An alias's own doc, which is not shown", 0),
], react={"a": "ok", "b": "ok", "c": "ok", "d": "ok", "j": "ok", "k": "ok", "n": "ok",
          "p": "ok", "P": "ok", "ARG": "ok"})

# Paragraphs past the formatting buffer, and an argument line past a line.
LONG = ("This paragraph is long enough that it cannot be held whole in the buffer the "
        "help is formatted in, which is two hundred bytes when it starts, so that the "
        "buffer is written out part way through a line, and the next line is broken where "
        "the text written later allows. It goes on for a while yet, with ordinary words "
        "of ordinary lengths, so that each line is filled to the margin before it breaks.")
argp("longtext", [
    ("long-doc", "L", "SOMETHING", [], LONG, 0),
    ("short-doc", "s", None, [], "Short", 0),
], args_doc="A-VERY-LONG-ARGUMENT-DESCRIPTION-THAT-WILL-NOT-FIT-ON-THE-REST-OF-THE-LINE "
            "AND-ANOTHER [MORE...]",
    doc=LONG + "\v" + LONG + "\n\nAnd a second paragraph after a blank line.",
    react={"L": "ok", "s": "ok", "ARG": "ok"})

# A filter on the root argp, which ARGP_NO_HELP makes it.
argp("rootfilt", [
    ("alpha", "a", "A", [], "Alpha", 0),
    ("help-me", "h", None, [], "argp_state_help, ARGP_HELP_STD_HELP", 0),
], args_doc="X", doc="Pre.\vPost.",
    react={"a": "ok", "h": "help 0x27a", "ARG": "ok"},
    filt={"DUP_ARGS_NOTE": "replace The note, filtered.", "PRE_DOC": "keep"})

# Arguments shared between a parent and a child.
argp("argkid", [], react={"ARG": "ok", "END": "ok", "NO_ARGS": "ok"})
argp("argparent", [], args_doc="FIRST REST...",
     children=[("argkid", 0, None, 0)],
     react={"ARG": "take 1", "END": "ok", "NO_ARGS": "ok", "INIT": "ok"})

# --- the scenarios -----------------------------------------------------------

SCENARIOS = []


def sc(name, ident, argv, flags=(), env=None, globs=None, how="parse"):
    SCENARIOS.append(dict(name=name, argp=ident, argv=list(argv), flags=list(flags),
                          env=dict(env or {}), globs=dict(globs or {}), how=how))


P = "/usr/bin/prog"
V = {"version": "prog 1.0"}

# Parsing.
sc("basic-options", "basic", [P, "-v", "--quiet", "-o", "out", "a", "b"])
sc("basic-attached", "basic", [P, "-vqoout", "--output=x", "--output", "y", "a", "b"])
sc("basic-optional", "basic", [P, "-l", "-l3", "--level", "--level=7", "a", "b"])
sc("basic-abbrev", "basic", [P, "--verb", "--out=o", "--max=2", "a", "b"])
sc("basic-ambiguous", "basic", [P, "--verbose-", "--ver", "a", "b"])
sc("basic-ambiguous-only", "basic", [P, "--ver", "a", "b"])
sc("basic-exact-prefix", "basic", [P, "--verbose", "--verbose-errors", "a", "b"])
sc("basic-unknown-short", "basic", [P, "-Z", "a", "b"])
sc("basic-unknown-long", "basic", [P, "--bogus", "a", "b"])
sc("basic-missing-arg", "basic", [P, "a", "b", "-o"])
sc("basic-missing-long-arg", "basic", [P, "a", "b", "--output"])
sc("basic-arg-to-flag", "basic", [P, "--verbose=yes", "a", "b"])
sc("basic-dashdash", "basic", [P, "-v", "--", "-q", "b"])
sc("basic-permute", "basic", [P, "a", "-v", "b", "-q"])
sc("basic-in-order", "basic", [P, "a", "-v", "b", "-q"], flags=["ARGP_IN_ORDER"])
sc("basic-posixly", "basic", [P, "a", "-v", "b"], env={"POSIXLY_CORRECT": "1"})
sc("basic-too-many", "basic", [P, "a", "b", "c"])
sc("basic-too-few", "basic", [P, "a"])
sc("basic-dash", "basic", [P, "-", "b"])
sc("basic-long-only-flag", "basic", [P, "-verbose", "-output=o", "a", "b"],
   flags=["ARGP_LONG_ONLY"])
sc("basic-argv0", "basic", [P, "a"], flags=["ARGP_PARSE_ARGV0"])
sc("basic-no-errs", "basic", [P, "-Z", "--bogus", "a", "b"], flags=["ARGP_NO_ERRS"])
sc("basic-no-exit", "basic", [P, "-Z", "a", "b"], flags=["ARGP_NO_EXIT"])
sc("basic-no-exit-help", "basic", [P, "--help", "a", "b"], flags=["ARGP_NO_EXIT"])
sc("basic-silent", "basic", [P, "--help", "-Z", "a", "b"], flags=["ARGP_SILENT"])
sc("basic-no-help", "basic", [P, "--help", "a", "b"], flags=["ARGP_NO_HELP"])
sc("basic-no-args-flag", "basic", [P, "a", "b"], flags=["ARGP_NO_ARGS"])
sc("basic-hidden", "basic", [P, "-H", "--hidden", "a", "b"])
sc("basic-alias", "basic", [P, "-s", "--silent", "a", "b"])
sc("basic-long-only-key", "basic", [P, "--format", "f1", "--format=f2", "a", "b"])
sc("basic-argv0-name", "basic", ["/opt/tools/other-name", "-Z", "a", "b"])
sc("basic-no-argv", "basic", [])
sc("basic-err-status", "basic", [P, "-Z"], globs={"err_exit": "7"})

# The built-in options.
sc("help", "basic", [P, "--help"])
sc("help-short", "basic", [P, "-?"])
sc("help-abbrev", "basic", [P, "--he"])
sc("usage", "basic", [P, "--usage"])
sc("version", "basic", [P, "--version"], globs=V)
sc("version-short", "basic", [P, "-V"], globs=V)
sc("version-none", "basic", [P, "--version"])
sc("version-hook", "basic", [P, "-V"], globs={"hook": "1"})
sc("help-version-bug", "basic", [P, "--help"], globs={"version": "prog 1.0",
                                                      "bug": "<bugs@example.org>"})
sc("usage-version", "basic", [P, "--usage"], globs=V)
sc("program-name", "basic", [P, "--program-name=renamed", "-Z"])
sc("hang", "basic", [P, "--HANG=0", "a", "b"])
sc("help-plain", "plain", [P, "--help"])
sc("usage-plain", "plain", [P, "--usage"])
sc("help-plain-version", "plain", [P, "--help"], globs=V)
sc("help-noargs", "noargs", [P, "--help"])

# Help under ARGP_HELP_FMT and COLUMNS.
for fmt in ("dup-args", "no-dup-args", "short-opt-col=4", "long-opt-col=10", "doc-opt-col=4",
            "opt-doc-col=40", "opt-doc-col=0", "header-col=4", "usage-indent=4", "rmargin=40",
            "rmargin=200", "rmargin=50,opt-doc-col=20,dup-args", " rmargin = 60 , ,dup-args",
            "bogus", "rmargin", "rmargin=abc", "rmargin=60x", "dup-args=1", "no-dup-args=0",
            "long-opt-col=2,short-opt-col=8", "rmargin=20", "usage-indent=40,rmargin=50"):
    sc(f"help-fmt {fmt}", "basic", [P, "--help"], env={"ARGP_HELP_FMT": fmt})
    sc(f"usage-fmt {fmt}", "basic", [P, "--usage"], env={"ARGP_HELP_FMT": fmt})
sc("help-columns", "basic", [P, "--help"], env={"COLUMNS": "40"})

# Groups, documentation options, children, filters, alternatives, width.
sc("help-docs", "docs", [P, "--help"])
sc("usage-docs", "docs", [P, "--usage"])
sc("usage-docs-nu", "docs_nu", [P, "--usage"])
sc("parse-docs", "docs", [P, "--also-three=x", "--second-name", "--secret-alias", "f"])
sc("parse-docs-alias", "docs", [P, "--also-three", "q", "--second", "f"])
sc("help-parent", "parent", [P, "--help"])
sc("usage-parent", "parent", [P, "--usage"])
sc("parse-parent", "parent", [P, "-T", "-c", "v1", "--both", "-D", "--deeper", "--deeper=2",
                              "-tg", "item1", "item2"])
sc("parse-parent-noargs", "parent", [P, "-T"])
sc("parse-parent-error", "parent", [P, "-T", "--child-opt"])
sc("help-filtered", "filtered", [P, "--help"])
sc("usage-filtered", "filtered", [P, "--usage"])
sc("help-filtered-dup", "filtered", [P, "--help"], env={"ARGP_HELP_FMT": "dup-args"})
sc("help-multi", "multi", [P, "--help"])
sc("usage-multi", "multi", [P, "--usage"])
sc("help-wide", "wide", [P, "--help"])
sc("usage-wide", "wide", [P, "--usage"])
sc("help-wide-narrow", "wide", [P, "--help"], env={"ARGP_HELP_FMT": "rmargin=30"})
sc("help-wide-wide", "wide", [P, "--help"], env={"ARGP_HELP_FMT": "rmargin=150"})
sc("help-long-only", "basic", [P, "--help"], flags=["ARGP_LONG_ONLY"])
sc("usage-long-only", "basic", [P, "--usage"], flags=["ARGP_LONG_ONLY"])

# A parser's own calls and answers.
for opt in ("-e", "-F", "-f", "-u", "-h", "-y", "-i", "-k", "-w"):
    sc(f"errs {opt}", "errs", [P, opt, "a"])
sc("errs args", "errs", [P, "a", "b", "c"])
sc("errs no-args", "errs", [P, "-w", "x"])
sc("errs no-exit", "errs", [P, "-e", "-F", "-u", "a"], flags=["ARGP_NO_EXIT"])
sc("errs no-errs", "errs", [P, "-e", "-u", "a"], flags=["ARGP_NO_ERRS"])
sc("errs silent", "errs", [P, "-h", "-e", "a"], flags=["ARGP_SILENT"])
sc("rest", "rest", [P, "-f", "cmd", "-x", "--y", "z"])
sc("rest-dashdash", "rest", [P, "--", "-f", "cmd"])
sc("rest-in-order", "rest", [P, "cmd", "-f", "x"], flags=["ARGP_IN_ORDER"])
sc("rest-none", "rest", [P, "-f"])

# Clusters, headers, shadowing, long text, the root's filter, shared arguments.
sc("help-clusters", "clusters", [P, "--help"])
sc("usage-clusters", "clusters", [P, "--usage"])
sc("parse-clusters", "clusters", [P, "-Z", "--zz", "-A", "-B", "-C", "--cd-opt", "-E", "x", "y"])
sc("help-nohead", "nohead", [P, "--help"])
sc("usage-nohead", "nohead", [P, "--usage"])
sc("help-nohead-dup", "nohead", [P, "--help"], env={"ARGP_HELP_FMT": "dup-args,no-dup-args-note"})
sc("parse-nohead", "nohead", [P, "-j", "-jx", "-k", "y", "-ky", "-pP", "--alias-doc", "-n"])
sc("help-longtext", "longtext", [P, "--help"])
sc("usage-longtext", "longtext", [P, "--usage"])
sc("help-longtext-60", "longtext", [P, "--help"], env={"ARGP_HELP_FMT": "rmargin=60"})
sc("usage-longtext-60", "longtext", [P, "--usage"], env={"ARGP_HELP_FMT": "rmargin=60"})
sc("help-rootfilt", "rootfilt", [P, "-h"], flags=["ARGP_NO_HELP"])
sc("help-rootfilt-std", "rootfilt", [P, "--help"])
sc("argparent", "argparent", [P, "one", "two", "three"])
sc("argparent-one", "argparent", [P, "one"])
sc("argparent-none", "argparent", [P])
sc("argv0-no-errs", "basic", [P, "a"], flags=["ARGP_PARSE_ARGV0", "ARGP_NO_ERRS"])
sc("argv0-no-errs-name", "basic", ["/x/y/z", "-Z"], flags=["ARGP_PARSE_ARGV0", "ARGP_NO_ERRS"])
sc("version-both", "basic", [P, "--version"], globs={"version": "prog 2.0", "hook": "1"})
sc("usage-err-status", "basic", [P, "a", "b", "c"], globs={"err_exit": "9"})
sc("empty-args", "basic", [P, "-o", "", "--output=", "-o", "--", "a", "b"])
sc("program-name-path", "basic", [P, "--program-name=/a/b/newname", "--help"])

# argp_help itself.
for flags in ("0x01", "0x02", "0x04", "0x08", "0x10", "0x20", "0x30", "0x40", "0x81",
              "0x82", "0x7a", "0x7b", "0x105", "0x27a", "0x3ff"):
    sc(f"argp_help {flags}", "basic", [], how=f"help {flags} named")
sc("argp_help bug", "basic", [], globs={"bug": "<bugs@example.org>"}, how="help 0x7a named")
sc("argp_help docs", "docs", [], how="help 0x7b named")
sc("argp_help parent", "parent", [], how="help 0x7b named")
sc("argp_help filtered", "filtered", [], how="help 0x7b named")
sc("argp_help null-name", "basic", [], how="help 0x03 -")

# --- the program -------------------------------------------------------------

C_MAIN = r'''
#define _GNU_SOURCE
#include <argp.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

struct react { int key; const char *what; };

struct spec {
    const char *id;
    const struct react *acts;
    int nacts;
    const struct react *filts;
    int nfilts;
    int nchildren;
};

static const char *keyname(int key)
{
    static char buf[32];
    switch (key) {
    case ARGP_KEY_ARG: return "ARG";
    case ARGP_KEY_ARGS: return "ARGS";
    case ARGP_KEY_END: return "END";
    case ARGP_KEY_NO_ARGS: return "NO_ARGS";
    case ARGP_KEY_INIT: return "INIT";
    case ARGP_KEY_SUCCESS: return "SUCCESS";
    case ARGP_KEY_ERROR: return "ERROR";
    case ARGP_KEY_FINI: return "FINI";
    case ARGP_KEY_HELP_PRE_DOC: return "PRE_DOC";
    case ARGP_KEY_HELP_POST_DOC: return "POST_DOC";
    case ARGP_KEY_HELP_HEADER: return "HEADER";
    case ARGP_KEY_HELP_EXTRA: return "EXTRA";
    case ARGP_KEY_HELP_DUP_ARGS_NOTE: return "DUP_ARGS_NOTE";
    case ARGP_KEY_HELP_ARGS_DOC: return "ARGS_DOC";
    }
    if (key > 0x20 && key < 0x7f)
        snprintf(buf, sizeof buf, "'%c'", key);
    else
        snprintf(buf, sizeof buf, "%d", key);
    return buf;
}

/* Text as the oracle writes it. */
static void esc(int fd, const char *s)
{
    for (const unsigned char *p = (const unsigned char *) s; *p; p++) {
        if (*p == '\\') dprintf(fd, "\\\\");
        else if (*p == '\n') dprintf(fd, "\\n");
        else if (*p == '\t') dprintf(fd, "\\t");
        else if (*p == '\v') dprintf(fd, "\\v");
        else if (*p < 0x20 || *p >= 0x7f) dprintf(fd, "\\x%02x", *p);
        else dprintf(fd, "%c", *p);
    }
}

static const char *find(const struct react *t, int n, int key)
{
    for (int i = 0; i < n; i++)
        if (t[i].key == key)
            return t[i].what;
    return NULL;
}

static char tags[64][32];
static int ntags;

static error_t run(const struct spec *sp, int key, char *arg, struct argp_state *st)
{
    dprintf(3, "C %s\t%s\t", sp->id, keyname(key));
    if (arg) esc(3, arg); else dprintf(3, "-");
    dprintf(3, "\tnext=%d num=%u argc=%d in=%s flags=%#x quoted=%d\n", st->next, st->arg_num,
            st->argc, st->input ? (const char *) st->input : "(null)", st->flags, st->quoted);
    if (key == ARGP_KEY_INIT && sp->nchildren > 0)
        for (int i = 0; i < sp->nchildren && ntags < 64; i++) {
            snprintf(tags[ntags], sizeof tags[ntags], "%s.%d",
                     st->input ? (const char *) st->input : "(null)", i);
            st->child_inputs[i] = tags[ntags++];
        }
    const char *w = find(sp->acts, sp->nacts, key);
    if (!w || !strcmp(w, "unknown")) return ARGP_ERR_UNKNOWN;
    if (!strcmp(w, "ok")) return 0;
    if (!strcmp(w, "einval")) return EINVAL;
    if (!strncmp(w, "error ", 6)) { argp_error(st, "%s", w + 6); return 0; }
    if (!strncmp(w, "failure ", 8)) {
        int status = 0, errnum = 0, n = 0;
        sscanf(w + 8, "%d %d %n", &status, &errnum, &n);
        argp_failure(st, status, errnum, "%s", w + 8 + n);
        return 0;
    }
    if (!strcmp(w, "usage")) { argp_usage(st); return 0; }
    if (!strncmp(w, "help ", 5)) {
        argp_state_help(st, st->out_stream, (unsigned) strtoul(w + 5, NULL, 0));
        return 0;
    }
    if (!strcmp(w, "rest")) {
        dprintf(3, "C %s\trest", sp->id);
        for (int i = st->next; i < st->argc; i++) { dprintf(3, "\t"); esc(3, st->argv[i]); }
        dprintf(3, "\n");
        st->next = st->argc;
        return 0;
    }
    if (!strcmp(w, "args")) {
        dprintf(3, "C %s\targs", sp->id);
        for (int i = st->next; i < st->argc; i++) { dprintf(3, "\t"); esc(3, st->argv[i]); }
        dprintf(3, "\n");
        st->next = st->argc;
        return 0;
    }
    if (!strncmp(w, "argmax ", 7)) {
        if (st->arg_num >= (unsigned) atoi(w + 7)) argp_usage(st);
        return 0;
    }
    if (!strncmp(w, "argmin ", 7)) {
        if (st->arg_num < (unsigned) atoi(w + 7)) argp_usage(st);
        return 0;
    }
    if (!strncmp(w, "take ", 5))
        return st->arg_num < (unsigned) atoi(w + 5) ? 0 : ARGP_ERR_UNKNOWN;
    dprintf(3, "C bad action %s\n", w);
    return EINVAL;
}

static char *filt(const struct spec *sp, int key, const char *text, void *input)
{
    dprintf(3, "F %s\t%s\t", sp->id, keyname(key));
    if (text) esc(3, text); else dprintf(3, "-");
    dprintf(3, "\tin=%s\n", input ? (const char *) input : "(null)");
    const char *w = find(sp->filts, sp->nfilts, key);
    if (!w || !strcmp(w, "keep")) return (char *) text;
    if (!strcmp(w, "drop")) return NULL;
    if (!strncmp(w, "replace ", 8)) return strdup(w + 8);
    if (!strncmp(w, "append ", 7)) {
        size_t a = text ? strlen(text) : 0, b = strlen(w + 7);
        char *r = malloc(a + b + 1);
        if (text) memcpy(r, text, a);
        memcpy(r + a, w + 7, b + 1);
        return r;
    }
    return (char *) text;
}

static void hook(FILE *stream, struct argp_state *st)
{
    fprintf(stream, "version from the hook, %s\n", st ? "with a state" : "without one");
}

@TABLES@

/* What the parent prints: on standard output, under glibc; collected, to
 * be compared with glibc's lines, as the fixture. */
#ifdef SLATEOS_FIXTURE
static char text[1 << 21];
static size_t text_len;

static void out(const char *s, size_t n)
{
    if (n <= sizeof text - text_len) {
        memcpy(text + text_len, s, n);
        text_len += n;
    } else {
        text_len = sizeof text; /* too much: the comparison fails */
    }
}
#else
static void out(const char *s, size_t n)
{
    fwrite(s, 1, n, stdout);
}
#endif

static void outs(const char *s)
{
    out(s, strlen(s));
}

/* `esc`, into `out`. */
static void out_esc(const char *s)
{
    char b[8];
    for (const unsigned char *p = (const unsigned char *) s; *p; p++) {
        if (*p == '\\') outs("\\\\");
        else if (*p == '\n') outs("\\n");
        else if (*p == '\t') outs("\\t");
        else if (*p == '\v') outs("\\v");
        else if (*p < 0x20 || *p >= 0x7f) {
            snprintf(b, sizeof b, "\\x%02x", *p);
            outs(b);
        } else
            out((const char *) p, 1);
    }
}

static void dump(int fd, const char *tag)
{
    static char buf[65536];
    lseek(fd, 0, SEEK_SET);
    ssize_t n = read(fd, buf, sizeof buf - 1);
    buf[n > 0 ? n : 0] = 0;
    outs(tag);
    out_esc(buf);
    outs("\n");
}

#ifdef SLATEOS_FIXTURE
static int skipped(const char *name)
{
    for (unsigned i = 0; i < sizeof SKIP / sizeof *SKIP; i++)
        if (!strcmp(SKIP[i], name))
            return 1;
    return 0;
}
#endif

int main(void)
{
    for (unsigned s = 0; s < sizeof SCEN / sizeof *SCEN; s++) {
        const struct scen *sc = &SCEN[s];
#ifdef SLATEOS_FIXTURE
        if (skipped(sc->name))
            continue;
#endif
        /* Removed only once read: on SlateOS an open file unlinked is lost
         * to its descriptor. */
        char no[] = "/tmp/argp-oXXXXXX", ne[] = "/tmp/argp-eXXXXXX", nl[] = "/tmp/argp-lXXXXXX";
        int fo = mkstemp(no), fe = mkstemp(ne), fl = mkstemp(nl);
        if (fo < 0 || fe < 0 || fl < 0) {
            printf("argp: no temporary files in /tmp\n");
            return 3;
        }
        fflush(stdout);
        pid_t pid = fork();
        if (pid == 0) {
#ifndef SLATEOS_FIXTURE
            /* Some settings make glibc's help loop for ever: the scenario
             * ends there, by the alarm, and says so. */
            alarm(10);
#endif
            dup2(fo, 1); dup2(fe, 2); dup2(fl, 3);
            for (int i = 0; i < sc->nenv; i++)
                if (sc->env[i][1]) setenv(sc->env[i][0], sc->env[i][1], 1);
                else unsetenv(sc->env[i][0]);
            program_invocation_name = (char *) "/usr/bin/prog";
            program_invocation_short_name = (char *) "prog";
            if (sc->version) argp_program_version = sc->version;
            if (sc->hook) argp_program_version_hook = hook;
            if (sc->bug) argp_program_bug_address = sc->bug;
            if (sc->err_exit) argp_err_exit_status = sc->err_exit;
            if (sc->help_flags >= 0) {
                argp_help(sc->argp, stdout, (unsigned) sc->help_flags, (char *) sc->help_name);
                fflush(stdout);
                dprintf(3, "X\thelp\n");
                _exit(0);
            }
            int idx = -12345;
            char *argv[32];
            for (int i = 0; i < sc->argc; i++) argv[i] = (char *) sc->argv[i];
            argv[sc->argc] = NULL;
            error_t r = argp_parse(sc->argp, sc->argc, sc->argc ? argv : NULL, sc->flags, &idx,
                                   (void *) "root");
            fflush(stdout);
            fflush(stderr);
            dprintf(3, "X\treturn %d %d\n", r, idx);
            _exit(0);
        }
        int st = 0;
        waitpid(pid, &st, 0);
        outs("S\t");
        outs(sc->name);
        outs("\n");
        static char logbuf[65536];
        lseek(fl, 0, SEEK_SET);
        ssize_t n = read(fl, logbuf, sizeof logbuf - 1);
        logbuf[n > 0 ? n : 0] = 0;
        /* The end line, `X\t...`, only at the start of a line: a logged
         * text may hold an X and a tab. */
        char *ended = !strncmp(logbuf, "X\t", 2) ? logbuf : strstr(logbuf, "\nX\t");
        if (ended && ended != logbuf) ended++;
        if (ended) *ended = 0;
        outs(logbuf);
        dump(fo, "O\t");
        dump(fe, "E\t");
        char how[64];
        if (ended) {
            outs("X\t");
            outs(ended + 2);
        } else {
            if (WIFEXITED(st)) snprintf(how, sizeof how, "X\texit %d\n", WEXITSTATUS(st));
            else snprintf(how, sizeof how, "X\tsignal %d\n", WTERMSIG(st));
            outs(how);
        }
        close(fo); close(fe); close(fl);
        unlink(no); unlink(ne); unlink(nl);
    }
#ifdef SLATEOS_FIXTURE
    /* Each line against glibc's. */
    const char *at = text;
    if (text_len >= sizeof text) {
        printf("ctest-argp: more output than the fixture holds\n");
        return 1;
    }
    text[text_len] = 0;
    for (unsigned k = 0; k < sizeof EXPECTED / sizeof *EXPECTED; k++) {
        size_t len = strlen(EXPECTED[k]);
        if (strncmp(at, EXPECTED[k], len) != 0 || at[len] != '\n') {
            const char *nl2 = strchr(at, '\n');
            printf("ctest-argp: line %u is not glibc's:\n  want %.300s\n  got  %.*s\n", k + 1,
                   EXPECTED[k], nl2 ? (int) (nl2 - at > 300 ? 300 : nl2 - at) : 0, at);
            return 1;
        }
        at += len + 1;
    }
    if (*at) {
        printf("ctest-argp: more lines than glibc's, from: %.60s\n", at);
        return 1;
    }
    printf("ctest-argp: %u lines, each glibc's\n", (unsigned) (sizeof EXPECTED / sizeof *EXPECTED));
    return 42;
#else
    return 0;
#endif
}
'''

KEYS = {"ARG": "ARGP_KEY_ARG", "ARGS": "ARGP_KEY_ARGS", "END": "ARGP_KEY_END",
        "NO_ARGS": "ARGP_KEY_NO_ARGS", "INIT": "ARGP_KEY_INIT", "SUCCESS": "ARGP_KEY_SUCCESS",
        "ERROR": "ARGP_KEY_ERROR", "FINI": "ARGP_KEY_FINI",
        "PRE_DOC": "ARGP_KEY_HELP_PRE_DOC", "POST_DOC": "ARGP_KEY_HELP_POST_DOC",
        "HEADER": "ARGP_KEY_HELP_HEADER", "EXTRA": "ARGP_KEY_HELP_EXTRA",
        "DUP_ARGS_NOTE": "ARGP_KEY_HELP_DUP_ARGS_NOTE", "ARGS_DOC": "ARGP_KEY_HELP_ARGS_DOC"}


def c_str(s):
    if s is None:
        return "NULL"
    out = []
    for ch in s.encode("utf-8"):
        c = chr(ch)
        if c == "\\":
            out.append("\\\\")
        elif c == '"':
            out.append('\\"')
        elif c == "\n":
            out.append("\\n")
        elif c == "\t":
            out.append("\\t")
        elif c == "\v":
            out.append("\\v")
        elif ch < 0x20 or ch >= 0x7f:
            out.append(f"\\x{ch:02x}\"\"")
        else:
            out.append(c)
    return '"' + "".join(out) + '"'


def c_key(k):
    if isinstance(k, int):
        return str(k)
    if k in KEYS:
        return KEYS[k]
    assert len(k) == 1, k
    return "'\\''" if k == "'" else f"'{k}'"


def esc(s):
    """Text as the oracle writes it (the C program's `esc`) -- and NULL as
    `-`, so a string that is `-` itself as `\\x2d`."""
    if s is None:
        return "-"
    if s == "-":
        return "\\x2d"
    out = []
    for ch in s.encode("utf-8"):
        c = chr(ch)
        if c == "\\":
            out.append("\\\\")
        elif c == "\n":
            out.append("\\n")
        elif c == "\t":
            out.append("\\t")
        elif c == "\v":
            out.append("\\v")
        elif ch < 0x20 or ch >= 0x7f:
            out.append(f"\\x{ch:02x}")
        else:
            out.append(c)
    return "".join(out)


def key_text(k):
    if isinstance(k, int):
        return "'%c'" % k if 0x20 < k < 0x7f else str(k)
    if k in KEYS:
        return k
    return f"'{k}'"


def option_flags(fl):
    return " | ".join(f"OPTION_{f}" for f in fl) or "0"


def tables():
    out = []
    order = list(ARGPS)
    for ident in order:
        out.append(f"static const struct argp A_{c_ident(ident)};")
    for ident in order:
        a = ARGPS[ident]
        n = c_ident(ident)
        opts = [f"    {{{c_str(nm)}, {c_key(k) if k != 0 else '0'}, {c_str(arg)}, "
                f"{option_flags(fl)}, {c_str(doc)}, {grp}}}," for nm, k, arg, fl, doc, grp
                in a["options"]]
        out.append(f"static const struct argp_option O_{n}[] = {{\n" + "\n".join(opts)
                   + "\n    {0}\n};")
        acts = [f"{{{c_key(k)}, {c_str(w)}}}" for k, w in a["react"].items()]
        out.append(f"static const struct react R_{n}[] = {{{', '.join(acts) or '{0, NULL}'}}};")
        if a["filt"] is not None:
            fl = [f"{{{c_key(k)}, {c_str(w)}}}" for k, w in a["filt"].items()]
            out.append(f"static const struct react F_{n}[] = {{{', '.join(fl)}}};")
        nf = len(a["filt"]) if a["filt"] is not None else 0
        out.append(f"static const struct spec SP_{n} = {{{c_str(ident)}, R_{n}, "
                   f"{len(a['react'])}, {'F_' + n if a['filt'] is not None else 'NULL'}, {nf}, "
                   f"{len(a['children'])}}};")
        out.append(f"static error_t P_{n}(int k, char *arg, struct argp_state *st) "
                   f"{{ return run(&SP_{n}, k, arg, st); }}")
        if a["filt"] is not None:
            out.append(f"static char *H_{n}(int k, const char *t, void *in) "
                       f"{{ return filt(&SP_{n}, k, t, in); }}")
        if a["children"]:
            ch = [f"    {{&A_{c_ident(cid)}, {cfl}, {c_str(hdr)}, {grp}}},"
                  for cid, cfl, hdr, grp in a["children"]]
            out.append(f"static const struct argp_child K_{n}[] = {{\n" + "\n".join(ch)
                       + "\n    {0}\n};")
    for ident in order:
        a = ARGPS[ident]
        n = c_ident(ident)
        out.append(f"static const struct argp A_{n} = {{O_{n}, P_{n}, {c_str(a['args_doc'])}, "
                   f"{c_str(a['doc'])}, {'K_' + n if a['children'] else 'NULL'}, "
                   f"{'H_' + n if a['filt'] is not None else 'NULL'}, NULL}};")
    out.append("struct scen { const char *name; const struct argp *argp; int argc; "
               "const char *argv[32]; unsigned flags; int nenv; const char *env[4][2]; "
               "const char *version; int hook; const char *bug; int err_exit; "
               "int help_flags; const char *help_name; };")
    rows = []
    for s in SCENARIOS:
        argv = ", ".join(c_str(x) for x in s["argv"]) or "NULL"
        fl = " | ".join(s["flags"]) or "0"
        env = ", ".join(f"{{{c_str(k)}, {c_str(v)}}}" for k, v in s["env"].items()) or \
            "{NULL, NULL}"
        g = s["globs"]
        if s["how"].startswith("help "):
            _, hf, hn = s["how"].split(" ")
            help_flags, help_name = hf, (None if hn == "-" else hn)
        else:
            help_flags, help_name = "-1", None
        rows.append(f"    {{{c_str(s['name'])}, &A_{c_ident(s['argp'])}, {len(s['argv'])}, "
                    f"{{{argv}}}, {fl}, {len(s['env'])}, {{{env}}}, "
                    f"{c_str(g.get('version'))}, {g.get('hook', '0')}, {c_str(g.get('bug'))}, "
                    f"{g.get('err_exit', '0')}, {help_flags}, {c_str(help_name)}}},")
    out.append("static const struct scen SCEN[] = {\n" + "\n".join(rows) + "\n};")
    return "\n".join(out)


def c_ident(s):
    return "".join(c if c.isalnum() else "_" for c in s)


def descriptions():
    """The parsers and scenarios, as the oracle writes them."""
    lines = []
    for ident, a in ARGPS.items():
        lines.append(f"A\t{ident}\t{esc(a['args_doc'])}\t{esc(a['doc'])}")
        for nm, k, arg, fl, doc, grp in a["options"]:
            lines.append(f"o\t{ident}\t{esc(nm)}\t{key_text(k) if k != 0 else '0'}\t{esc(arg)}\t"
                         f"{','.join(fl) or '0'}\t{esc(doc)}\t{grp}")
        for cid, cfl, hdr, grp in a["children"]:
            lines.append(f"c\t{ident}\t{cid}\t{cfl}\t{esc(hdr)}\t{grp}")
        for k, w in a["react"].items():
            lines.append(f"r\t{ident}\t{key_text(k)}\t{esc(w)}")
        if a["filt"] is not None:
            lines.append(f"f\t{ident}\t-\tkeep")
            for k, w in a["filt"].items():
                lines.append(f"f\t{ident}\t{key_text(k)}\t{esc(w)}")
    return lines


FIXTURE = HERE.parent.parent.parent / "services" / "ctest-argp" / "main.c"

# The scenarios this library answers otherwise than glibc -- posix/src/argp/
# tests.rs's DEVIATIONS, which must name the same: the fixture skips them
# (and with them every scenario under which glibc's help never ends).
DEVIATIONS = ("basic-no-argv", "help-fmt rmargin=abc", "usage-fmt rmargin=abc",
              "help-fmt rmargin=20", "usage-docs", "usage-fmt usage-indent=40,rmargin=50")


def fixture(body):
    """The fixture's main.c: the program built as the fixture, with glibc's
    lines embedded for every scenario but the deviations."""
    expected = []
    keep = True
    for line in body:
        if line.startswith("S\t"):
            keep = line[2:] not in DEVIATIONS
        if keep:
            expected.append(line)
    tabs = tables()
    tabs += "\nstatic const char *const SKIP[] = {" + ", ".join(c_str(n) for n in DEVIATIONS) + "};"
    tabs += ("\nstatic const char *const EXPECTED[] = {\n"
             + ",\n".join("    " + c_str(e) for e in expected) + "\n};")
    return ("/* Generated by posix/tools/oracle/argp_harness.py from glibc 2.39's\n"
            " * answers; do not edit. services/ctest-argp/build.py says what it is. */\n"
            "#define SLATEOS_FIXTURE\n" + C_MAIN.replace("@TABLES@", tabs))


def main() -> None:
    src = C_MAIN.replace("@TABLES@", tables())
    with workdir() as t:
        d = Path(t)
        (d / "argp.c").write_text(src, encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(d)} && gcc -O1 -Wall -Wno-format-truncation -o argp argp.c 2>&1 "
                f"&& LC_ALL=C ./argp")
        if r.returncode != 0:
            sys.exit(f"the harness failed:\n{r.stdout[-4000:]}\n{r.stderr[-3000:]}")
    body = [line for line in r.stdout.split("\n") if line]
    # The scenarios' own lines -- argv, environment, globals -- after each S.
    by_name = {s["name"]: s for s in SCENARIOS}
    out = [
        "# glibc 2.39's argp, for posix/src/argp.rs.",
        "# Generated by posix/tools/oracle/argp_harness.py, which says the forms; do not edit.",
    ] + descriptions()
    for line in body:
        if line.startswith("S\t"):
            s = by_name[line[2:]]
            out.append(f"S\t{s['name']}\t{s['argp']}\t{','.join(s['flags']) or '0'}\t{s['how']}")
            out.append("a" + "".join(f"\t{esc(x)}" for x in s["argv"]))
            for k, v in s["env"].items():
                out.append(f"e\t{k}\t{esc(v)}")
            for k, v in s["globs"].items():
                out.append(f"g\t{k}\t{esc(v)}")
        else:
            out.append(line)
    OUT.write_text("\n".join(out) + "\n", encoding="utf-8", newline="\n")
    FIXTURE.parent.mkdir(parents=True, exist_ok=True)
    FIXTURE.write_text(fixture(body), encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {len(SCENARIOS)} scenarios, {len(out)} lines; "
          f"{FIXTURE.parent.name}/main.c")


if __name__ == "__main__":
    main()
