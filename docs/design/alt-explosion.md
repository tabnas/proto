# Design: the alternate explosion in the compiled proto grammar

| | |
|---|---|
| **Status** | Investigation and recommendation. Nothing here is implemented; §9 says what to build, in which repository, and how to know it worked. |
| **Scope** | `@tabnas/bnf` (the emitter, where the cause is), `@tabnas/parser` (dispatch cost, token sets), this repository (the grammar, and what it can do meanwhile). |
| **Repo** | This document lives in `tabnas/proto` because proto is where the problem is met: the grammar the language needs is the one the compiler cannot compile. The emitter half is tracked as `tabnas/bnf#71`. |
| **Measured against** | `@tabnas/parser` 0.12.2, `@tabnas/abnf` 0.4.15, `@tabnas/bnf` 0.1.19, `@tabnas/proto` 0.5.0 (`main` at 366fd21), Node 22.22, Go 1.24.7, Rust 1.94 (release profile). Every figure below was produced by a script run against this tree; §11 says how. |

## 1. Summary

The five `.abnf` files are 104 lines and 48 rules. Compiled, they are
452 rules and 2,888 open alternates, 1,953 of which carry a four-token
prefix such as `#REQUIRED #T11 #TX #T11`. The compiler enumerates every
concrete four-token path an alternative can start with and emits one
alternate per path, so the table is a cross product of everything the
grammar allows in the first four token positions. That is the classic
strong-LL(k) blow-up, exponential in k, and the literature has had the
remedy since 1993: use as little lookahead as each decision needs, and
store what is needed as a tree rather than as tuples.

Two things make this more than a size complaint. First, the shipped
grammar is wrong in a way the fix requires touching: protoc accepts
keywords as identifiers (`optional bool reserved = 5;` is in
`descriptor.proto`), and the grammar refuses them, because admitting
them multiplies the tables past what compiles (6.1 million alternates,
per `tabnas/bnf#71`). Second, the Go port pays for every alternate on
every token: it parses the same file at 39 KB/s against 267 KB/s in
TypeScript and 320 KB/s in Rust, and its throughput moves linearly with
the alternate count.

The recommendation (§9) is layered. In the emitter, dispatch on the first
token unless two alternatives collide on it, and deepen only the
colliding pair, one token at a time, as a nested dispatch. In the engine
and emitter together, let a lookahead position name a token set, so a
27-way identifier class is one alternate rather than 27, and so a keyword
can be admitted wherever an identifier is expected. In the engine, index
alternates by first token so dispatch cost stops depending on the count.
The proto grammar can take two small steps today (§10) and is ready for
the rest once the emitter lands.

## 2. Ground state

`tabnas/proto` has no open issues and no open pull requests. `main` is
366fd21 and every workflow on it is green (`ci`, `deps-gate`, `Rust`,
`Docs`, `scorecard`). On this machine `npm test` and `go test ./...`
pass, and the Rust crate builds and parses once its sibling checkouts
(`parser`, `abnf`, `bnf`, `support`) sit beside the repository. Nothing
needed merging.

`tabnas/parser` has two open issues, both lexer-level divergences
(`#217`, a Rust token-set override that does not reach earlier
alternates; `#218`, block comments with no end marker). Neither bears on
this document, though `#217` is the Rust half of the token-set work in
§9.

## 3. The symptom, measured

### 3.1 Size

| measure | value |
|---|---|
| ABNF source | 104 lines, 48 rules |
| compiled rules | 452 (217 generated helpers) |
| open alternates | 2,888 (close alternates: 362) |
| alternates with a 4-token prefix | 1,953 |
| distinct prefix vectors | 761 |
| serialised spec | 295 KB |
| largest rule | `*messageElement` helper, 320 open alternates |

The next largest are `messageElement` (183), the `*topLevelDef` helper
(173), the two `*messageValueEntry` helpers (163 each) and `topLevelDef`
(98). A slice of `messageElement` shows the shape:

```
{"s":"#REQUIRED #T11 #TX #T11", "p":"messageElement$alt0", "b":4}
{"s":"#REQUIRED #T11 #TX #TX",  "p":"messageElement$alt0", "b":4}
{"s":"#REQUIRED #T11 #EXPORT #T11", ...}
{"s":"#REQUIRED #T11 #EXPORT #TX", ...}
{"s":"#REQUIRED #T11 #LOCAL #T11", ...}
...   (36 entries for `#REQUIRED …`, all pushing the same `field` rule)
```

Every entry peeks four tokens, pushes them all back (`b: 4`) and pushes
the same rule. Twelve entries begin `#REQUIRED`, twelve `#OPTIONAL`,
twelve `#REPEATED`, and the pattern repeats for the unlabelled case: one
entry per element of {label} × {leading dot or not} × {`TX`, `export`,
`local`} × {dot or `=` next}.

