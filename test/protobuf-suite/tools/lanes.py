#!/usr/bin/env python3
"""Split raw.json into the lane files, converting each ExpectParsesTo golden
(text-format FileDescriptorProto) to proto3 JSON with sorted keys.

The lane files are written in the committed format: indent 1, and `<`, `>`
and `&` escaped as \\u003c / \\u003e / \\u0026, with an integral double
written as an integer (18446744073709552000), as Go's encoding/json does.

usage: lanes.py <raw.json> <outdir>
"""
import json, math, os, sys
from decimal import Decimal
from google.protobuf import descriptor_pb2, text_format, json_format


def sort_keys(v):
    if isinstance(v, dict):
        return {k: sort_keys(v[k]) for k in sorted(v)}
    if isinstance(v, list):
        return [sort_keys(x) for x in v]
    return v


def golden(text):
    fdp = descriptor_pb2.FileDescriptorProto()
    text_format.Parse(text, fdp)
    return sort_keys(json_format.MessageToDict(fdp))


def ints(v):
    if isinstance(v, dict):
        return {k: ints(x) for k, x in v.items()}
    if isinstance(v, list):
        return [ints(x) for x in v]
    if isinstance(v, float) and math.isfinite(v) and v == int(v) and abs(v) < 1e21:
        # shortest round-trip digits, positional (Go/JS style)
        return int(Decimal(repr(v)))
    return v


def dump(v, path):
    s = json.dumps(ints(v), indent=1, ensure_ascii=False)
    s = s.replace('<', '\\u003c').replace('>', '\\u003e').replace('&', '\\u0026')
    s = s.replace('\u2028', '\\u2028').replace('\u2029', '\\u2029')
    with open(path, 'w', encoding='utf-8') as f:
        f.write(s + '\n')


def main():
    raw = json.load(open(sys.argv[1], encoding='utf-8'))
    out = sys.argv[2]
    valid = []
    for c in raw['valid']:
        valid.append({'name': c['name'], 'helper': c['helper'],
                      'input': c['input'], 'expected': golden(c['expectedText'])})
    dump(valid, os.path.join(out, 'valid.json'))
    dump(raw['invalid'], os.path.join(out, 'invalid.json'))
    dump(raw['acceptOnly'], os.path.join(out, 'accept-only.json'))
    dump([sort_keys(x) for x in raw['excluded']], os.path.join(out, 'excluded.json'))


if __name__ == '__main__':
    main()
