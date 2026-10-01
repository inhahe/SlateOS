#!/bin/python3
"""Oils' spec-test harness, ported from Python 2 to Python 3 to run on SlateOS.

A port of `test/sh_spec.py` and the parts of `test/spec_lib.py` it uses, from
Oils' `release/0.38.0` branch (https://github.com/oils-for-unix/oils). Upstream
runs these under Python 2; SlateOS's only Python is CPython 3.12, and the spec
tests have to run *there* -- design-decisions.md §1043 asks for genuine Oils'
spec tests on SlateOS before it becomes the default shell. Oils is under the
Apache License 2.0 (`LICENSE.txt` beside this file); this file is a derivative
work and keeps that license.

What is the same, and must stay the same: the test-file grammar and its
parser, how assertions are built for a shell (qualifiers `OK`, `BUG`, `N-I`
and their numbered forms, the default `status: 0`, the "no Python traceback on
stderr" check), how a cell's result is the worst of its assertions, the
environment a case runs in, how each case is run (its code on the shell's
stdin, a fresh directory as `$TMP` and working directory), and the
success rule at the end. `scripts/oils-spec/validate.sh` runs this and
upstream's harness side by side over every spec file and requires the two
results tables to be identical.

What changed, and why:

* **Bytes throughout.** Python 2 read the test files, the shells' output and
  the expected values as byte strings, with ASCII-only regex classes and
  `strip`. This does the same with `bytes`, so a non-UTF-8 byte in a test or
  an output is compared exactly, never decoded.
* **The one comparison Python 2 got from implicit coercion is spelled out.**
  A `stdout-json:` value parses to a unicode string; Python 2 compared it with
  a byte-string output by decoding the bytes as ASCII, and treated a failure
  to decode as "not equal". `_py2_equal` reproduces that, including its
  consequence that a non-ASCII `-json` expectation can never match.
* **Timeouts are Python's,** not an external `timeout -s KILL`. As under
  `timeout`, the shell then gets a process group of its own, and the whole
  group is killed when time is up, so a background job that keeps the pipes
  open cannot hang the run; it is reported as `TIME`, as upstream's `-9` was.
  Off by default, as upstream's is, and without one the shell shares the
  harness's group exactly as it did upstream.
* **Output** is a plain text table (`--format text`) or upstream's TSV
  (`--tsv-output`), not ANSI or HTML: the TSV is what `validate.sh` compares,
  and a serial console has no use for colour.
* Upstream-only modes for its CI (HTML reports, `--yahtzee-out-file`,
  `--pyann-out-dir`, `--stats-template`, `--test-case-json`) are left out.

usage:
    sh_spec.py [options] TEST_FILE SHELL...
    sh_spec.py [options] --oils-bin-dir DIR TEST_FILE
"""

from __future__ import annotations

import argparse
import collections
import errno
import json
import os
import re
import shutil
import signal
import subprocess
import sys

# ---------------------------------------------------------------------------
# spec_lib
# ---------------------------------------------------------------------------


def log(msg, *args):
    if args:
        msg = msg % args
    print(msg, file=sys.stderr)


OSH_CPP_RE = re.compile(r'_bin/\w+-\w+(-sh)?/osh')
YSH_CPP_RE = re.compile(r'_bin/\w+-\w+(-sh)?/ysh')
OIL_CPP_RE = re.compile(r'_bin/\w+-\w+(-sh)?/oil')


def MakeShellPairs(shells):
    """(label, path) for each shell, labelled as upstream labels them."""
    shell_pairs = []
    saw_osh = saw_ysh = saw_oil = False
    for path in shells:
        if path[-1].isdigit():  # bash-4.4, zsh-5.9
            label = os.path.basename(path)[:7]
        else:
            first, _ = os.path.splitext(path)
            label = os.path.basename(first)
        if label == 'sh':
            label = 'toysh'
        if label == 'osh':
            if saw_osh:
                label = 'osh-cpp' if OSH_CPP_RE.search(path) else 'osh_ALT'
            saw_osh = True
        elif label == 'ysh':
            if saw_ysh:
                label = 'ysh-cpp' if YSH_CPP_RE.search(path) else 'ysh_ALT'
            saw_ysh = True
        elif label == 'oil':
            if saw_oil:
                label = 'oil-cpp' if OIL_CPP_RE.search(path) else 'oil_ALT'
            saw_oil = True
        shell_pairs.append((label, path))
    return shell_pairs


