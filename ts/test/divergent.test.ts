/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// The divergence register, executed in the canonical runtime.
//
// `test/divergent.tsv` holds one row per input where this repo's three
// ports DISAGREE, with a cell per runtime. This file reads the `ts`
// column through @tabnas/support's DivergenceRegister, the same
// mechanism `rs/tests/divergent_test.rs` reads the `rust` column with.
//
// WHY THIS IS NOT A FIXTURE. A fixture fails when behaviour REGRESSES.
// The register fails both ways: when a port is repaired to agree with
// the others the row still claims they differ, so the suite goes red and
// names the row to delete. That is what keeps a divergence from
// outliving its own repair, and it is why the file sits beside
// `test/spec/` rather than in it, where the parity runner would run it.
//
// Two of these rows record a defect in THIS runtime, not in a port. The
// register is where they are pinned until the repair lands; see
// ../../DIVERGENCE.md for the prose and ../../test/divergent.tsv for the
// rows themselves.

import { join } from 'node:path'

import { findSpecDir, makeRegister } from '@tabnas/support'

import { parse } from '../dist/proto'

const REPO = join(findSpecDir(__dirname), '..', '..')

makeRegister({
  runtime: 'ts',
  runtimes: ['ts', 'go', 'rust'],

  parse: (input, row) => {
    const opts = row.named('opts')
    return parse(input, '' === opts.trim() ? undefined : JSON.parse(opts))
  },

  // As in parity.test.ts: an `ERROR:<want>` cell holds a fragment of the
  // message rather than a code, because this package declares none.
  matchError: (err: any, want) => String(err?.message).includes(want),

  // Compare after a JSON round trip, which is what the Go and Rust
  // runners do and what test/AGENTS.md states the contract to be. It
  // matters here more than anywhere: two of these rows record values
  // JSON alone can express, a `NaN` field number and a function-valued
  // `type`, and without the round trip the live object never equals the
  // cell that records it.
  normalize: (value) => JSON.parse(JSON.stringify(value)),
}).file(join(REPO, 'test', 'divergent.tsv'))