### 3.2 Which lines of grammar drive it

Recompiling variants of the source isolates the multipliers:

| grammar | open alternates |
|---|---|
| all five files (shipped) | 2,888 |
| without `fullIdent =/ symbolVisibility *( "." ident )` (edition 2024) | 1,950 |
| without the two `=/ symbolVisibility message / … enumDef` lines | 2,846 |
| `common.abnf` alone | 1,801 |
| common, with `field` split into a labelled and an unlabelled rule | 1,865 |
| common, without `[ "." ]` on `messageType` | 1,692 |

One line, the edition-2024 rule that lets `fullIdent` open with `export`
or `local`, adds 938 alternates, because it turns every identifier
position inside the four-token window into a three-way fan-out. The
`[ "." ]` prefix on `messageType` doubles the same positions. Nothing in
the grammar is unusual; the multiplication is in how it is compiled.

### 3.3 Cost, per runtime

A synthetic proto3 file of 200 messages with 10 fields each (104 KB):

| runtime | engine + grammar install | parse | throughput |
|---|---|---|---|
| TypeScript | 390 to 410 ms, 38.6 MB heap | 328 to 382 ms | 267 to 311 KB/s |
| Go | 33 to 38 ms, 3.1 MB | 2.63 to 2.69 s | 38 to 39 KB/s |
| Rust (release) | 105 ms | 319 ms | 320 KB/s |

The protoc corpus (139 inputs) parses in 0.25 ms per input in
TypeScript. Note that `parse()` in TypeScript and `Parse()` in Go build a
fresh engine on every call, so a caller who uses the one-shot API pays
the install each time; the Rust crate caches the engine in a `OnceLock`.

Where the time goes differs by runtime. A CPU profile of the TypeScript
parse puts 35% in the lexer's match body (the 26 keyword regexes are
tried in turn at each position), 16% in garbage collection, and about
16% in alternate matching (`parse_alts` and `Rule.process`). The Go
profile is different in kind: 88% of samples are under
`Lex.matchMatch`, 56% under `Context.altS`, and 44% are map lookups.
The comment above `matchMatch` in `go/lexer.go` states the reason: the
gating work is "O(match tokens x alternates x tins-per-slot)" per lex
attempt. Every alternate in the current rule is consulted for every
token the lexer cuts.

That is why Go's throughput follows the alternate count where the others'
barely moves:

| grammar variant (open alternates) | TypeScript | Go |
|---|---|---|
| shipped (2,888) | 267 KB/s | 39 KB/s |
| without the `fullIdent` extension (1,950) | 316 KB/s | 54 KB/s |
| `common.abnf` alone (1,801) | 321 KB/s | 74 KB/s |

### 3.4 How it grows

protoc treats every word as an identifier and compares keywords by text
(§6), so a field, a message, an enum value, a package segment or an rpc
may be named `message`, `option`, `to`, `max`, `stream` or `reserved`.
The ABNF spelling of that is `ident = TX / "message" / "option" / …`.
Admitting keywords one at a time, under the shipped compiler:

| keywords admitted as `ident` | rules | open alternates | largest rule | install |
|---|---|---|---|---|
| 0 (shipped) | 452 | 2,888 | 320 | 0.4 s |
| 1 | 511 | 5,794 | 612 | |
| 2 | 568 | 10,027 | 1,499 | 1.1 s |
| 4 | 682 | 24,530 | 5,307 | 1.6 s |
| 6 | 796 | 50,330 | 13,267 | 3.0 s, 249 MB heap |
| 26 (all of them) | | 6,102,261 | 962,726 | 28 to 35 s, then a stack overflow at install |

The last row is from `tabnas/bnf#71`, which reproduces the mechanism
with a three-rule grammar (`doc = "{" *entry "}"`, `entry = kw kw`,
`kw = "k1" / … / "kN"`): 457,711 alternates at N = 26, of which 457,654
are in the repetition helper. The largest rule in every row is a
`*messageValueEntry` helper, because a repetition's dispatch window runs
across iterations (§5.3).

