"""Compile the Indic syllable grammar to a DFA and write `indic_machine.rs`.

A syllable is the unit the Indic shaper reorders, and deciding where one
ends is a *lexing* problem: HarfBuzz writes the answer as a ragel scanner
(`hb-ot-shaper-indic-machine.rl`), seven regular expressions over the
character categories, matched longest-first with ties broken by the order
they are listed.

This script holds the same seven expressions — transcribed below, one
Python tuple per ragel line — and compiles them the way ragel would:
Thompson construction to an NFA, subset construction to a DFA, then a
transition table emitted as Rust. The Rust side is then twenty lines of
table walking with no grammar in it at all.

# Why not hand-write the matcher

Because the grammar is ambiguous and the ambiguity is not local. `z` can
begin both alternatives of `halant_or_matra_group`, so a greedy matcher
that commits to `final_halant_group` on seeing a ZWNJ and then fails must
back up — and "back up and try the other branch" is exactly the thing a
subset construction does once, at build time, instead of at every
character. Getting it wrong does not crash: it silently cuts a syllable in
the wrong place, and the text is reordered wrongly for the rest of the
run.

# Why not ship ragel's output

Ragel is not on this machine and its C output is a goto-threaded state
machine that does not translate to Rust without a rewrite. Compiling the
grammar ourselves keeps the *grammar* — the part worth reading — in one
readable place, and makes re-deriving it after an upstream change a
matter of editing seven lines.

# What is deliberately dropped

`RS` (register shifter) and `VD` (dependent vowel) appear in HarfBuzz's
grammar but no character in our table derives to either: every `RS` is
Khmer's U+17C9 or U+17CA, which the category overrides turn into `Robatic`
before the table is written, and nothing anywhere maps to `VD` (HarfBuzz
gives it the same machine symbol as `A`). Their branches are therefore
unreachable, and are left out rather than encoded as states nothing can
enter. This is the only difference from upstream's grammar.

# Why the Khmer categories are columns here

`CATEGORIES` is shared with the Khmer machine, so this table has seven
columns — `VAbv`, `VBlw`, `VPre`, `VPst`, `Robatic`, `Xgroup`, `Ygroup` —
that no Indic rule names. They are not wasted: `other = any` is written as
an alternation over *every* category, so a Khmer character arriving at the
Indic machine is one non-Indic syllable rather than an index off the end of
a row. It should never arrive — the two are dispatched on script — but a
table that answers rather than panics is the one to have.
"""

import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from gen_indic_tables import CATEGORIES, RUST_CATEGORY  # noqa: E402

# ---------------------------------------------------------------------------
# The grammar, transcribed from `hb-ot-shaper-indic-machine.rl`.
#
# A pattern is a tuple: ("cat", NAME) matches one character of that category,
# and seq/alt/star/opt/eps combine them. `s(...)` and `a(...)` are shorthand.
# ---------------------------------------------------------------------------

EPS = ("eps",)


def s(*parts):
    """Concatenation."""
    return ("seq", *parts)


def a(*parts):
    """Alternation."""
    return ("alt", *parts)


def star(p):
    """Zero or more."""
    return ("star", p)


def opt(p):
    """Zero or one."""
    return ("alt", p, EPS)


def c(name):
    """One character of category `name`."""
    if name not in CATEGORIES:
        raise SystemExit(f"no such category {name!r}")
    return ("cat", name)


# c = (C | Ra)                      # is_consonant
CONSONANT = a(c("C"), c("Ra"))
# n = ((ZWNJ?.RS)? (N.N?)?)         # is_consonant_modifier, less the dead RS
N_MOD = opt(s(c("N"), opt(c("N"))))
# z = ZWJ|ZWNJ                      # is_joiner
Z = a(c("ZWJ"), c("ZWNJ"))
# reph = (Ra H | Repha)
REPH = a(s(c("Ra"), c("H")), c("Repha"))
# sm = SM | SMPst
SM = a(c("SM"), c("SMPst"))

# cn = c.ZWJ?.n?
CN = s(CONSONANT, opt(c("ZWJ")), opt(N_MOD))
# symbol = Symbol.N?
SYMBOL = s(c("Symbol"), opt(c("N")))
# matra_group = z*.(M | sm? MPst).N?.H?
MATRA_GROUP = s(
    star(Z),
    a(c("M"), s(opt(SM), c("MPst"))),
    opt(c("N")),
    opt(c("H")),
)
# syllable_tail = (z?.sm.sm?.ZWNJ?)? (A | VD)*     # VD is dead
SYLLABLE_TAIL = s(
    opt(s(opt(Z), SM, opt(SM), opt(c("ZWNJ")))),
    star(c("A")),
)
# halant_group = (z?.H.(ZWJ.N?)?)
HALANT_GROUP = s(opt(Z), c("H"), opt(s(c("ZWJ"), opt(c("N")))))
# final_halant_group = halant_group | H.ZWNJ
FINAL_HALANT_GROUP = a(HALANT_GROUP, s(c("H"), c("ZWNJ")))
# medial_group = CM?
MEDIAL_GROUP = opt(c("CM"))
# halant_or_matra_group = (final_halant_group | matra_group*)
HALANT_OR_MATRA_GROUP = a(FINAL_HALANT_GROUP, star(MATRA_GROUP))

