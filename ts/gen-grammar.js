#!/usr/bin/env node

// Compile the proto grammar into the engine's serialized grammar, at build
// time, so that the TypeScript runtime never loads the ABNF compiler.
//
//   proto-grammar/*.abnf  --(@tabnas/abnf, here)-->  ts/src/proto-grammar.json
//
// The five .abnf files are concatenated exactly as embed-grammar.js
// concatenates them (it exports the function), then converted with the
// options the plugin used to pass at every install, `tag: 'proto'`,
// `start: 'proto'`, `wordKeywords` and `tokenClasses` (AGENTS.md, "Grammar
// conventions", says why both are required), plus `builtins: true`: the
// compiler then writes the `{rule, src, kids}` tree builders as the
// engine's `@node$` / `@capture$` / `@bubble$` builtins with their
// configuration as data, instead of closures. `toPureSpec` keeps those
// builtins and refuses a spec that still holds a closure, so the output is
// pure data and builds the same CST the closures built. The plugin adds
// its one action of its own (`recordAggregate`, on `constant`) at install.
//
// This file is the TypeScript port's alone. The engine's spec JSON is
// meant to be cross-runtime, but this compiler writes a whole-word keyword
// as `@~/^option(?![A-Za-z0-9_])/`, and a lookahead is a construct the Go
// (RE2) and Rust (`regex`) engines do not have. Their compilers write the
// same guard as `\b`, so each of those ports generates and embeds its own
// compiled grammar with its own compiler: go/proto-grammar.json
// (`go generate`) and rs/proto-grammar.json (rs/tests/grammar_spec_test.rs).
//
// Run via: npm run gen-grammar (from ts/), after changing any .abnf file
// or after an @tabnas/abnf or @tabnas/bnf release that changes what they
// emit. The output is deterministic: the same compiler on the same grammar
// writes the same bytes on every machine. ts/test/grammar-spec.test.ts
// compiles again in memory and fails when the committed file is stale.
//
// Not part of `npm run build`. A build that regenerated the file would
// make the staleness test compare the compiler with itself.

const fs = require('fs')
const path = require('path')

const { readGrammar } = require('./embed-grammar.js')

const SPEC_FILE = path.join(__dirname, 'src', 'proto-grammar.json')

// The conversion options, the ones `tn.abnf(grammarText, ...)` was given
// at install, with `builtins` for a function-free result.
const CONVERT = {
  tag: 'proto',
  start: 'proto',
  wordKeywords: true,
  tokenClasses: true,
  builtins: true,
}

// The compiled grammar as the text of src/proto-grammar.json.
function compileGrammar() {
  // Resolved here rather than at the top, so that requiring this file for
  // its constants does not load the compiler.
  const { abnfConvert, toPureSpec, toJsonic } = require('@tabnas/abnf')

  // The grammar admits every keyword as an identifier, which a compiler
  // without `tokenClasses` expands into millions of alternates
  // (tabnas/bnf#71) and never finishes. Probe it with a three-token
  // grammar first, so an old @tabnas/bnf fails here, at once and by name,
  // rather than hanging.
  const probe = abnfConvert('s = a a\na = "x" / "y"\n',
    { tag: 'proto', start: 's', tokenClasses: true })
  if (!probe?.options?.tokenSet?.a) {
    throw new Error('gen-grammar: the installed @tabnas/bnf does not ' +
      'support tokenClasses; upgrade @tabnas/bnf (tabnas/bnf#74)')
  }

  const spec = toPureSpec(abnfConvert(readGrammar(), CONVERT))
  return toJsonic(spec, { strict: true }) + '\n'
}

function main() {
  fs.writeFileSync(SPEC_FILE, compileGrammar())
  console.log('Compiled proto-grammar/*.abnf into',
    path.relative(path.join(__dirname, '..'), SPEC_FILE))
}

module.exports = { compileGrammar, CONVERT, SPEC_FILE }

if (require.main === module) main()
