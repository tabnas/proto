/* Copyright (c) 2025 Richard Rodger and other contributors, MIT License */

// Cross-runtime conformance, driven by the shared `test/spec/*.tsv` fixtures
// at the repo root (see ../../test/AGENTS.md).
//
// The fixture loader, the escape codec, the `ERROR:` contract and the row
// loop all come from @tabnas/support, whose Go half `go/parity_test.go`
// uses to run the SAME files — so the two implementations cannot drift
// without one of them going red, and neither can the two loaders.
//
// What is left here is only what is specific to proto: the row's options,
// and what an `ERROR:` cell means.
//
// (This also retires a harness defect the old runner had to work around:
// loading inside a `describe()` body made node report a red SUITE with
// ZERO failed tests and still exit 0, so an empty fixture printed a mark
// and the run went green. The shared runner loads at module scope, where a
// throw fails the file and sets the exit code — checked, not assumed.)

import { findSpecDir, makeRunner } from '@tabnas/support'

import { parse } from '../dist/proto'

makeRunner({
  parse: (input, row) => {
    const opts = row.named('opts')
    return parse(input, '' === opts.trim() ? undefined : JSON.parse(opts))
  },

  // proto's `ERROR:<want>` cells hold a fragment of the message — there is
  // one, `version mismatch` — rather than an error code, because the
  // rejection it names is the plugin's own version check rather than a
  // parse failure the engine gives a code to. A bare `ERROR` still accepts
  // any failure.
  matchError: (err: any, want) => String(err?.message).includes(want),
})
  // `findSpecDir` walks up from this file — `dist-test/` at runtime — to the
  // repo root's `test/spec`, so moving the suite does not mean recounting
  // `..` hops. `dir` then auto-discovers every fixture in it, so adding a
  // .tsv runs it in both runtimes without touching either runner.
  .dir(findSpecDir(__dirname))