# complex_syllable_tail = (halant_group.cn)* medial_group
#                         halant_or_matra_group syllable_tail
COMPLEX_TAIL = s(
    star(s(HALANT_GROUP, CN)),
    MEDIAL_GROUP,
    HALANT_OR_MATRA_GROUP,
    SYLLABLE_TAIL,
)

# The seven scanner rules, in the order ragel lists them — which is the
# order ties are broken in, so it is part of the answer, not presentation.
# The Rust variant each maps to is the second element.
RULES = (
    # consonant_syllable = (Repha|CS)? cn complex_syllable_tail
    (s(opt(a(c("Repha"), c("CS"))), CN, COMPLEX_TAIL), "Consonant"),
    # vowel_syllable = reph? V.n? (ZWJ | complex_syllable_tail)
    (
        s(
            opt(REPH),
            c("V"),
            opt(N_MOD),
            a(c("ZWJ"), COMPLEX_TAIL),
        ),
        "Vowel",
    ),
    # standalone_cluster = ((Repha|CS)? PLACEHOLDER | reph? DOTTEDCIRCLE).n?
    #                      complex_syllable_tail
    (
        s(
            a(
                s(opt(a(c("Repha"), c("CS"))), c("PLACEHOLDER")),
                s(opt(REPH), c("DOTTEDCIRCLE")),
            ),
            opt(N_MOD),
            COMPLEX_TAIL,
        ),
        "Standalone",
    ),
    # symbol_cluster = symbol syllable_tail
    (s(SYMBOL, SYLLABLE_TAIL), "Symbol"),
    # SMPst
    (c("SMPst"), "NonIndic"),
    # broken_cluster = reph? n? complex_syllable_tail
    (s(opt(REPH), opt(N_MOD), COMPLEX_TAIL), "Broken"),
    # other = any
    (a(*(c(name) for name in CATEGORIES)), "NonIndic"),
)

# The Rust enum the rules name. Order is the order of the variants there.
SYLLABLE_TYPES = ("Consonant", "Vowel", "Standalone", "Symbol", "Broken", "NonIndic")


# ---------------------------------------------------------------------------
# Thompson construction.
#
# An NFA is a list of states; `eps[i]` is the set reachable without reading,
# `trans[i]` maps a category name to the set reachable by reading it. Every
# fragment has exactly one entry and one exit state, which is what makes the
# combinators one line each.
# ---------------------------------------------------------------------------


class Nfa:
    def __init__(self):
        self.eps = []
        self.trans = []

    def state(self):
        self.eps.append(set())
        self.trans.append({})
        return len(self.eps) - 1

    def build(self, pattern):
        """Return (entry, exit) for `pattern`."""
        kind = pattern[0]
        if kind == "eps":
            q = self.state()
            return q, q
        if kind == "cat":
            entry, exit_ = self.state(), self.state()
            self.trans[entry].setdefault(pattern[1], set()).add(exit_)
            return entry, exit_
        if kind == "seq":
            entry = exit_ = self.state()
            for part in pattern[1:]:
                sub_in, sub_out = self.build(part)
                self.eps[exit_].add(sub_in)
                exit_ = sub_out
            return entry, exit_
        if kind == "alt":
            entry, exit_ = self.state(), self.state()
            for part in pattern[1:]:
                sub_in, sub_out = self.build(part)
                self.eps[entry].add(sub_in)
                self.eps[sub_out].add(exit_)
            return entry, exit_
        if kind == "star":
            entry, exit_ = self.state(), self.state()
            sub_in, sub_out = self.build(pattern[1])
            self.eps[entry].add(sub_in)
            self.eps[entry].add(exit_)
            self.eps[sub_out].add(sub_in)
            self.eps[sub_out].add(exit_)
            return entry, exit_
        raise SystemExit(f"unknown pattern {kind!r}")

    def closure(self, states):
        """Every state reachable from `states` without reading a character."""
        seen = set(states)
        stack = list(states)
        while stack:
            q = stack.pop()
            for r in self.eps[q]:
                if r not in seen:
                    seen.add(r)
                    stack.append(r)
        return frozenset(seen)

    def step(self, states, name):
        """Where `states` goes on reading one character of category `name`."""
        out = set()
        for q in states:
            out |= self.trans[q].get(name, set())
        return self.closure(out)


