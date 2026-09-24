#!/usr/bin/env python3
"""Cross-check an extracted corpus against a protoc release binary.

1. valid: every ExpectParsesTo golden is parsed by protoc's own C++
   TextFormat (`protoc --encode=google.protobuf.FileDescriptorProto`), the
   binary decoded, and compared with the JSON golden in valid.json.
2. invalid: every source is compiled with protoc. It should be refused,
   and each upstream diagnostic (0-based `L:C: msg`) should appear in
   protoc's output (1-based `file:L+1:C+1: msg`).
3. accept-only / valid: protoc's exit status, for information only:
   protoc also resolves names and validates, which the parser tests do not.

A progress line goes to stderr every 25 protoc runs.

usage: crosscheck.py <protoc-root> <raw.json> <valid.json>
  <protoc-root> is the unzipped protoc-<version>-<platform>.zip (bin/, include/).
"""
import json, os, re, subprocess, sys, tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import lanes  # noqa: E402  (the same normalisation valid.json was written with)

from google.protobuf import descriptor_pb2, json_format  # noqa: E402

root, rawp, validp = sys.argv[1:4]
protoc = os.path.join(root, 'bin', 'protoc')
inc = os.path.join(root, 'include')
raw = json.load(open(rawp, encoding='utf-8'))
valid = json.load(open(validp, encoding='utf-8'))


def progress(step, done, total):
    if done % 25 == 0 or done == total:
        print('%s: %d/%d (%d%%)' % (step, done, total, 100 * done // total),
              file=sys.stderr, flush=True)


print(subprocess.run([protoc, '--version'], capture_output=True, text=True).stdout.strip())

bad = 0
for i, (r, v) in enumerate(zip(raw['valid'], valid), 1):
    assert r['name'] == v['name']
    p = subprocess.run([protoc, '-I', inc, '--encode=google.protobuf.FileDescriptorProto',
                        'google/protobuf/descriptor.proto'],
                       input=r['expectedText'].encode(), capture_output=True)
    progress('valid goldens', i, len(valid))
    if p.returncode != 0:
        print('GOLDEN-REJECTED', r['name'], p.stderr.decode()[:300])
        bad += 1
        continue
    fdp = descriptor_pb2.FileDescriptorProto.FromString(p.stdout)
    got = lanes.ints(lanes.sort_keys(json_format.MessageToDict(fdp)))
    if json.dumps(got, sort_keys=True) != json.dumps(v['expected'], sort_keys=True):
        print('GOLDEN-MISMATCH', r['name'])
        bad += 1
print('valid goldens via protoc TextFormat: %d/%d agree' % (len(valid) - bad, len(valid)))


def compile_(src):
    d = tempfile.mkdtemp()
    f = os.path.join(d, 'foo.proto')
    with open(f, 'w', encoding='utf-8', newline='') as h:
        h.write(src)
    p = subprocess.run([protoc, '-I', d, '-I', inc, '--descriptor_set_out=' + os.devnull, f],
                       capture_output=True, text=True)
    return p.returncode, p.stderr.replace(f, 'foo.proto').replace(d + '/', '')


rej = 0
found = 0
total_lines = 0
missing = []
for i, c in enumerate(raw['invalid'], 1):
    rc, err = compile_(c['input'])
    progress('invalid', i, len(raw['invalid']))
    if rc != 0:
        rej += 1
    for line in c['error'].splitlines():
        m = re.match(r'^(-?\d+):(-?\d+): (.*)$', line)
        if not m:
            continue
        total_lines += 1
        want = 'foo.proto:%d:%d: %s' % (int(m.group(1)) + 1, int(m.group(2)) + 1, m.group(3))
        if want in err:
            found += 1
        else:
            missing.append((c['name'], line, err.strip().splitlines()[:3]))
print('invalid: protoc refuses %d/%d; upstream diagnostic lines reproduced by protoc %d/%d'
      % (rej, len(raw['invalid']), found, total_lines))
for m in missing:
    print('  not reproduced:', m[0], '|', m[1], '| protoc:', m[2])

for lane, cases in (('accept-only', raw['acceptOnly']), ('valid', raw['valid'])):
    acc = 0
    for i, c in enumerate(cases, 1):
        rc, _ = compile_(c['input'])
        acc += rc == 0
        progress(lane + ' (exit status)', i, len(cases))
    print('%s: protoc (parse, resolve and validate) exits 0 on %d/%d' % (lane, acc, len(cases)))
