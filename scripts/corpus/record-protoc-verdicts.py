#!/usr/bin/env python3
"""Run the real protoc over each leniency probe and record its verdict.

  usage: record-protoc-verdicts.py <protoc> <probes.json> <out.json>

The probe INPUTS are ours (test/leniency-probes.json, committed). The VERDICTS
are protoc's, obtained by running it — never asserted from memory and never
hand-written. The output is written into the gitignored corpus directory.

protoc is invoked as a pure front end: --descriptor_set_out to a throwaway file
with no code generator, so what is measured is exactly "does protoc accept this
.proto source".
"""
import json
import os
import subprocess
import sys
import tempfile

protoc, probes_path, out_path = sys.argv[1], sys.argv[2], sys.argv[3]

probes = json.load(open(probes_path, encoding='utf-8'))
version = subprocess.run([protoc, '--version'], capture_output=True, text=True,
                         check=True).stdout.strip()

results = []
with tempfile.TemporaryDirectory() as tmp:
    for p in probes:
        src = os.path.join(tmp, 'probe.proto')
        with open(src, 'w', encoding='utf-8') as f:
            f.write(p['input'])
        r = subprocess.run(
            [protoc, '--proto_path=' + tmp,
             '--descriptor_set_out=' + os.path.join(tmp, 'out.pb'), src],
            capture_output=True, text=True)
        results.append({
            'name': p['name'],
            'input': p['input'],
            'why': p.get('why', ''),
            'accepted': r.returncode == 0,
            'protoc': (r.stderr or '').strip(),
        })

json.dump({'protocVersion': version, 'probes': results},
          open(out_path, 'w'), indent=1)

acc = sum(1 for r in results if r['accepted'])
print('protoc verdicts (%s): %d accepted, %d rejected, of %d probes'
      % (version, acc, len(results) - acc, len(results)))
for r in results:
    print('  %-34s %s' % (r['name'], 'ACCEPT' if r['accepted'] else 'REJECT'))
