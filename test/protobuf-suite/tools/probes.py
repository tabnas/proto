#!/usr/bin/env python3
"""Re-measure leniency.json's `accepted` and `protoc` fields with a protoc
release binary.

Each probe is compiled on its own; SAME means protoc's answer (accepted or
not) and its message both match what leniency.json records. The recorded
messages carry the directory of the run that wrote them, so that directory
is read back out of them and put in place of this run's before comparing.

usage: probes.py <protoc> <leniency.json>
"""
import json, os, re, subprocess, sys, tempfile

protoc, path = sys.argv[1], sys.argv[2]
probes = json.load(open(path, encoding='utf-8'))['probes']
recorded_dir = next((m.group(1) for m in (re.match(r'^(.*)/probe\.proto:', p['protoc'])
                                           for p in probes) if m), '')
d = tempfile.mkdtemp()
print(subprocess.run([protoc, '--version'], capture_output=True, text=True).stdout.strip())
same = 0
for p in probes:
    f = os.path.join(d, 'probe.proto')
    with open(f, 'w', encoding='utf-8', newline='') as h:
        h.write(p['input'])
    r = subprocess.run([protoc, '-I', d, '--descriptor_set_out=' + os.devnull, f],
                       capture_output=True, text=True)
    accepted = r.returncode == 0
    message = r.stderr.strip().replace(d, recorded_dir)
    ok = accepted == p['accepted'] and message == p['protoc']
    same += ok
    print(('SAME ' if ok else 'DIFF ') + p['name'], accepted,
          '' if ok else '| now %r | recorded %r' % (message, p['protoc']))
print('leniency: %d/%d probes as recorded' % (same, len(probes)))
