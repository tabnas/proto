#!/usr/bin/env python3
"""Check the corpus, or a fixture file, against protoc's own parser.

`oracle` (built from oracle/) is compiler::Parser itself, so this checks
the parser's output where crosscheck.py can only check the protoc binary,
which also resolves names and validates.

  corpus mode, every lane of test/protobuf-suite/:
    valid        the parser accepts the source and produces the golden;
    accept-only  the parser accepts the source;
    invalid      the parser reports errors, and the upstream diagnostic is
                 among them. Upstream runs a few of these with a required
                 syntax identifier or a validation-error collector, which
                 this default run does not reproduce; they are listed.

  fixture mode (--spec <file.tsv>): every row's input goes through the
    parser, and the row's expected descriptor must equal the parser's once
    protoc's `uninterpretedOption` list is bridged to this package's
    `{ name: value }` options map, as the conformance runners bridge it.
    A string value that is not UTF-8 bridges with U+FFFD for each
    ill-formed sequence, as this package records it. An ERROR row must be
    refused by the parser; one the parser accepts is listed apart, since
    a fixture may pin text that text format refuses when protoc reads the
    option (test/spec/aggregate.tsv says which). test/spec/aggregate.tsv
    and test/spec/adjacent-strings.tsv are the files this is for.

Each lane goes through the parser in one run of the oracle; nothing here
takes long enough to need progress lines.

usage: oracle-check.py <oracle> <suite-dir>
       oracle-check.py <oracle> --spec <file.tsv>
  Run it with a Python that has the protobuf package matching the release.
"""
import base64, json, os, subprocess, sys, tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import lanes  # noqa: E402  (the normalisation valid.json was written with)


def run(oracle, inputs):
    d = tempfile.mkdtemp()
    paths = []
    for i, text in enumerate(inputs):
        p = os.path.join(d, '%04d.proto' % i)
        with open(p, 'w', encoding='utf-8', newline='') as f:
            f.write(text)
        paths.append(p)
    out = subprocess.run([oracle] + paths, capture_output=True, text=True, check=True)
    return [json.loads(line) for line in out.stdout.splitlines()]


def accepted(r):
    return r['ok'] and r['end'] and '' == r['errors']


def corpus(oracle, suite):
    def load(name):
        return json.load(open(os.path.join(suite, name), encoding='utf-8'))

    valid = load('valid.json')
    same = 0
    for c, r in zip(valid, run(oracle, [c['input'] for c in valid])):
        got = lanes.ints(lanes.sort_keys(r['descriptor']))
        if accepted(r) and json.dumps(got, sort_keys=True) == json.dumps(c['expected'], sort_keys=True):
            same += 1
        else:
            print('  valid differs:', c['name'], r['errors'].strip())
    print('valid: the parser produces the golden for %d/%d' % (same, len(valid)))

    acc = load('accept-only.json')
    ok = 0
    for c, r in zip(acc, run(oracle, [c['input'] for c in acc])):
        if accepted(r):
            ok += 1
        else:
            print('  accept-only refused:', c['name'], r['errors'].strip())
    print('accept-only: the parser accepts %d/%d' % (ok, len(acc)))

    inv = load('invalid.json')
    res = run(oracle, [c['input'] for c in inv])
    refused = sum(1 for r in res if r['errors'])
    found = 0
    for c, r in zip(inv, res):
        if c['error'] in r['errors']:
            found += 1
        else:
            print('  invalid differs:', c['name'], '| upstream %r | parser %r'
                  % (c['error'], r['errors'][:160]))
    print('invalid: the parser reports errors for %d/%d; the upstream diagnostic is among them '
          'for %d/%d' % (refused, len(inv), found, len(inv)))


def unescape(s):
    out = []
    i = 0
    while i < len(s):
        if '\\' == s[i] and i + 1 < len(s) and s[i + 1] in 'nrt\\':
            out.append({'n': '\n', 'r': '\r', 't': '\t', '\\': '\\'}[s[i + 1]])
            i += 2
        else:
            out.append(s[i])
            i += 1
    return ''.join(out)


def option_name(parts):
    return '.'.join('(' + p['namePart'] + ')' if p.get('isExtension') else p['namePart']
                    for p in parts)


def option_value(u):
    if 'stringValue' in u:
        return base64.b64decode(u['stringValue']).decode('utf-8', 'replace')
    if 'positiveIntValue' in u:
        return int(u['positiveIntValue'])
    if 'negativeIntValue' in u:
        return int(u['negativeIntValue'])
    if 'doubleValue' in u:
        return u['doubleValue']
    if 'aggregateValue' in u:
        return u['aggregateValue']
    ident = u.get('identifierValue')
    return {'true': True, 'false': False}.get(ident, ident)


def bridge(v):
    if isinstance(v, list):
        return [bridge(x) for x in v]
    if isinstance(v, dict):
        out = {}
        for k, x in v.items():
            if 'options' == k and isinstance(x, dict) and 'uninterpretedOption' in x:
                m = {kk: bridge(vv) for kk, vv in x.items() if 'uninterpretedOption' != kk}
                for u in x['uninterpretedOption']:
                    m[option_name(u['name'])] = option_value(u)
                out[k] = m
            else:
                out[k] = bridge(x)
        return out
    return v


def norm(v):
    if isinstance(v, list):
        return [norm(x) for x in v]
    if isinstance(v, dict):
        out = {}
        for k in sorted(v):
            x = norm(v[k])
            if x is None or [] == x:
                continue
            out[k] = x
        return out
    return v


def spec(oracle, path):
    rows = []
    with open(path, encoding='utf-8', newline='') as f:
        lines = f.read().split('\n')
    for line in lines[1:]:
        if '' == line or (line.startswith('#') and '\t' not in line):
            continue
        cells = line.split('\t')
        expected = None if cells[1].startswith('ERROR') else json.loads(cells[1])
        rows.append((unescape(cells[0]), expected))
    same = 0
    refused = 0
    errors = 0
    for (text, expected), r in zip(rows, run(oracle, [t for t, _ in rows])):
        if expected is None:
            errors += 1
            if accepted(r):
                print('  error row the parser accepts:', json.dumps(text)[:80])
            else:
                refused += 1
            continue
        want = norm(bridge(r['descriptor']))
        got = norm(expected)
        # protoc leaves `syntax` unset for a file that declares none.
        if 'syntax' not in want and 'proto2' == got.get('syntax'):
            del got['syntax']
        if accepted(r) and got == want:
            same += 1
        else:
            print('  row differs:', json.dumps(text)[:80], r['errors'].strip())
    print('%s: %d/%d rows equal the parser\'s descriptor; the parser refuses %d/%d error rows'
          % (os.path.basename(path), same, len(rows) - errors, refused, errors))


if '--spec' == sys.argv[2]:
    spec(sys.argv[1], sys.argv[3])
else:
    corpus(sys.argv[1], sys.argv[2])