## 4. The grammar is also wrong today, for the same reason

Two defects in the shipped grammar are consequences of the size problem,
not separate bugs.

**Keywords cannot be identifiers.** Sixteen inputs that protoc accepts
are refused with `[tabnas/unexpected]`: a field named `message`,
`option`, `optional`, `map`, `to`, `max`, `stream` or `reserved`; a
message named `service`; a package `foo.message.bar`; a type
`foo.option.Bar`; an enum value `max`; an rpc `stream`; an option named
`message`; a field named `export` in edition 2024. The vendored protoc
corpus (`test/protobuf-suite/`) has no case of a keyword used as a name,
which is why the conformance lanes are green.

**Identifiers that case-fold to a keyword are refused.** RFC 5234 quoted
strings are case-insensitive, so `"edition"` compiles to the matcher
`/^edition(?![A-Za-z0-9_])/i`, and `Edition`, `Message`, `Service` and
`MAX` all lex as keywords. protobuf's own `descriptor.proto` is refused
at line 45, `enum Edition {`. The same file declares fields named
`package`, `service`, `syntax`, `edition`, `reserved`, `repeated` and
`weak`, each of which the grammar refuses in isolation.

The second defect has a grammar-only fix (§10.1). The first does not:
§3.4 is what happens when the grammar says what protoc does.

## 5. Where the alternates come from

The pipeline is `parseAbnf` → `emitGrammarSpec` in `@tabnas/bnf`
(`ts/src/compiler.ts`; the Go port is `go/emit.go` and `go/factor.go`,
the Rust port `rs/src/emit.rs`). `emitGrammarSpec` runs, in order, value
planning, literal lifting, `eliminateLeftRecursion`,
`rewriteProbeDispatches`, `leftFactor`, `rewriteTailRepeats`, `desugar`,
then FIRST/FOLLOW computation and one `emitProduction` per rule. Five
things in that chain compound.

### 5.1 The dispatcher enumerates every k-token path, unconditionally

A rule with more than one alternative, at least one of which has more
than one segment, is emitted as a dispatcher: each alternative becomes
an impl rule (`<rule>$alt<i>`), and the dispatcher's open state peeks
tokens and pushes the matching impl. The peeks come from
`altPrefixes(alt, grammar, …, LOOKAHEAD_K)` with `LOOKAHEAD_K = 4`
(`go/factor.go` names it `lookaheadKSpan`, also 4; `go/emit.go` has a
second `lookaheadK = 4`). `altPrefixesRaw` walks the alternative element
by element; a reference to a multi-alternative rule fans out into one
path per sub-alternative, an optional fans out into with and without,
and the walk stops only at four tokens or a cycle. The emitter then
issues one alternate per distinct path:

```js
const prefixes = altPrefixes(alt, grammar, literals, regexTokens, LOOKAHEAD_K)
for (const p of usable) dispatchEntries.push({ s: p.join(' '), b: p.length, p: implName, … })
```

Nothing asks whether the first token already decides. The single-segment
path of the same function is more careful: it emits one-token FIRST
peeks and fans out to K-token prefixes only when `altHeadContested` or
`contestedByFollow` says two heads overlap. The multi-segment path, which
is the one every statement rule in this grammar takes, always fans out.

### 5.2 Left factoring only fires beyond the window

`leftFactor` merges alternatives that share a prefix, but `factorOnce`
declines when the shared prefix fits inside the window
(`prefixBeyondLookahead` is `LOOKAHEAD_K < seqTokenSpan(prefix)`), on
the reasoning that "the dispatcher already separates these". So
`[ label ] fieldType …` and `[ label ] "group" …` are never factored;
they are separated by enumerating both through four tokens.

### 5.3 A repetition's window crosses into the next iteration

`*messageElement` desugars to a helper `H = messageElement H / ε`. Its
prefixes are computed through the same walk, so a one-token element
(`;`) is followed into the next iteration and the next, until four
tokens are filled: the helper carries 320 alternates to the element's
183. For `*messageValueEntry`, whose entries are two or three tokens
long, the window spans two iterations and the product squares. This is
the mechanism `tabnas/bnf#71` measures as N⁴.

### 5.4 Paull's substitution copies the fan-out into every caller

