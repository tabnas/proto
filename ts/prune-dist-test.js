/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// Delete compiled test output whose TypeScript source is gone.
//
// `tsc --build` emits new files but never removes old ones, and
// `tsc --build --clean` only deletes outputs it still knows about — an
// orphan is by definition no longer in the project, so `--clean` walks
// straight past it. Measured: delete `test/x.test.ts`, rebuild, and
// `dist-test/x.test.js` is still there and `npm test` still runs it.
//
// That is the same defect as a stale artifact by another route: the suite
// reports on something that is not in `test/`. Wiping `dist-test` whole
// would also fix it and would cost a full recompile on every run, which
// is the runner a developer uses most.
//
// Node rather than `rm`: npm runs scripts through `cmd.exe` on Windows.

const fs = require('node:fs')
const path = require('node:path')

const SRC = path.join(__dirname, 'test')
const OUT = path.join(__dirname, 'dist-test')

if (!fs.existsSync(OUT)) process.exit(0)

// `x.test.js`, `x.test.d.ts` and `x.test.js.map` all come from `x.test.ts`.
const stem = (name) => name.replace(/\.(d\.ts|js\.map|js)$/, '')

let pruned = 0
for (const entry of fs.readdirSync(OUT, { withFileTypes: true })) {
  if (!entry.isFile()) continue
  if (!/\.(js|d\.ts|js\.map)$/.test(entry.name)) continue
  if (fs.existsSync(path.join(SRC, stem(entry.name) + '.ts'))) continue
  fs.rmSync(path.join(OUT, entry.name))
  console.log('pruned orphan: dist-test/' + entry.name)
  pruned++
}
if (0 === pruned) console.log('dist-test: no orphans')
