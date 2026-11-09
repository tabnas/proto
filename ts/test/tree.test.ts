/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// The source-ordered tree, `parseTree`, held to the shared fixtures in
// `test/tree/*.tsv` (see ../../test/AGENTS.md): each source and the tree's
// JSON text, member order included, which the Go and Rust runners
// (`go/tree_test.go`, `rs/tests/tree_test.rs`) hold their ports to. The
// order is part of the contract: a host that reads the tree streams its
// members in that order, and the transducer part makes the same events.

import * as path from 'node:path'

import { findSpecDir, makeRunner } from '@tabnas/support'

import { parseTree } from '../dist/proto'

makeRunner({
  parse: (input) => JSON.stringify(parseTree(input)),
  // The cell is the tree's JSON text, compared as text.
  parseExpected: (expected) => expected,
  // An `ERROR:<want>` cell holds a fragment of the message: the nesting
  // cap is the plugin's own refusal, which carries no code.
  matchError: (err: any, want) => String(err?.message).includes(want),
}).dir(path.join(findSpecDir(__dirname), '..', 'tree'))
