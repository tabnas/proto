
const Fs = require('node:fs')
const Path = require('node:path')

const REPO = Path.join(__dirname, '..', '..')

// The reader-facing set. Working documents (design notes, feasibility
// reports, ledgers) are deliberately out: see "The published set" in
// docs/STYLE-GUIDE.md.
const PAGES = [
  "ts/doc/concepts.md",
  "ts/doc/guide.md",
  "ts/doc/reference.md",
  "ts/doc/tutorial.md",
  "README.md",
  "ts/README.md"
]

const TUTORIALS = [
  "ts/doc/tutorial.md"
]


function exists(rel) {
  return Fs.existsSync(Path.join(REPO, rel))
}


// Filtered to what is actually on disk, so a renamed page fails as a
// missing gate rather than as a crash.
function gatedDocs() {
  return PAGES.filter(exists)
}


function tutorials() {
  return TUTORIALS.filter(exists)
}


module.exports = { gatedDocs, tutorials, PAGES }

if (require.main === module) {
  process.stdout.write(gatedDocs().join('\n') + '\n')
}
