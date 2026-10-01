r"""fmtmsg and addseverity, as posix/src/fmtmsg.rs does them, written a second
way: from XSH fmtmsg and glibc's manual, and held to glibc 2.39's answers
(fmtmsg_oracle.txt) wherever those leave a case open.

It exists for the cases where this library's answer is not glibc's --
design-decisions section 1150 -- and fmtmsg_harness.py uses it to write
fmtmsg_deviations.txt, which posix/src/fmtmsg.rs's tests hold the Rust to
instead. There is one rule behind them: glibc reads MSGVERB and SEV_LEVEL at
a process's first fmtmsg, so an addseverity before it is undone by
SEV_LEVEL (and cannot remove a level SEV_LEVEL is yet to define), while one
after it stands; this reads them at the first call of either function, so a
program's addseverity always has the last word.

    python posix/tools/oracle/fmtmsg_model.py posix/src/fmtmsg_oracle.txt

prints how many of the oracle's cases the model answers otherwise than
glibc, as this library does and as glibc does (0 is the model being right).
"""

import sys
from pathlib import Path

MM_PRINT, MM_CONSOLE = 0x100, 0x200
KEYWORDS = [("label", 1), ("severity", 2), ("text", 4), ("action", 8), ("tag", 16)]
ALL = 31
STANDARD = {0: "", 1: "HALT", 2: "ERROR", 3: "WARNING", 4: "INFO"}


def verb_mask(value):
    """MSGVERB's parts: all of them unless every keyword is one, each ending
    the value or followed by a colon."""
    if not value:
        return ALL
    mask, i = 0, 0
    while i < len(value):
        for word, bit in KEYWORDS:
            end = i + len(word)
            if value.startswith(word, i) and (end == len(value) or value[end] == ":"):
                mask |= bit
                i = end + (1 if end < len(value) else 0)
                break
        else:
            return ALL
    return mask


def strtol(s, i):
    """C's strtol(s + i, &end, 0): (value, end)."""
    j = i
    while j < len(s) and s[j] in " \t\n\v\f\r":
        j += 1
    sign = 1
    if j < len(s) and s[j] in "+-":
        sign = -1 if s[j] == "-" else 1
        j += 1
    base = 10
    if s.startswith(("0x", "0X"), j) and j + 2 < len(s) and s[j + 2] in "0123456789abcdefABCDEF":
        base, j = 16, j + 2
    elif j < len(s) and s[j] == "0":
        base = 8
    start, v = j, 0
    digits = "0123456789abcdef"[:base]
    while j < len(s) and s[j].lower() in digits:
        v = v * base + digits.index(s[j].lower())
        j += 1
    if j == start:
        return 0, i
    return sign * v, j


def sev_levels(value, levels):
    """SEV_LEVEL's entries -- `keyword,level,printstring`, colon-separated --
    for levels above MM_INFO."""
    i = 0
    while i < len(value):
        end = value.find(":", i)
        end = len(value) if end < 0 else end
        j = i
        while j < end:
            j += 1
            if value[j - 1] == ",":
                break
        if j < end:
            level, k = strtol(value, j)
            if level > 4 and k < len(value) and value[k] == ",":
                levels[level] = value[k + 1:end]
        i = end + (1 if end < len(value) else 0)


class State:
    def __init__(self, verb, sev, glibc_order):
        self.verb, self.sev, self.glibc_order = verb, sev, glibc_order
        self.ready = False
        self.mask = ALL
        self.levels = {}

    def init(self):
        if not self.ready:
            self.ready = True
            self.mask = verb_mask(self.verb)
            if self.sev is not None:
                sev_levels(self.sev, self.levels)

    def addseverity(self, n, s):
        if not self.glibc_order:
            self.init()
        if n <= 4:
            return -1
        if s is None:
            if n in self.levels:
                del self.levels[n]
                return 0
            return -1
        self.levels[n] = s
        return 0

    def fmtmsg(self, cls, label, severity, text, action, tag, closed):
        """(return, what standard error got)."""
        self.init()
        if label is not None:
            c = label.find(":")
            if c < 0 or c > 10 or len(label) - c - 1 > 14:
                return -1, ""
        if severity in STANDARD:
            string = STANDARD[severity]
        elif severity in self.levels:
            string = self.levels[severity]
        else:
            return -1, ""
        result, out = 0, ""
        if cls & MM_PRINT:
            if closed:
                result = 1
            else:
                out = compose(self.mask, label, severity, string, text, action, tag)
        # The console: WSL's opened, so glibc's answers are for one that works.
        return result, out


def compose(mask, label, severity, string, text, action, tag):
    do_label = mask & 1 and label is not None
    do_sev = mask & 2 and severity != 0
    do_text = mask & 4 and text is not None
    do_action = mask & 8 and action is not None
    do_tag = mask & 16 and tag is not None
    return ((label if do_label else "")
            + (": " if do_label and (do_sev or do_text or do_action or do_tag) else "")
            + (string if do_sev else "")
            + (": " if do_sev and (do_text or do_action or do_tag) else "")
            + (text if do_text else "")
            + ("\n" if do_text and (do_action or do_tag) else "")
            + ("TO FIX: " + action if do_action else "")
            + ("  " if do_action and do_tag else "")
            + (tag if do_tag else "")
            + "\n")


def unescape(t):
    r"""The oracle's escapes back: `\x` NULL, `""` empty, `\xNN` a byte."""
    if t == "\\x":
        return None
    if t == '""':
        return ""
    out, i = [], 0
    while i < len(t):
        if t.startswith("\\x", i) and i + 4 <= len(t):
            out.append(chr(int(t[i + 2:i + 4], 16)))
            i += 4
        else:
            out.append(t[i])
            i += 1
    return "".join(out)


def escape(s):
    if not s:
        return "\\x"
    return "".join(f"\\x{ord(c):02x}" if c in "\\ \"" or not "!" <= c <= "~" else c for c in s)


def parse(line):
    """A case of the oracle: (fields, (return, adds, stderr))."""
    left, _, right = line.partition(" = ")
    fields = dict(f.split("=", 1) for f in left.split(" "))
    ret, adds, err = right.split(" ", 2)
    return fields, (int(ret), adds, err)


def answer(fields, glibc_order=False):
    """The case's line, as the oracle writes glibc's, answered here."""
    st = State(unescape(fields["verb"]), unescape(fields["sev"]), glibc_order)
    got = []
    if fields["add"] != "\\x":
        for op in fields["add"].split(","):
            n, s = op.split("/", 1)
            got.append(st.addseverity(int(n), unescape(s)))
    ret, out = st.fmtmsg(int(fields["class"]), unescape(fields["label"]), int(fields["severity"]),
                         unescape(fields["text"]), unescape(fields["action"]),
                         unescape(fields["tag"]), fields.get("stderr") == "closed")
    adds = ",".join(str(g) for g in got) if got else "-"
    return f"{ret} {adds} {escape(out)}"


def main():
    lines = [line for line in Path(sys.argv[1]).read_text(encoding="utf-8").splitlines()
             if not line.startswith("#")]
    for order, name in ((True, "as glibc orders it"), (False, "as this library does")):
        bad = []
        for line in lines:
            fields, _ = parse(line)
            want = line.partition(" = ")[2]
            if answer(fields, order) != want:
                bad.append(line)
        print(f"{len(bad)} of {len(lines)} differ from glibc {name}")
        for b in bad[:6]:
            print("  ", b[:200])


if __name__ == "__main__":
    main()