RANGE_RE = re.compile(r'(\d+) \s* - \s* (\d+)', re.VERBOSE)


def ParseRange(range_str):
    try:
        d = int(range_str)
        return d, d
    except ValueError:
        m = RANGE_RE.match(range_str)
        if not m:
            raise RuntimeError('Invalid range %r' % range_str)
        b, e = m.groups()
        return int(b), int(e)


class RangePredicate:
    """Zero-based, inclusive."""

    def __init__(self, begin, end):
        self.begin = begin
        self.end = end

    def __call__(self, i, case):
        return self.begin <= i <= self.end


class RegexPredicate:

    def __init__(self, desc_re):
        self.desc_re = desc_re

    def __call__(self, i, case):
        return bool(self.desc_re.search(_text(case['desc'])))


# ---------------------------------------------------------------------------
# Parsing a test file
# ---------------------------------------------------------------------------

OSH_CPYTHON = ('osh', 'osh-dbg')
OTHER_OSH = ('osh_ALT', )
YSH_CPYTHON = ('ysh', 'ysh-dbg')
OTHER_YSH = ('oil_ALT', )
OTHER_OILS = OTHER_OSH + OTHER_YSH + ('osh-cpp', 'ysh-cpp')


class ParseError(Exception):
    pass


# Byte patterns: Python 2 matched byte strings, where \s and \w are ASCII.
KEY_VALUE_RE = re.compile(
    rb'''
   [#][#] \s+
   (?: (OK(?:-\d)? | BUG(?:-\d)? | N-I) \s+ ([\w+/]+) \s+ )?
   ([\w\-]+)
   :
   \s* (.*)
''', re.VERBOSE)

END_MULTILINE_RE = re.compile(rb'''
    [#][#] \s+ END
''', re.VERBOSE)

TEST_CASE_BEGIN = 0
KEY_VALUE = 1
KEY_VALUE_MULTILINE = 2
END_MULTILINE = 3
PLAIN_LINE = 4
EOF = 5

LEX_OUTER = 0  # blank lines ignored
LEX_RAW = 1  # blank lines significant


def _ascii(b):
    """A name from the file -- a key, a qualifier, a shell -- as text. The
    regex admits only ASCII there, so this cannot fail."""
    return b.decode('ascii') if b is not None else None


def _text(b):
    """Bytes for display only; never compared."""
    return b.decode('utf-8', 'replace') if isinstance(b, bytes) else b


class Tokenizer:
    """Modal lexer, over a file opened in binary mode."""

    def __init__(self, f):
        self.f = f
        self.cursor = None
        self.line_num = 0
        self.next()

    def _ClassifyLine(self, line, lex_mode):
        if not line:
            return self.line_num, EOF, b''

        if lex_mode == LEX_OUTER and not line.strip():
            return None

        if line.startswith(b'####'):
            return self.line_num, TEST_CASE_BEGIN, line[4:].strip()

        m = KEY_VALUE_RE.match(line)
        if m:
            qualifier, shells, name, value = m.groups()
            qualifier, shells, name = _ascii(qualifier), _ascii(shells), _ascii(name)
            # Upstream's HACK: expected data has the newline.
            if name in ('stdout', 'stderr'):
                value += b'\n'
            if name in ('STDOUT', 'STDERR'):
                token_type = KEY_VALUE_MULTILINE
            else:
                token_type = KEY_VALUE
            return self.line_num, token_type, (qualifier, shells, name, value)

        if END_MULTILINE_RE.match(line):
            return self.line_num, END_MULTILINE, None

        if line.startswith(b'##'):
            raise RuntimeError('Invalid ## line %r' % line)

        if line.lstrip().startswith(b'#'):  # comments are ignored
            return None

        return self.line_num, PLAIN_LINE, line

    def next(self, lex_mode=LEX_OUTER):
        while True:
            line = self.f.readline()
            self.line_num += 1
            tok = self._ClassifyLine(line, lex_mode)
            if tok is not None:
                break
        self.cursor = tok
        return self.cursor

    def peek(self):
        return self.cursor


