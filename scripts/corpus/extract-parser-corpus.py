#!/usr/bin/env python3
"""Extract the .proto parser conformance corpus from protobuf's own
`compiler/parser_unittest.cc`.

This tool is OURS; the corpus it produces is third-party and is never committed
(see scripts/fetch-protobuf-corpus.sh).

It scrapes the C++ string literals out of every `Expect*()` call site and sorts
them into lanes by which upstream helper asserts them:

  ExpectParsesTo(in, out)          -> valid       : parses AND equals `out`
  ExpectHasErrors(in, ...)         -> invalid     : must be REJECTED
  ExpectHasEarlyExitErrors(in,...) -> invalid     : must be REJECTED
  ExpectHasWarnings(in, ...)       -> acceptOnly  : parses clean; no golden
  ExpectHasValidationErrors(in,..) -> acceptOnly  : protoc's PARSER accepts it;
                                                    only DescriptorPool rejects

The acceptOnly split is load-bearing and is taken from the helper bodies, not
guessed: both ExpectHasWarnings and ExpectHasValidationErrors execute
`ASSERT_EQ("", error_collector_.text_)` after Parse(), i.e. they assert the
parser reported NO error. @tabnas/proto is a parser and does no semantic
validation, so accepting them is the correct behaviour and rejecting them is a
failure. Upstream publishes no descriptor golden for either, so that lane can
only assert accept/reject - it is reported separately and never folded into the
headline valid figure.

Every case it cannot extract is recorded, with a reason, in excluded.json, and
printed. A skip must always have a MECHANICAL reason (the input is computed in
C++ at run time rather than written as a string literal). Never add to it to
make a number look better.
"""
import json
import re
import sys

SRC = sys.argv[1]
OUT = sys.argv[2]
text = open(SRC, encoding='utf-8').read()

# ---- C++ literal scanning ---------------------------------------------------

ESCAPES = {'n': '\n', 't': '\t', 'r': '\r', '0': '\0', '\\': '\\',
           '"': '"', "'": "'", 'a': '\a', 'b': '\b', 'f': '\f', 'v': '\v', '?': '?'}


def scan_literal(s, i):
    """Scan one C++ string literal starting at s[i] ('"' or R"delim( ).
    Returns (value, next_index) or None."""
    m = re.match(r'R"([^(]*)\(', s[i:])
    if m:
        delim = m.group(1)
        start = i + m.end()
        close = ')' + delim + '"'
        end = s.index(close, start)
        return s[start:end], end + len(close)
    if s[i] != '"':
        return None
    j = i + 1
    out = []
    while j < len(s):
        c = s[j]
        if c == '\\':
            nxt = s[j + 1]
            if nxt == 'x':
                k = j + 2
                hx = ''
                while k < len(s) and s[k] in '0123456789abcdefABCDEF' and len(hx) < 2:
                    hx += s[k]
                    k += 1
                out.append(chr(int(hx, 16)))
                j = k
                continue
            if nxt in '01234567':
                k = j + 1
                oc = ''
                while k < len(s) and s[k] in '01234567' and len(oc) < 3:
                    oc += s[k]
                    k += 1
                out.append(chr(int(oc, 8)))
                j = k
                continue
            out.append(ESCAPES.get(nxt, nxt))
            j += 2
            continue
        if c == '"':
            return ''.join(out), j + 1
        out.append(c)
        j += 1
    raise ValueError('unterminated literal at %d' % i)


def skip_ws(s, i):
    """Advance past whitespace and comments."""
    while i < len(s):
        if s[i] in ' \t\n\r':
            i += 1
        elif s.startswith('//', i):
            i = s.index('\n', i) + 1
        elif s.startswith('/*', i):
            i = s.index('*/', i) + 2
        else:
            break
    return i


def read_concat_literal(s, i):
    """Read one or more adjacent string literals -> (value, next_index) or None."""
    i = skip_ws(s, i)
    r = scan_literal(s, i)
    if r is None:
        return None
    val, i = r
    while True:
        j = skip_ws(s, i)
        nxt = scan_literal(s, j)
        if nxt is None:
            return val, i
        val += nxt[0]
        i = nxt[1]


