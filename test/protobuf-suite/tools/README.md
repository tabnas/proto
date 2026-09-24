# Refreshing the protoc corpus

The JSON under `test/protobuf-suite/` is extracted from protoc's own parser
unit test, `src/google/protobuf/compiler/parser_unittest.cc`, at a protobuf
release tag, and cross-checked against that release. These scripts are how.
Run on their own inputs they reproduce the committed corpus byte for byte:
v35.1 (upstream commit `35cd01f9`) with PyPI `protobuf` 7.35.1, and v36.2
(`2c74169b`) with 7.36.2.

The corpus tracks upstream's latest release, so a refresh starts by reading
what that is:

```bash
git ls-remote --tags https://github.com/protocolbuffers/protobuf 'refs/tags/v*' \
  | awk '{print $2}' | grep -E '^refs/tags/v[0-9]+\.[0-9]+(\.[0-9]+)?$' | sort -V | tail -1
```

| File | What it does |
|---|---|
| `extract.py` | `parser_unittest.cc` -> `raw.json`: every call to a `ParserTest` helper, by lane, with an `excluded` row (and its reason) for a call whose input or golden is built in C++ at run time |
| `lanes.py` | `raw.json` -> `valid.json`, `invalid.json`, `accept-only.json`, `excluded.json`, each golden converted from text format to sorted JSON |
| `crosscheck.py` | the protoc release binary: every golden through protoc's own TextFormat, the `invalid` lane through protoc, exit statuses for information |
| `probes.py` | re-measures `leniency.json`'s recorded protoc answers |
| `oracle/` | protoc's own `compiler::Parser` as a command-line tool, built from the release source |
| `oracle-check.py` | every lane, or a fixture file such as `test/spec/aggregate.tsv`, against that parser |
| `suite-tsv.js` | regenerates `test/spec/protobuf-suite.tsv` from `valid.json` |

## What you need

- The release tag, say `v36.2`.
- A throwaway Python virtual environment holding the PyPI `protobuf`
  package of the SAME release: protoc 36.2 pairs with `protobuf==7.36.2`,
  35.1 with `7.35.1`. `lanes.py` converts goldens with that package's text
  format parser and JSON printer, and another version can print another
  answer.
- For the cross-checks, the protoc release archive for your platform
  (`protoc-36.2-linux-x86_64.zip` from the release page), its checksum
  verified.
- For the parser-level check, a C++17 compiler, CMake and Ninja.

Nothing here is committed besides the scripts and their JSON output:
`.gitignore` keeps `test/protobuf-suite/parser_unittest.cc`,
`test/protobuf-suite/protoc/` and the archive out.

## Steps

Run from the repository root. `$T` is the tag and `$PY` the environment's
Python.

1. **Extract and split.**

   ```bash
   T=v36.2
   python3 -m venv /tmp/pb-venv && /tmp/pb-venv/bin/pip install "protobuf==7.${T#v}"
   PY=/tmp/pb-venv/bin/python
   curl -fsSLo test/protobuf-suite/parser_unittest.cc \
     "https://raw.githubusercontent.com/protocolbuffers/protobuf/$T/src/google/protobuf/compiler/parser_unittest.cc"
   $PY test/protobuf-suite/tools/extract.py test/protobuf-suite/parser_unittest.cc \
     > test/protobuf-suite/raw.json
   $PY test/protobuf-suite/tools/lanes.py test/protobuf-suite/raw.json test/protobuf-suite
   ```

   `leniency.json` is written by hand. Set its `protocVersion` to what
   `protoc --version` prints, and re-measure it in step 2.

2. **Cross-check with the protoc binary.** It prints a progress line
   every 25 protoc runs.

   ```bash
   unzip -q protoc-36.2-linux-x86_64.zip -d test/protobuf-suite/protoc
   $PY test/protobuf-suite/tools/crosscheck.py test/protobuf-suite/protoc \
     test/protobuf-suite/raw.json test/protobuf-suite/valid.json
   $PY test/protobuf-suite/tools/probes.py test/protobuf-suite/protoc/bin/protoc \
     test/protobuf-suite/leniency.json
   ```

   At v36.2: 89/89 goldens agree; protoc refuses 98 of the 99 `invalid`
   cases and prints 115 of the 117 upstream diagnostic lines; the 13
   leniency probes are as recorded. The one case accepted and the two
   lines missed come from upstream tests that set
   `require_syntax_identifier_`, which the binary does not.

3. **Check against protoc's own parser.** The binary also resolves names
   and validates, so it can only check the corpus from outside; the parser
   is what the goldens describe. Build it once per release: about 300
   compile steps, each printed by Ninja as `[done/total]`.

   ```bash
   git clone --depth 1 --branch "$T" https://github.com/protocolbuffers/protobuf.git /tmp/protobuf
   cmake -S test/protobuf-suite/tools/oracle -B /tmp/oracle -G Ninja \
     -DCMAKE_BUILD_TYPE=Release -DPROTOBUF_SOURCE_DIR=/tmp/protobuf
   ninja -C /tmp/oracle oracle
   $PY test/protobuf-suite/tools/oracle-check.py /tmp/oracle/oracle test/protobuf-suite
   $PY test/protobuf-suite/tools/oracle-check.py /tmp/oracle/oracle --spec test/spec/aggregate.tsv
   $PY test/protobuf-suite/tools/oracle-check.py /tmp/oracle/oracle --spec test/spec/adjacent-strings.tsv
   ```

   At v36.2: the parser produces all 89 goldens and accepts all 50
   `accept-only` cases; it reports errors for 98 of the 99 `invalid` cases,
   with the upstream diagnostic among them for 96. Two of the three misses
   are the `require_syntax_identifier_` tests again, and the third,
   `ExplicitlyMapEntryError`, reports its line through a validation error
   collector upstream, where this run prints `-1:0`. Every fixture row
   that expects a descriptor equals the parser's: 84 of 84 in
   `aggregate.tsv` and 21 of 21 in `adjacent-strings.tsv`. The parser
   refuses 3 of the 12 `aggregate.tsv` error rows and the one
   `adjacent-strings.tsv` error row; the other 9 are the block
   `aggregate.tsv` marks as text protoc's parser records and text format
   refuses, and the check lists each.

4. **Run the conformance runners**, in every runtime: `npm test` in `ts/`,
   `GOWORK=off go test -count=1 ./...` in `go/`, `ci/rust/run.sh`. A
   case that fails is a parser defect: the goldens are protoc's.

5. **Move what records the corpus.** The provenance in `../AGENTS.md`,
   the version named in each runner's header comment, the lane counts in
   the root `AGENTS.md` ("Conformance", with the date it was measured),
   `README.md`, `ts/README.md` and `rs/AGENTS.md`, and the ratchets in
   `rs/tests/protobuf_conformance_test.rs` (in-scope `valid`,
   `accept-only`) and `rs/tests/perf_test.rs` (fixture rows). `grep` for
   the old numbers rather than trusting this list.

6. **Regenerate the fixture**, once step 4 is green, after `npm run build`
   in `ts/`. It prints a progress line every ten cases.

   ```bash
   node test/protobuf-suite/tools/suite-tsv.js
   ```