def AddMetadataToCase(case, qualifier, shells, name, value, line_num):
    for shell in shells.split('/'):
        if shell not in case:
            case[shell] = {}
        name_without_type = re.sub(r'-json$', '', name)
        if (name_without_type in case[shell] or
                name_without_type + '-json' in case[shell]):
            raise ParseError('Line %d: duplicate spec %r for %r' %
                             (line_num, name, shell))
        if 'qualifier' in case[shell] and qualifier != case[shell]['qualifier']:
            raise ParseError(
                'Line %d: inconsistent qualifier %r is specified for %r, '
                'but %r was previously specified.  ' %
                (line_num, qualifier, shell, case[shell]['qualifier']))
        case[shell][name] = value
        case[shell]['qualifier'] = qualifier


def ParseKeyValue(tokens, case):
    """The contiguous commented-out metadata of a case."""
    while True:
        line_num, kind, item = tokens.peek()

        if kind == KEY_VALUE_MULTILINE:
            qualifier, shells, name, empty_value = item
            if empty_value:
                raise ParseError(
                    'Line %d: got value %r for %r, but the value should be on the '
                    'following lines' % (line_num, empty_value, name))
            value_lines = []
            while True:
                tokens.next(lex_mode=LEX_RAW)
                _, kind2, item2 = tokens.peek()
                if kind2 != PLAIN_LINE:
                    break
                value_lines.append(item2)
            value = b''.join(value_lines)
            name = name.lower()  # STDOUT -> stdout
            if qualifier:
                AddMetadataToCase(case, qualifier, shells, name, value, line_num)
            else:
                case[name] = value
            if kind2 == END_MULTILINE:  # optional
                tokens.next()

        elif kind == KEY_VALUE:
            qualifier, shells, name, value = item
            if qualifier:
                AddMetadataToCase(case, qualifier, shells, name, value, line_num)
            else:
                case[name] = value
            tokens.next()

        else:
            break


def ParseCodeLines(tokens, case):
    _, kind, item = tokens.peek()
    if kind != PLAIN_LINE:
        raise ParseError('Expected a line of code (got %r, %r)' % (kind, item))
    code_lines = []
    while True:
        _, kind, item = tokens.peek()
        if kind != PLAIN_LINE:
            case['code'] = b''.join(code_lines)
            return
        code_lines.append(item)
        tokens.next(lex_mode=LEX_RAW)


def ParseTestCase(tokens):
    """One case, or None at EOF."""
    line_num, kind, item = tokens.peek()
    if kind == EOF:
        return None
    if kind != TEST_CASE_BEGIN:
        raise RuntimeError("line %d: Expected TEST_CASE_BEGIN, got %r" %
                           (line_num, [kind, item]))
    tokens.next()
    case = {'desc': item, 'line_num': line_num}
    ParseKeyValue(tokens, case)
    if 'code' in case:  # given as a key-value pair
        return case
    ParseCodeLines(tokens, case)
    ParseKeyValue(tokens, case)
    return case


_META_FIELDS = [
    'our_shell',
    'compare_shells',
    'suite',
    'tags',
    'oils_failures_allowed',
    'oils_cpp_failures_allowed',
    'legacy_tmp_dir',
]


def ParseTestFile(test_file, tokens):
    file_metadata = {}
    test_cases = []
    while True:
        line_num, kind, item = tokens.peek()
        if kind != KEY_VALUE:
            break
        qualifier, shells, name, value = item
        if qualifier is not None:
            raise RuntimeError('Invalid qualifier in spec file metadata')
        if shells is not None:
            raise RuntimeError('Invalid shells in spec file metadata')
        file_metadata[name] = value
        tokens.next()
    while True:
        test_case = ParseTestCase(tokens)
        if test_case is None:
            break
        test_cases.append(test_case)
    for name in file_metadata:
        if name not in _META_FIELDS:
            raise RuntimeError('Invalid file metadata %r in %r' % (name, test_file))
    return file_metadata, test_cases