`eliminateLeftRecursion` inlines a rule referenced at the head of an
alternative (`abnf/AGENTS.md` records that "Paull's substitution can blow
up on large mutually-recursive grammars"; RFC 5322 and Dhall do not
finish). Each copy of an inlined rule carries its own fan-out, so the
same 183 `messageElement` paths recur in each context that starts with
it.

### 5.5 A lookahead position cannot name a class

The engine's alternate spec already allows a position of `s` to be a
token set (`#KEY` in the jsonic grammars resolves to several tins), and
TypeScript re-resolves those names on every `norm()`. The emitter never
uses this: a position that could be any of `TX`, `export`, `local`
becomes three alternates, and a position that could be any of 27
identifier-like tokens becomes 27. The cross product in §3.1 is the
product of the class sizes at each of four positions.

### 5.6 The engine scans alternates linearly

`parse_alts` in `ts/src/rules.ts` iterates every alternate of the rule
in order (the code carries `// TODO: replace with lookup map`), and the
Go lexer gate does the same per token (§3.3). Neither is the cause of
the count, but both turn the count into parse time, and Go turns it into
most of the parse time.

One more coupling, which any fix has to respect: the descriptor walk
reads "inlined values from `src`" (`AGENTS.md`, "The walk and abnf
inlining"). Changing how the compiler factors or inlines changes CST
shapes, and the walk notices. §7.1 shows one such case.

## 6. What other parsers do

The proto language is LL(1) once keywords are contextual, and every
production parser treats it that way.

protoc's parser (`src/google/protobuf/compiler/parser.cc`) has one
`TYPE_IDENTIFIER` token class and no keyword class; `LookingAt("message")`
compares text, `ConsumeIdentifier` accepts any identifier, and
`ParseMessageStatement` is a chain of `LookingAt` tests ending in "else
parse a field". Its one two-token decision is `map` followed by `<`; a
`map` not followed by `<` is a type named map.

`bufbuild/protocompile` (Go, goyacc) and `jhump/protoreflect` before it
declare every keyword as an identifier-typed token and give `identifier`
a production listing all of them (`identifier : _NAME | _SYNTAX |
_EDITION | … | _LOCAL`), with the comment "we don't allow message
statement keywords as identifiers" at statement heads, to mimic protoc.
`protobufjs`, `protox` (Rust, logos) and `rust-protobuf` are hand-written
descents that compare identifier text; `pb-rs` is `nom` ordered choice.

The ANTLR `Protobuf3.g4` grammar in `grammars-v4` keeps keywords as
distinct tokens and folds them back with one rule, `ident : IDENTIFIER |
keywords ;`, listing 26 keywords. ANTLR never tabulates that cross
product: ALL(*) builds the lookahead DFA for each decision at parse time
from the inputs actually seen.

Chevrotain, the closest analogue (a JavaScript LL(k) toolkit that
enumerates concrete token paths), extends a path only while it is still
ambiguous and stops as soon as a prefix is unique, with a per-rule
`maxLookahead` (default 3, once 5, then 4). Its documentation warns that
"the number of possible paths in the grammar can grow exponentially as
the max length of the possible paths increases", and it now ships an
ALL(*) strategy as a plugin.

Two LR tools solve the keyword half with a lexer feature. Lezer's
`@extend<identifier, "message">` lets a keyword keep both readings, "for
contextual keywords where it isn't clear whether they should be treated
as an identifier or a keyword until a few tokens later". tree-sitter's
`word` field makes keyword recognition a two-step match over the
identifier token; the tree-sitter proto grammar hit exactly this
repository's problem (`message.v1.Req f = 1;` failing because `message`
lexed as a keyword) and fixed it on 2026-09-24 with an aliased
`_keyword_identifier` choice.

Coco/R and JavaCC keep LL(1) as the default and let the author add
lookahead at the one choice point that needs it (`IF(IsCast())`,
`LOOKAHEAD(2)`); JavaCC's manual "strongly discourages" raising the
global default.

## 7. What the literature says

Parr's thesis (Purdue, 1993) is the direct citation. It states that
conventional lookahead "for LL(k) and LR(k) parsers and their variants
is exponentially large in k" because it "must be able to represent all
possible vocabulary-symbol permutations of length k", and it defines the
"Optimal LL(k) Normal Form", in which every alternative starts with a
distinct k-terminal prefix. That normal form is what `emitProduction`
produces. The thesis offers three remedies, in order: modulate k per
decision, try a linear approximation (k sets of terminals, one per
position, "rather than O(|T|^k) k-tuples") before full tuples, and
cache. Its survey of 22 grammars found that "most decisions need only
zero or one terminal of lookahead". Parr and Quong (SPE 1995) built
ANTLR on this: "ANTLR computes and uses the minimum lookahead necessary
for each decision".

