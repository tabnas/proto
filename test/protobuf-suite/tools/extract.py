#!/usr/bin/env python3
"""Extract raw.json from upstream protobuf's
src/google/protobuf/compiler/parser_unittest.cc.

Every call to one of the five ParserTest helpers inside a TEST_F body is a
case. A call with an argument that is not a string literal (or
HasSubstr(<literal>)) becomes an `excluded` row, with the reason. An
ExpectParsesTo golden is kept as the text-format FileDescriptorProto
upstream wrote; lanes.py converts it to JSON.

usage: extract.py <parser_unittest.cc> > raw.json
"""
import json, re, sys

HELPERS = ['ExpectParsesTo', 'ExpectHasWarnings', 'ExpectHasErrors',
           'ExpectHasEarlyExitErrors', 'ExpectHasValidationErrors']

NOTES = {
    'ExpectHasWarnings': 'upstream ExpectHasWarnings: asserts the parser reported NO error and only a warning; publishes no descriptor golden. Accept-only.',
    'ExpectHasValidationErrors': 'upstream ExpectHasValidationErrors: asserts protoc\'s PARSER accepted it (ASSERT_EQ("", error_collector_.text_)); it fails only later, in DescriptorPool validation, which this package does not perform. Must therefore be ACCEPTED.',
}


def skip_ws_comments(s, i):
    n = len(s)
    while i < n:
        if s[i].isspace():
            i += 1
        elif s.startswith('//', i):
            j = s.find('\n', i)
            i = n if j < 0 else j + 1
        elif s.startswith('/*', i):
            i = s.index('*/', i) + 2
        else:
            break
    return i


RAW_RE = re.compile(r'(?:u8|u|U|L)?R"([^()\\ ]{0,16})\(')


def read_literal(s, i):
    """Read one C++ string literal at s[i:]; return (value, end) or None."""
    m = RAW_RE.match(s, i)
    if m:
        delim = m.group(1)
        start = m.end()
        end = s.index(')' + delim + '"', start)
        return s[start:end], end + len(delim) + 2
    j = i
    if s.startswith('u8"', j):
        j += 2
    if j >= len(s) or s[j] != '"':
        return None
    j += 1
    out = []
    while s[j] != '"':
        c = s[j]
        if c == '\\':
            e = s[j + 1]
            simple = {'n': '\n', 't': '\t', 'r': '\r', '"': '"', "'": "'",
                      '\\': '\\', '?': '?', 'a': '\a', 'b': '\b', 'f': '\f',
                      'v': '\v'}
            if e in simple:
                out.append(simple[e]); j += 2
            elif e in '01234567':
                k = j + 1
                while k < j + 4 and s[k] in '01234567':
                    k += 1
                out.append(chr(int(s[j + 1:k], 8))); j = k
            elif e == 'x':
                k = j + 2
                while k < len(s) and s[k] in '0123456789abcdefABCDEF':
                    k += 1
                out.append(chr(int(s[j + 2:k], 16))); j = k
            else:
                raise ValueError('escape %r at %d' % (e, j))
        else:
            out.append(c); j += 1
    return ''.join(out), j + 1


def literal_arg(arg):
    """Concatenated string-literal value of an argument, or None."""
    a = arg.strip()
    m = re.match(r'^(?:::)?(?:testing::)?HasSubstr\s*\((.*)\)$', a, re.S)
    if m:
        a = m.group(1)
    i = skip_ws_comments(a, 0)
    parts = []
    while i < len(a):
        r = read_literal(a, i)
        if r is None:
            return None
        parts.append(r[0])
        i = skip_ws_comments(a, r[1])
    return ''.join(parts) if parts else None


def scan_to(s, i, closers):
    """Advance from i over balanced text, stopping at a top-level char in
    closers. Skips strings, chars, comments."""
    depth = 0
    n = len(s)
    while i < n:
        if s.startswith('//', i) or s.startswith('/*', i):
            i = skip_ws_comments(s, i)
            continue
        c = s[i]
        if RAW_RE.match(s, i) or c == '"':
            r = read_literal(s, i)
            if r is not None:
                i = r[1]
                continue
        if c == "'":
            j = i + 1
            while s[j] != "'":
                j += 2 if s[j] == '\\' else 1
            i = j + 1
            continue
        if depth == 0 and c in closers:
            return i
        if c in '([{':
            depth += 1
        elif c in ')]}':
            depth -= 1
        i += 1
    raise ValueError('unbalanced')


def tests(src):
    for m in re.finditer(r'^TEST_F\(\s*(\w+)\s*,\s*(\w+)\s*\)\s*\{', src, re.M):
        start = m.end()
        end = scan_to(src, start, '}')
        yield m.group(1) + '.' + m.group(2), src[start:end]


def calls(body, helper):
    for m in re.finditer(r'\b' + helper + r'\s*\(', body):
        i = m.end()
        args = []
        while True:
            j = scan_to(body, i, ',)')
            args.append(body[i:j])
            if body[j] == ')':
                break
            i = j + 1
        yield args


def main():
    src = open(sys.argv[1], encoding='utf-8').read()
    out = {'valid': [], 'invalid': [], 'acceptOnly': [], 'excluded': [],
           'byHelper': {}}
    all_tests = list(tests(src))
    for helper in HELPERS:
        count = 0
        for name, body in all_tests:
            for args in calls(body, helper):
                lane = ('valid' if helper == 'ExpectParsesTo' else
                        'acceptOnly' if helper in NOTES else 'invalid')
                vals = [literal_arg(a) for a in args]
                if vals[0] is None or vals[1] is None:
                    reason = ('input is computed in C++ at run time, not written as a string literal'
                              if vals[0] is None else
                              'expected output is built in C++ at run time, not a string literal'
                              if helper == 'ExpectParsesTo' else
                              'expected diagnostic is built in C++ at run time, not a string literal')
                    out['excluded'].append({'helper': helper, 'case': name,
                                            'lane': 'invalid' if lane == 'invalid' else lane,
                                            'reason': reason})
                    continue
                count += 1
                row = {'name': name, 'helper': helper, 'input': vals[0]}
                if helper == 'ExpectParsesTo':
                    row['expectedText'] = vals[1]
                else:
                    row['error'] = vals[1]
                    if helper in NOTES:
                        row['note'] = NOTES[helper]
                out[lane].append(row)
        out['byHelper'][helper] = count
    return out


if __name__ == '__main__':
    json.dump(main(), sys.stdout, indent=1)