# ---------------------------------------------------------------------------
# Assertions and results
# ---------------------------------------------------------------------------


def CreateStringAssertion(d, key, assertions, qualifier=False):
    found = False
    exp = d.get(key)
    if exp is not None:
        assertions.append(EqualAssertion(key, exp, qualifier=qualifier))
        found = True
    exp_json = d.get(key + '-json')
    if exp_json is not None:
        # A str, as Python 2's json gave a unicode string; see _py2_equal.
        exp = json.loads(exp_json.decode('utf-8'))
        assertions.append(EqualAssertion(key, exp, qualifier=qualifier))
        found = True
    return found


def CreateIntAssertion(d, key, assertions, qualifier=False):
    exp = d.get(key)
    if exp is not None:
        assertions.append(EqualAssertion(key, int(exp), qualifier=qualifier))
        return True
    return False


def CreateAssertions(case, sh_label):
    """The assertions a shell's run of a case must meet."""
    assertions = []
    stdout = stderr = status = False

    if sh_label.startswith('osh'):
        case_sh = 'osh'
    elif sh_label.startswith('bash'):
        case_sh = 'bash'
    else:
        case_sh = sh_label

    if case_sh in case:
        q = case[case_sh]['qualifier']
        if CreateStringAssertion(case[case_sh], 'stdout', assertions, qualifier=q):
            stdout = True
        if CreateStringAssertion(case[case_sh], 'stderr', assertions, qualifier=q):
            stderr = True
        if CreateIntAssertion(case[case_sh], 'status', assertions, qualifier=q):
            status = True

    if not stdout:
        CreateStringAssertion(case, 'stdout', assertions)
    if not stderr:
        CreateStringAssertion(case, 'stderr', assertions)
    if not status:
        if 'status' in case:
            CreateIntAssertion(case, 'status', assertions)
        else:
            # Unspecified status means it must exit 0.
            assertions.append(EqualAssertion('status', 0))

    assertions.append(SubstringAssertion('stderr', b'Traceback (most recent'))
    return assertions


class Result:
    """Ordered: a cell's result is the minimum of its assertions'."""
    TIMEOUT = 0
    FAIL = 1
    BUG = 2
    BUG_2 = 3
    NI = 4
    OK = 5
    OK_2 = 6
    OK_3 = 7
    OK_4 = 8
    PASS = 9
    length = 10


def QualifierToResult(qualifier):
    return {
        'BUG': Result.BUG,
        'BUG-2': Result.BUG_2,
        'N-I': Result.NI,
        'OK': Result.OK,
        'OK-2': Result.OK_2,
        'OK-3': Result.OK_3,
        'OK-4': Result.OK_4,
    }.get(qualifier, Result.PASS)


def _py2_equal(actual, expected):
    """`actual == expected` as Python 2 answered it.

    Byte strings and ints compare as themselves. A unicode string (a `-json`
    expectation) against a byte string made Python 2 decode the bytes as
    ASCII; when that failed it warned and called them unequal. So such an
    expectation matches only output that is pure ASCII and equal.
    """
    if isinstance(expected, str) and isinstance(actual, bytes):
        try:
            return actual.decode('ascii') == expected
        except UnicodeDecodeError:
            return False
    return actual == expected


def _py2_repr(value):
    """repr() as Python 2 printed it, for messages only."""
    if isinstance(value, bytes):
        return repr(value)[1:]
    if isinstance(value, str):
        return 'u' + repr(value)
    return repr(value)


class EqualAssertion:

    def __init__(self, key, expected, qualifier=None):
        self.key = key
        self.expected = expected
        self.qualifier = qualifier

    def __repr__(self):
        return '<EqualAssertion %s == %s>' % (self.key, _py2_repr(self.expected))

    def Check(self, shell, record):
        actual = record[self.key]
        if not _py2_equal(actual, self.expected):
            msg = '\n[%s %s]\nExpected %s\nGot      %s\n' % (
                shell, self.key, _py2_repr(self.expected), _py2_repr(actual))
            return Result.FAIL, msg
        return QualifierToResult(self.qualifier), ''