Grune and Jacobs (Parsing Techniques, 2nd ed. 2008, §8.3) give the
textbook version: strong-LL(k) parsers with k > 1 "are seldom used in
practice, because the parse tables are huge", and cite Ukkonen (1983)
for the exponential lower bound on full LL(k > 1). The remedy they list
under §8.3.2 is linear-approximate LL(k), and under §8.3.3 LL-regular
grammars, where the lookahead is a regular language decided by a finite
automaton rather than a fixed k (Čulik and Cohen 1973 for LR; Jarzabek
and Krawczyk 1975 and Nijholt 1976 to 1982 for LL). Bermudez and
Schimpf (1990) turned that into a practical algorithm that "determines
the amount of lookahead required, and the user is spared the task of
guessing it".

Parr and Fisher (PLDI 2011) grafted lookahead DFAs onto LL parsers, with
decisions that "throttle up from conventional fixed k ≥ 1 lookahead to
arbitrary lookahead and, finally, fail over to backtracking"; Parr,
Harwell and Fisher (OOPSLA 2014) moved the analysis to parse time
because the LL(*) condition is undecidable statically.

The PEG line is the other family. Ford (2002, 2004) replaces lookahead
with prioritised choice and memoised backtracking; Mizushima, Maeda and
Yamaguchi (PASTE 2010) insert a cut after an alternative whose FIRST set
is disjoint from the alternatives after it, which is the same decision
this compiler's `altHeadContested` makes, expressed as a commit point.
Medeiros and Ierusalimschy (2008) note that packrat's linear space has
"a rather large constant", which is the cost side.

The engine's own architecture note (`parser/doc/architecture.md`) says
alternates are tried in order, the first match wins, and there is no
backtracking search. That is the strong-LL family, and the literature's
advice for it is unambiguous: minimal k per decision, per-position sets
where they suffice, a trie where they do not, and a bounded fallback for
the rest. Every figure in §8 is that advice, measured.

## 8. What the experiments say

### 8.1 The shipped grammar needs two tokens at a handful of decisions

The emitter was recompiled with `LOOKAHEAD_K` lowered (a one-line patch
of the installed `@tabnas/bnf`, not committed), and every corpus input
and spec row was compared, as a descriptor, with the shipped parser's
recorded output:

| uniform K | open alternates | install | outputs identical to shipped (of 313) |
|---|---|---|---|
| 4 (shipped) | 2,888 | 400 ms | 313 |
| 3 | 1,686 | 390 ms | 312 |
| 2 | 1,202 | 310 ms | 312 |
| 1 | 901 | 280 ms | 284 |

K = 1 fails 29 inputs, all at decisions that need a second token:
`optional group` against `optional Foo`, `import option` and `import
public` against `import "x"`, `-inf` against `-33`, `extensions … to
max` shapes, and the edition-2024 visibility prefixes. K = 2 resolves
all of them. The one input that differs at K = 2 and K = 3,
`ParseMessageTest.ExplicitOptionalLabelProto3`, parses, but the CST
inlined differently and the walk read the label from a different place.
That is §5.6's coupling, and it is a test that any emitter change has
to keep green.

So the shipped grammar's tables are between 2.4 and 3.2 times larger
than they need to be, and the excess is not paying for correctness.

### 8.2 A protoc-faithful grammar compiles once the window is small

The grammar was rewritten as protoc reads it: all 26 keywords admitted
in `ident`, keyword-headed statements listed before the identifier-headed
one in each element list, `group` in `common.abnf`, and the edition-2024
`fullIdent` extension deleted (unnecessary once `export` and `local` are
identifiers).

| build | rules | open alternates | largest rule | install | keyword cases (15) | protoc corpus (139) |
|---|---|---|---|---|---|---|
| K = 4 | | 6,102,261 (`bnf#71`) | 962,726 | 28 to 35 s, then overflow | | |
| K = 2 | 1,918 | 25,084 | 988 | 3.9 s | 15 | 136 same, 1 CST-shape differ, 2 refused |
| K = 1, label left-factored by hand | 1,979 | 9,164 | 45 | 3.6 s | 14 | 122 accepted, 17 refused |