_TESTS = [(m.start(), m.group(1) + '.' + m.group(2))
          for m in re.finditer(r'TEST_F\(\s*(\w+)\s*,\s*(\w+)\s*\)', text)]


def enclosing_test(pos):
    """Name of the TEST_F(...) block containing character offset pos, or None
    when pos is above the first one (that is the helper DEFINITION, not a case)."""
    best = None
    for start, name in _TESTS:
        if start < pos:
            best = name
        else:
            break
    return best


def calls(fnname):
    """Yield (test_name_or_None, args_start_index) for each `fnname(` call site."""
    for m in re.finditer(r'\b' + fnname + r'\(', text):
        yield enclosing_test(m.start()), m.end()


def second_arg_string(i):
    """After the first arg + comma, read the expected-error argument. It is
    either a string literal or testing::HasSubstr("...")."""
    i = skip_ws(text, i)
    if text[i] != ',':
        return None
    i = skip_ws(text, i + 1)
    m = re.match(r'(?:testing::)?HasSubstr\(', text[i:])
    if m:
        i += m.end()
    r = read_concat_literal(text, i)
    return r[0] if r else None


ACCEPT_ONLY_NOTE = {
    'ExpectHasWarnings':
        'upstream ExpectHasWarnings: asserts the parser reported NO error and '
        'only a warning; publishes no descriptor golden. Accept-only.',
    'ExpectHasValidationErrors':
        "upstream ExpectHasValidationErrors: asserts protoc's PARSER accepted "
        'it (ASSERT_EQ("", error_collector_.text_)); it fails only later, in '
        'DescriptorPool validation, which this package does not perform. '
        'Must therefore be ACCEPTED.',
}

valid, invalid, accept_only = [], [], []
excluded = []
counts = {}

LANES = (
    ('ExpectParsesTo', 'valid'),
    ('ExpectHasWarnings', 'acceptOnly'),
    ('ExpectHasErrors', 'invalid'),
    ('ExpectHasEarlyExitErrors', 'invalid'),
    ('ExpectHasValidationErrors', 'acceptOnly'),
)

for fn, lane in LANES:
    n = 0
    for name, i in calls(fn):
        if name is None:
            continue  # the helper's own definition, above the first TEST_F
        r = read_concat_literal(text, i)
        if r is None:
            excluded.append({'helper': fn, 'case': name, 'lane': lane,
                             'reason': 'input is computed in C++ at run time, '
                                       'not written as a string literal'})
            continue
        src, j = r
        n += 1
        if lane == 'valid':
            k = skip_ws(text, j)
            if text[k] != ',':
                n -= 1
                excluded.append({'helper': fn, 'case': name, 'lane': lane,
                                 'reason': 'no second (expected-output) argument'})
                continue
            r2 = read_concat_literal(text, k + 1)
            if r2 is None:
                n -= 1
                excluded.append({'helper': fn, 'case': name, 'lane': lane,
                                 'reason': 'expected output is built in C++ at '
                                           'run time, not a string literal'})
                continue
            valid.append({'name': name, 'helper': fn, 'input': src,
                          'expectedText': r2[0]})
        elif lane == 'invalid':
            invalid.append({'name': name, 'helper': fn, 'input': src,
                            'error': second_arg_string(j)})
        else:
            accept_only.append({'name': name, 'helper': fn, 'input': src,
                                'error': second_arg_string(j),
                                'note': ACCEPT_ONLY_NOTE[fn]})
    counts[fn] = n

json.dump({'valid': valid, 'invalid': invalid, 'acceptOnly': accept_only,
           'excluded': excluded, 'byHelper': counts},
          open(OUT, 'w'), indent=1)

print('extract: valid=%d invalid=%d acceptOnly=%d excluded=%d'
      % (len(valid), len(invalid), len(accept_only), len(excluded)))
for s in excluded:
    print('  EXCLUDED %-28s %s  (%s)' % (s['helper'], s['case'], s['reason']))