class SubstringAssertion:

    def __init__(self, key, substring):
        self.key = key
        self.substring = substring

    def __repr__(self):
        return '<SubstringAssertion %s == %s>' % (self.key, _py2_repr(self.substring))

    def Check(self, shell, record):
        if self.substring in record[self.key]:
            return Result.FAIL, '[%s %s] Found %s' % (
                shell, self.key, _py2_repr(self.substring))
        return Result.PASS, ''


TEXT_CELLS = {
    Result.TIMEOUT: 'TIME',
    Result.FAIL: 'FAIL',
    Result.BUG: 'BUG',
    Result.BUG_2: 'BUG-2',
    Result.NI: 'N-I',
    Result.OK: 'ok',
    Result.OK_2: 'ok-2',
    Result.OK_3: 'ok-3',
    Result.OK_4: 'ok-4',
    Result.PASS: 'pass',
}


class Stats:

    def __init__(self, num_cases, sh_labels):
        self.counters = collections.defaultdict(int)
        c = self.counters
        c['num_cases'] = num_cases
        c['oils_num_passed'] = 0
        c['oils_num_failed'] = 0
        c['oils_cpp_num_failed'] = 0
        c['oils_ALT_delta'] = 0
        self.by_shell = {sh: collections.defaultdict(int) for sh in sh_labels}
        self.nonzero_results = collections.defaultdict(int)
        self.tsv_rows = []

    def Inc(self, counter_name):
        self.counters[counter_name] += 1

    def Get(self, counter_name):
        return self.counters[counter_name]

    def Set(self, counter_name, val):
        self.counters[counter_name] = val

    def ReportCell(self, case_num, cell_result, sh_label):
        self.tsv_rows.append((str(case_num), sh_label, TEXT_CELLS[cell_result]))
        self.by_shell[sh_label][cell_result] += 1
        self.nonzero_results[cell_result] += 1
        c = self.counters
        if cell_result == Result.TIMEOUT:
            c['num_timeout'] += 1
        elif cell_result == Result.FAIL:
            if sh_label not in OTHER_OILS:
                c['num_failed'] += 1
            if sh_label in OSH_CPYTHON + YSH_CPYTHON:
                c['oils_num_failed'] += 1
            if sh_label in ('osh-cpp', 'ysh-cpp'):
                c['oils_cpp_num_failed'] += 1
        elif cell_result in (Result.BUG, Result.BUG_2):
            c['num_bug'] += 1
        elif cell_result == Result.NI:
            c['num_ni'] += 1
        elif cell_result in (Result.OK, Result.OK_2, Result.OK_3, Result.OK_4):
            c['num_ok'] += 1
        elif cell_result == Result.PASS:
            c['num_passed'] += 1
            if sh_label in OSH_CPYTHON + YSH_CPYTHON:
                c['oils_num_passed'] += 1
        else:
            raise AssertionError()

    def WriteTsv(self, f):
        f.write('case\tshell\tresult\n')
        for row in self.tsv_rows:
            f.write('\t'.join(row))
            f.write('\n')


# ---------------------------------------------------------------------------
# Running cases
# ---------------------------------------------------------------------------


def _PrepareCaseTempDir(case_tmp_dir, legacy_tmp_dir=False):
    # The previous run's directory is cleaned here, not by that run, so its
    # state can be inspected afterwards -- upstream's reason.
    try:
        shutil.rmtree(case_tmp_dir)
    except OSError:
        pass
    try:
        os.makedirs(case_tmp_dir)
    except OSError as e:
        if e.errno != errno.EEXIST:
            raise
    if legacy_tmp_dir:
        try:
            os.mkdir(os.path.join(case_tmp_dir, '_tmp'))
        except OSError as e:
            if e.errno != errno.EEXIST:
                raise