At K = 2 the grammar does what protoc does: `int32 message = 1;`,
`package foo.message.bar;`, `enum E { max = 0; }`, `rpc stream (M)
returns (M);`, `.message.Foo f = 1;`, nested messages, groups, maps,
reserved ranges and `-inf` all parse, and 136 corpus descriptors are
identical. The two refusals are the edition-2024 `export message` cases,
where the `=/ symbolVisibility message` alternatives are appended after
`field` and lose the first-match on `#EXPORT #MESSAGE` (now two
identifier tokens); ordering them first fixes it, as protoc's
`LookingAt("export")` does. The K = 1 row shows the shape a minimal-k
emitter would produce: `messageElement` falls from 988 alternates to
45, and the 17 refusals are the same second-token decisions as in §8.1.

Both rows are still around ten thousand alternates and take over three
seconds to install, and the reason is §5.5: `ident` is a 27-alternative
rule, so every position that can hold an identifier fans out 27 ways.
With a token set at those positions the K = 1 row would be in the
hundreds. The two levers are independent and both are needed.

## 9. Recommendation

Do these in order; each is useful alone and each makes the next cheaper.

### 9.1 Emitter: minimal lookahead per decision, deeper lookahead as a trie

In `emitProduction` (all three ports), replace the unconditional
`altPrefixes(alt, …, LOOKAHEAD_K)` fan-out of the dispatcher path with
the decision the single-segment path already makes:

1. Compute FIRST₁ for each alternative (and FOLLOW for a repetition or
   factored helper). If no token is claimed by two alternatives with
   different targets, emit one alternate per (token, alternative), or one
   per alternative once §9.2 lands.
2. For each token that two or more alternatives claim, deepen only those
   alternatives by one position, and repeat, up to `LOOKAHEAD_K`. Emit
   the deeper prefixes as a nested dispatch: a helper rule keyed on the
   contested head that peeks the next position (Parr's child-sibling
   tree; the engine expresses it with `p:` plus `b:` push-back, exactly
   as the probe dispatchers already do). The number of alternates is
   then the size of the trie of contested prefixes, not the product of
   all paths.
3. Seed the repetition helper's prefix walk with itself, so a window
   never runs into the next iteration (`bnf#71`, direction 4). The
   continue-or-exit decision of `*X` needs FIRST(X) against FOLLOW and
   nothing more.
4. Keep `leftFactor`, but let it fire on any shared prefix that a
   contested decision would otherwise enumerate, rather than on
   prefixes beyond the window alone.

Expected effect, from §8: the shipped grammar at about 900 alternates
plus a few dozen for the two-token decisions; the protoc-faithful grammar
compiling in well under a second. `bnf#71` measured its repro falling
from 457,711 to 85 alternates with first-token dispatch, and reports
that most front-end suites pass with it. The one known cost is CST
shape: the walk here must be re-run against the corpus, and where it
read an inlined value from `src` it may need to read a child instead.

### 9.2 Engine and emitter: a lookahead position may name a token set

