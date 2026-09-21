
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
  "ts/README.md",
  "rs/README.md"
]

const TUTORIALS = [
  "ts/doc/tutorial.md"
]


function exists(rel) {
  return Fs.existsSync(Path.join(REPO, rel))
}


// A declared page that is not on disk THROWS.
//
// This filtered instead, and the comment here claimed the filter made a
// renamed page "fail as a missing gate". It did the opposite: the page
// left the list, both halves of the gate carried on over what remained,
// and the coverage test passed because it only counts what the list
// returned. Deleting a page was the one way to stop it being checked.
function gatedDocs() {
  return present(PAGES, 'gated')
}


function tutorials() {
  return present(TUTORIALS, 'a tutorial')
}


function present(declared, what) {
  const gone = declared.filter((f) => !exists(f))
  if (0 < gone.length) {
    throw new Error(
      `gated-docs: declared ${what} but not on disk: ` + gone.join(', ') +
      '. Rename it here, or delete the entry deliberately.')
  }
  return declared
}


module.exports = { gatedDocs, tutorials, PAGES }

if (require.main === module) {
  process.stdout.write(gatedDocs().join('\n') + '\n')
}