def _run_shell(argv, env, cwd, code, timeout):
    """(stdout, stderr, status, timed_out) for one run of a case.

    Without a timeout the shell runs in the harness's own process group, as
    upstream's does. With one it gets a group of its own, as upstream's did
    under `timeout -s KILL`, which makes itself a group leader; on expiry the
    whole group is killed, so a background job holding the pipes open cannot
    wedge the run."""
    p = subprocess.Popen(argv, env=env, cwd=cwd, stdin=subprocess.PIPE,
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                         process_group=0 if timeout else None)
    try:
        out, err = p.communicate(code, timeout=timeout)
        return out, err, p.returncode, False
    except subprocess.TimeoutExpired:
        try:
            os.killpg(p.pid, signal.SIGKILL)
        except OSError:
            p.kill()
        out, err = p.communicate()
        return out, err, p.returncode, True


class TextOutput:
    """A plain table: case number, line, a result per shell, description."""

    def __init__(self, f, verbose):
        self.f = f
        self.verbose = verbose
        self.details = []

    def BeginCases(self, test_file):
        self.f.write('%s\n' % test_file)

    def WriteHeader(self, sh_labels):
        self.f.write('case\tline\t%s\n' % '\t'.join(sh_labels))

    def WriteRow(self, i, line_num, row, desc):
        cells = '\t'.join(TEXT_CELLS[r] for r in row)
        self.f.write('%3d\t%3d\t%s\t%s\n' % (i, line_num, cells, _text(desc)))
        self.f.flush()

    def AddDetails(self, entry):
        self.details.append(entry)

    def EndCases(self, sh_labels, stats):
        if self.verbose:
            for case_num, sh_label, stdout, stderr, messages in self.details:
                self.f.write('case %d (%s):\n' % (case_num, sh_label))
                for m in messages:
                    self.f.write('%s\n' % m)
                self.f.write('STDOUT:\n%s\nSTDERR:\n%s\n' % (_text(stdout), _text(stderr)))
        c = stats.counters
        self.f.write(
            '\n%d cases run: %d passed, %d ok, %d not implemented, %d bug, '
            '%d failed, %d timed out\n' % (
                c['num_cases_run'], c['num_passed'], c['num_ok'], c['num_ni'],
                c['num_bug'], c['num_failed'], c['num_timeout']))


def RunCases(cases, case_predicate, shells, env, out, opts, legacy_tmp_dir=False):
    if isinstance(case_predicate, RangePredicate) and case_predicate.begin > len(cases) - 1:
        raise RuntimeError(
            "valid case indexes are from 0 to %s. given range: %s-%s" %
            (len(cases) - 1, case_predicate.begin, case_predicate.end))

    sh_labels = [sh_label for sh_label, _ in shells]
    out.WriteHeader(sh_labels)
    stats = Stats(len(cases), sh_labels)

    sh_env = []
    for _, sh_path in shells:
        e = dict(env)
        e[opts.sh_env_var_name] = sh_path
        sh_env.append(e)

    osh_cpython_index = -1
    for i, (sh_label, _) in enumerate(shells):
        if sh_label in OSH_CPYTHON:
            osh_cpython_index = i
            break

    timeout = float(opts.timeout) if opts.timeout else None

    for i, case in enumerate(cases):
        case['case_num'] = i
        line_num = case['line_num']
        desc = case['desc']
        code = case['code']

        if opts.trace:
            log('case %d: %s', i, _text(desc))
        if not case_predicate(i, case):
            stats.Inc('num_skipped')
            continue
        if opts.do_print:
            sys.stdout.buffer.write(b'#### ' + desc + b'\n' + code + b'\n\n')
            continue

        stats.Inc('num_cases_run')
        result_row = []

        for shell_index, (sh_label, sh_path) in enumerate(shells):
            argv = [sh_path]
            if opts.posix and sh_label != 'dash':
                argv.extend(['-o', 'posix'])
            if opts.trace:
                log('\targv: %s', ' '.join(argv))

            case_env = sh_env[shell_index]
            tmp_base = os.path.normpath(opts.tmp_env)
            case_tmp_dir = os.path.join(tmp_base, '%02d-%s' % (i, sh_label))
            _PrepareCaseTempDir(case_tmp_dir, legacy_tmp_dir=legacy_tmp_dir)
            case_env['TMP'] = case_tmp_dir

            try:
                stdout, stderr, status, timed_out = _run_shell(
                    argv, case_env, case_tmp_dir, code, timeout)
            except OSError as e:
                print('Error running %r: %s' % (sh_path, e), file=sys.stderr)
                sys.exit(1)

            actual = {'sh_label': sh_label, 'stdout': stdout, 'stderr': stderr,
                      'status': status}

            if timed_out:
                cell_result = Result.TIMEOUT
            else:
                messages = []
                cell_result = Result.PASS
                for a in CreateAssertions(case, sh_label):
                    result, msg = a.Check(sh_label, actual)
                    cell_result = min(cell_result, result)
                    if msg:
                        messages.append(msg)
                if cell_result != Result.PASS or opts.details:
                    out.AddDetails((i, sh_label, stdout, stderr, messages))

            result_row.append(cell_result)
            stats.ReportCell(i, cell_result, sh_label)

            if sh_label in OTHER_OSH:
                if osh_cpython_index == -1:
                    raise RuntimeError("Couldn't determine index of osh-cpython")
                if result_row[shell_index] != result_row[osh_cpython_index]:
                    stats.Inc('oils_ALT_delta')

        out.WriteRow(i, line_num, result_row, desc)

    return stats