def compile_rules(rules, categories=CATEGORIES):
    """Subset-construct a scanner over `rules`. Returns (transitions, accepts).

    `rules` is a sequence of `(pattern, syllable_name)`, listed in the order
    ragel would list them: ties go to the earliest. `gen_khmer_machine.py`
    calls this with its own three rules — the construction is the same, only
    the grammar differs.

    `categories` is the alphabet, in the order the transition table's columns
    are to be laid out. It defaults to the Indic categories, which Khmer and
    Myanmar share; `gen_universal_machine.py` passes its own, since USE keeps
    its category in a different buffer slot and shares nothing with these.
    """
    nfa = Nfa()
    start = nfa.state()
    # `accepting[state] = rule index`. A DFA state accepts the *lowest*
    # rule index among the NFA states in it, which is ragel's tie-break.
    accepting = {}
    for i, (pattern, _) in enumerate(rules):
        sub_in, sub_out = nfa.build(pattern)
        nfa.eps[start].add(sub_in)
        accepting[sub_out] = min(accepting.get(sub_out, i), i)

    dead = frozenset()
    start_set = nfa.closure({start})
    order = [dead, start_set]
    index = {dead: 0, start_set: 1}
    transitions = []
    accepts = []
    i = 0
    while i < len(order):
        states = order[i]
        row = []
        for name in categories:
            nxt = nfa.step(states, name) if states else dead
            if nxt not in index:
                index[nxt] = len(order)
                order.append(nxt)
            row.append(index[nxt])
        transitions.append(row)
        rules = [accepting[q] for q in states if q in accepting]
        accepts.append(min(rules) if rules else None)
        i += 1
    return transitions, accepts


def minimise(transitions, accepts):
    """Merge states no input can tell apart (Moore's algorithm).

    Subset construction produces a state per reachable *set*, and many of
    those sets behave identically — the difference between them is which NFA
    branch got there, which no longer matters once the DFA is built. Ragel
    minimises too, so this also keeps our state count comparable to
    upstream's when the grammar is next diffed against it.
    """
    # Two states start out distinguishable only if they accept differently.
    part = {}
    classes = {}
    for s_ in range(len(transitions)):
        classes.setdefault(accepts[s_], len(classes))
        part[s_] = classes[accepts[s_]]

    while True:
        keys = {}
        nxt = {}
        for s_ in range(len(transitions)):
            key = (part[s_], tuple(part[t] for t in transitions[s_]))
            nxt[s_] = keys.setdefault(key, len(keys))
        if len(keys) == len(set(part.values())):
            break
        part = nxt

    # Relabel so the dead state is 0 and the start state is 1, which is the
    # contract the Rust side reads the table under.
    if part[0] == part[1]:
        raise SystemExit("the start state is dead — the grammar matches nothing")
    remap = {part[0]: 0, part[1]: 1}
    for s_ in range(len(transitions)):
        remap.setdefault(part[s_], len(remap))
    out_trans = [None] * len(remap)
    out_accepts = [None] * len(remap)
    for s_ in range(len(transitions)):
        i = remap[part[s_]]
        out_trans[i] = [remap[part[t]] for t in transitions[s_]]
        out_accepts[i] = accepts[s_]
    return out_trans, out_accepts


def main():
    transitions, accepts = compile_rules(RULES)
    before = len(transitions)
    transitions, accepts = minimise(transitions, accepts)
    if len(transitions) > 250:
        raise SystemExit(f"{len(transitions)} states will not fit in a u8")

    out = pathlib.Path(__file__).parent / ".." / "src" / "indic_machine.rs"
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        w = f.write
        w(
            "//! The Indic syllable machine, as a DFA.\n"
            "//!\n"
            "//! Generated by `gui/font/tools/gen_indic_machine.py` from the\n"
            "//! same seven regular expressions HarfBuzz's ragel scanner uses.\n"
            "//! Do not edit: run the script instead, where the grammar is\n"
            "//! written out and explained.\n"
            "\n"
            "use crate::indic::Syllable;\n"
            "\n"
            "/// State 0 is dead — nothing leaves it — and state 1 is the\n"
            "/// start. A row is indexed by [`Category`](crate::indic::Category)\n"
            "/// cast to `usize`, so the enum's variant order is part of this\n"
            "/// table.\n"
            f"pub(crate) static TRANSITIONS: [[u8; {len(CATEGORIES)}]; "
            f"{len(transitions)}] = [\n"
        )
        for row in transitions:
            w("    [" + ", ".join(str(x) for x in row) + "],\n")
        w("];\n\n")
        w(
            "/// Which rule a state accepts, if any. Where more than one\n"
            "/// rule could accept, this is the first one the grammar lists,\n"
            "/// which is how the scanner breaks a tie.\n"
            f"pub(crate) static ACCEPTS: [Option<Syllable>; {len(accepts)}] = [\n"
        )
        for rule in accepts:
            if rule is None:
                w("    None,\n")
            else:
                w(f"    Some(Syllable::{RULES[rule][1]}),\n")
        w("];\n")

    print(f"wrote {out}")
    print(
        f"  {len(transitions)} states over {len(CATEGORIES)} categories"
        f" ({before} before minimisation)"
    )
    named = sorted({RULES[r][1] for r in accepts if r is not None})
    print(f"  accepting: {', '.join(named)}")
    unused = [t for t in SYLLABLE_TYPES if t not in named]
    if unused:
        raise SystemExit(f"no state accepts {unused} — the grammar lost a rule")
    if set(RUST_CATEGORY) != set(CATEGORIES):
        raise SystemExit("the category table and the machine disagree")


if __name__ == "__main__":
    main()