Give the emitter a way to say "any identifier-like token here" as one
position rather than 27 alternates: allocate a token set per FIRST class
it would otherwise fan out (`tokenSet` in the spec's options), and emit
`s` positions that name the set. This is Parr's linear approximation.
Where it over-approximates (the compiler can check: two alternatives
whose per-position sets intersect at every position but whose tuples
do not), fall back to the tuples for that decision only.

The TypeScript engine already resolves set names per position in
`normalt`; Go resolves them per parse in `Context.altS` (§3.3 shows the
cost) and should resolve them at install; the Rust engine expands them
at install and does not re-resolve (`parser#217`). Making all three
resolve once, at install, is the engine half.

This is also the soft-keyword facility proto needs. With it, the front
end can offer an option (`wordKeywords: { contextual: true }`, or an
annotation on the `ident` rule) meaning "a rule named `ident` also
accepts every keyword token", which is what Lezer's `@extend`,
tree-sitter's `word` and protocompile's `identifier` production each
provide. The grammar then stays as written and protoc-faithful, and the
statement-head ordering of §8.2 is the only discipline the author needs.

### 9.3 Engine: index alternates by first token

Build, at install, a map from first-position tin to the ordered list of
alternates that can match it (plus the wildcard and nullable ones), and
have `parse_alts` and the Go lexer gate consult that list rather than
every alternate. This removes the linear dependence on the count that
§3.3 measures in Go, and it is worth doing whether or not §9.1 shrinks
the count, because §9.2 will keep some rules wide. Go first: it is the
runtime where the count is most of the parse time.

### 9.4 What is not recommended

A uniform lower K (1 or 2) is not a fix: it breaks the second-token
decisions (§8.1) or still multiplies (§8.2), and other front ends (the
GBNF corpus with character-level tokens) are why K is 4. Generalised
backtracking (PEG-style ordered choice with rewind) as the primary
dispatch would trade table size for speculative parsing and an unbounded
rewind window, against the engine's deterministic design; keep the
existing probe and rewind machinery as the fallback for decisions no
finite lookahead separates, which is where LL(*) also fails over to
backtracking. GLR or Earley is a different engine. Restructuring the
proto grammar alone cannot express keywords as identifiers under the
current emitter (§3.4), so it is not a route to correctness, only to a
smaller wrong grammar.

### 9.5 How to know it worked

- The shipped grammar compiles to at most 1,000 open alternates and
  installs in under 100 ms in TypeScript.
- The protoc-faithful grammar (§8.2) compiles in under a second and
  parses all 139 corpus inputs to identical descriptors, plus the 17
  keyword-as-identifier inputs in §4, plus protobuf's `descriptor.proto`.
- Go parses the 104 KB synthetic file within two times the TypeScript
  time.
- `test/spec/*.tsv` and `test/divergent.tsv` hold in all three runtimes.
- The `bnf` front-end suites (abnf, gbnf, ebnf conformance) hold, with
  the `abnf/AGENTS.md` line "Alt dispatch is one-token lookahead plus the
  probe pattern" made true again (today it describes an emitter that was
  since replaced by the K = 4 enumeration).

## 10. What proto can do now

### 10.1 Make keyword literals case-sensitive

RFC 7405 `%s"…"` literals are supported by the front end. Writing every
keyword as `%s"message"` compiles to the same 2,888 alternates and makes
`enum Edition {`, `int32 Message = 1;` and `message Service {}` parse
(measured). Lowercase keywords still dispatch as keywords, and
`int32 message = 1;` is still refused, so this is the second defect of §4
only. It is a `fix!:` for anyone relying on the case-insensitive
reading, which protoc never offered.

### 10.2 Record the keyword gap where a reader meets it

State in `README.md` and `DIVERGENCE.md` (or a new entry in
`test/protobuf-suite/AGENTS.md`) that keywords are reserved words in
this grammar and that `descriptor.proto` is refused, with the §4 list,
and pin it with a fixture that is expected to flip when §9.1 and §9.2
ship, so the gap cannot be forgotten and its closing cannot go unnoticed.

### 10.3 Prepare the grammar for first-token dispatch

Reorder each element list keyword-first with the identifier-headed
statement last (`messageElement = enumDef / message / … / field`, and
likewise `oneofElement`, `enumElement`, `serviceElement`), which is
protoc's `LookingAt` order and is what §8.2 needed. This changes nothing
under the current emitter (the four-token prefixes are disjoint either
way) and removes the one ordering hazard the new emitter would have.

### 10.4 When the emitter ships

Admit the keywords in `ident`, move `groupField` into `common.abnf`,
delete `fullIdent =/ symbolVisibility *( "." ident )`, order the
`symbolVisibility` alternatives before `field`, and re-measure §3.3 in
all three runtimes. Consider caching the engine behind the one-shot
`parse()` in TypeScript and Go, as the Rust crate does, since the
install is paid per call today.

## 11. How the figures were produced

All measurements were made on one machine in one session, from a fresh
`npm install` of the published dependencies, `go test ./...` from
`go/`, and a release build of `rs/` against sibling checkouts of
`parser`, `abnf`, `bnf` and `support`. Alternate counts come from
`abnfConvert(grammarText, { tag: 'proto', start: 'proto', wordKeywords:
true })` and from the installed engine's `tn.rule()`. The lookahead
sweep patched `const LOOKAHEAD_K = 4` in a copy of the installed
`@tabnas/bnf` and compared descriptors against the shipped parser's
outputs recorded in a separate process. Throughput used the synthetic
file described in §3.3, best of three to five runs after one warm-up.
Profiles are `node --cpu-prof` and Go's `runtime/pprof`. The literature
and third-party statements were read from the primary sources named in
§6 and §7; page and line references are as of 2026-09-25.