# ---------------------------------------------------------------------------
# main
# ---------------------------------------------------------------------------


def MakeTestEnv(opts):
    if not opts.tmp_env:
        raise RuntimeError('--tmp-env required')
    if not opts.path_env:
        raise RuntimeError('--path-env required')
    env = {'PATH': opts.path_env}
    for p in opts.env_pair:
        name, value = p.split('=', 1)
        env[name] = value
    return env


def _DefaultSuite(spec_name):
    if spec_name.startswith('ysh-') or spec_name.startswith('hay'):
        return 'ysh'
    if spec_name.startswith('tea-'):
        return 'tea'
    return 'osh'


def _SuccessOrFailure(test_name, stats):
    allowed = stats.Get('oils_failures_allowed')
    allowed_cpp = stats.Get('oils_cpp_failures_allowed')
    all_count = stats.Get('num_failed')
    oils_count = stats.Get('oils_num_failed')
    oils_cpp_count = stats.Get('oils_cpp_num_failed')

    errors = []
    if oils_count != allowed:
        errors.append('Got %d Oils failures, but %d are allowed' % (oils_count, allowed))
    elif allowed != 0:
        log('%s: note: Got %d allowed Oils failures', test_name, allowed)

    if oils_cpp_count != 0:
        if oils_cpp_count != allowed_cpp:
            errors.append('Got %d Oils C++ failures, but %d are allowed' %
                          (oils_cpp_count, allowed_cpp))
        elif allowed_cpp != 0:
            log('%s: note: Got %d allowed Oils C++ failures', test_name, allowed_cpp)

    if all_count != allowed:
        errors.append('Got %d total failures, but %d are allowed' % (all_count, allowed))

    if errors:
        for msg in errors:
            log('%s: FATAL: %s', test_name, msg)
        return 1
    return 0


def _parser():
    p = argparse.ArgumentParser(
        prog='sh_spec.py', usage='%(prog)s [options] TEST_FILE shell...')
    p.add_argument('-v', '--verbose', action='store_true')
    p.add_argument('-r', '--range', dest='range', default=None)
    p.add_argument('--regex', default=None)
    p.add_argument('--list', dest='do_list', action='store_true')
    p.add_argument('--oils-failures-allowed', dest='oils_failures_allowed',
                   type=int, default=0)
    p.add_argument('--oils-bin-dir', dest='oils_bin_dir', default=None)
    p.add_argument('--oils-cpp-bin-dir', dest='oils_cpp_bin_dir', default=None)
    p.add_argument('--ovm-bin-dir', dest='ovm_bin_dir', default=None)
    p.add_argument('--compare-shells', dest='compare_shells', action='store_true')
    p.add_argument('-d', '--details', action='store_true')
    p.add_argument('-t', '--trace', action='store_true')
    p.add_argument('-p', '--print', dest='do_print', action='store_true')
    p.add_argument('--print-spec-suite', dest='print_spec_suite', action='store_true')
    p.add_argument('--format', choices=['text'], default='text')
    p.add_argument('--tsv-output', dest='tsv_output', default=None)
    p.add_argument('--path-env', dest='path_env', default='')
    p.add_argument('--tmp-env', dest='tmp_env', default='')
    p.add_argument('--env-pair', dest='env_pair', default=[], action='append')
    p.add_argument('--timeout', default='')
    p.add_argument('--posix', action='store_true')
    p.add_argument('--sh-env-var-name', dest='sh_env_var_name', default='SH')
    p.add_argument('test_file')
    p.add_argument('shells', nargs='*')
    return p


def main(argv):
    # Upstream's guard: a harness started from a shell that exported these
    # would hand them to every case.
    for name in ('RANDOM', 'PPID'):
        v = os.getenv(name)
        if v is not None:
            raise AssertionError('got $%s = %s' % (name, v))

    opts = _parser().parse_args(argv[1:])
    test_file = opts.test_file

    with open(test_file, 'rb') as f:
        file_metadata, cases = ParseTestFile(test_file, Tokenizer(f))
    meta = {k: v.decode('utf-8') for k, v in file_metadata.items()}

    if opts.do_list:
        for i, case in enumerate(cases):
            print('%d\t%s' % (i, _text(case['desc'])))
        return 0

    if opts.print_spec_suite:
        spec_name = os.path.basename(test_file).split('.')[0]
        print(meta.get('suite') or _DefaultSuite(spec_name))
        return 0

    if opts.oils_bin_dir:
        shells = []
        if opts.compare_shells:
            comp = meta.get('compare_shells')
            shells.extend(comp.split() if comp else [])
        our_shell = meta.get('our_shell', 'osh')
        if our_shell != '-':
            shells.append(os.path.join(opts.oils_bin_dir, our_shell))
            if opts.ovm_bin_dir:
                shells.append(os.path.join(opts.ovm_bin_dir, our_shell))
            if opts.oils_cpp_bin_dir:
                shells.append(os.path.join(opts.oils_cpp_bin_dir, our_shell))
        opts.oils_failures_allowed = int(meta.get('oils_failures_allowed', 0))
    else:
        shells = opts.shells

    shell_pairs = MakeShellPairs(shells)

    if opts.range:
        begin, end = ParseRange(opts.range)
        case_predicate = RangePredicate(begin, end)
    elif opts.regex:
        case_predicate = RegexPredicate(re.compile(opts.regex, re.IGNORECASE))
    else:
        case_predicate = lambda i, case: True

    out = TextOutput(sys.stderr if opts.do_print else sys.stdout, opts.verbose)
    out.BeginCases(os.path.basename(test_file))
    stats = RunCases(cases, case_predicate, shell_pairs, MakeTestEnv(opts), out,
                     opts, legacy_tmp_dir=bool(meta.get('legacy_tmp_dir')))
    out.EndCases([label for label, _ in shell_pairs], stats)

    if opts.tsv_output:
        with open(opts.tsv_output, 'w', encoding='utf-8', newline='\n') as f:
            stats.WriteTsv(f)

    stats.Set('oils_failures_allowed', opts.oils_failures_allowed)
    stats.Set('oils_cpp_failures_allowed',
              int(meta.get('oils_cpp_failures_allowed', opts.oils_failures_allowed)))
    test_name = os.path.basename(test_file).split('.')[0]
    return _SuccessOrFailure(test_name, stats)


if __name__ == '__main__':
    try:
        sys.exit(main(sys.argv))
    except KeyboardInterrupt:
        print('%s: interrupted with Ctrl-C' % sys.argv[0], file=sys.stderr)
        sys.exit(1)
