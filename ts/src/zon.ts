/* Copyright (c) 2025 Richard Rodger, MIT License */

// The engine is the tabnas parser; jsonic supplies the relaxed-JSON
// grammar that the embedded grammar text is authored in.
import {
  Tabnas,
  Rule,
  Plugin,
  Context,
  Config,
  TabnasOptions,
  Lex,
} from '@tabnas/parser'
import { jsonic } from '@tabnas/jsonic'

// Plugin options.
type ZonOptions = {
  // When true, parse Zig char literals ('x') as numeric code points.
  // When false, parse them as single-character strings.
  charAsNumber: boolean
  // When set, wrap enum literals (.foo used as value) in `{ [enumTag]: 'foo' }`
  // instead of producing the bare string 'foo'.
  enumTag: null | string
}

// --- BEGIN EMBEDDED zon-grammar.jsonic ---
const grammarText = `
# ZON Grammar Definition
# Parses Zig Object Notation (ZON) - a data format based on Zig anonymous
# struct literals.
#
# Example:
#   .{
#       .name = "example",
#       .version = "0.0.1",
#       .deps = .{ .foo = .{ .url = "https://..." } },
#       .paths = .{ "build.zig", "src" },
#   }
#
# The custom zon-dot lex matcher distinguishes struct (map) and tuple (list)
# openings at lex time by peeking ahead:
#   .{  followed by  .ident =    -> emits #OB  (struct / map)
#   .{  otherwise                -> emits #OS  (tuple / list)
# Both close on } which lexes as #CB. The list rules below use #CB (not
# the default #CS) as the list terminator so that the single } character
# closes both struct and tuple literals.
#
# A bare .identifier emits #TX with val = identifier (the leading dot is
# stripped). This token is both a valid KEY (when followed by =) and a
# valid VAL (when used as an enum literal).
#
# The grammar is applied with { rule: { alt: { g: 'zon' } } } so every
# alt below is automatically tagged with the 'zon' group.

{
  rule: val: open: [
    # Empty .{} -> empty list.
    { s: '#OS #CB' b: 2 p: list g: 'list,empty' }
  ]

  rule: list: open: [
    # @array$ allocates the list node (an empty array). Under the new core
    # the list node is no longer auto-seeded: @tabnas/json's @array$ only
    # runs on its own #OS open alts, which ZON replaces here. Without this
    # the elem rule's @elem-bc/replace would push onto an undefined node.
    { s: '#OS #CB' b: 1 a: '@array$' g: 'list,empty' }
    { s: '#OS' p: elem a: '@array$' g: 'list,open' }
  ]
  rule: list: close: [
    { s: '#CB' g: 'list,close' }
  ]

  rule: elem: close: [
    { s: '#CA #CB' b: 1 g: 'elem,trailing' }
    { s: '#CA' r: elem g: 'elem,next' }
    { s: '#CB' b: 1 g: 'elem,end' }
  ]

  rule: pair: close: [
    { s: '#CA #CB' b: 1 g: 'pair,trailing' }
    { s: '#CA' r: pair g: 'pair,next' }
    { s: '#CB' b: 1 g: 'pair,end' }
  ]
}
`
// --- END EMBEDDED zon-grammar.jsonic ---

// Plugin implementation.
const Zon: Plugin = (tn: Tabnas, options: ZonOptions) => {
  const charAsNumber = !!options.charAsNumber
  const enumTag = options.enumTag || null

  // Human descriptions for the ZON tokens, surfaced in railroad diagram
  // legends (read off the live config by @tabnas/railroad). ZON uses Zig
  // anonymous-struct syntax: both maps and tuples open with `.{` and close
  // with `}`; bare { [ ] are disabled and keys are `.identifier` field names.
  tn.options({
    config: {
      modify: {
        'zon-tokendesc': (cfg: any) => {
          cfg.tokenDesc = Object.assign(cfg.tokenDesc || {}, {
            '#OB': '.{ — start of a struct (map)',
            '#OS': '.{ — start of a tuple (list)',
            '#CS': '} — end of a tuple (list)',
            'KEY': 'field name: .identifier (dot stripped)',
            'VAL': 'value: number, string, true/false/null, or .enum',
          })
        },
      },
    },
  })

  // If enumTag is set, wrap enum-literal values (produced by zonDot) into
  // `{ [enumTag]: name }` objects. The relaxed-JSON grammar takes ownership
  // of the `@val-bc` (val close) phase via `@val-bc/replace`, which sets
  // r.node from the matched token; once a phase is "replaced" the engine
  // suppresses any `/prepend` on it. So run in the `@val-ac` (after-close)
  // phase and rewrap the node the close handler produced from the enum token.
  const refs: Record<string, Function> = {
    '@val-ac': (r: Rule, _ctx: Context) => {
      if (!enumTag) return
      if (undefined !== r.child.node) return
      if (0 === r.os) return
      const tkn: any = r.o0
      if (tkn && tkn.use && tkn.use.zonEnum) {
        r.node = { [enumTag]: tkn.val }
      }
    },
  }

  const grammarDef = new Tabnas().use(jsonic).parse(grammarText)
  grammarDef.ref = refs
  // All jsonic option overrides live on the grammar object so the plugin
  // applies them atomically alongside its rule alts.
  grammarDef.options = {
    rule: {
      // Remove jsonic extensions (implicit maps/lists, top-level commas,
      // path dives). ZON uses explicit struct literals only.
      exclude: 'jsonic,imp',
      start: 'val',
    },
    fixed: {
      token: {
        // Bare `{`, `[`, `]` are not valid in ZON. Struct opening is `.{`
        // which is handled by the custom zonDot lex matcher below.
        '#OB': null,
        '#OS': null,
        '#CS': null,
        // `=` replaces `:` as the key/value separator.
        '#CL': '=',
      },
    },
    tokenSet: {
      // ZON field names are identifiers only.
      KEY: ['#TX'],
    },
    string: {
      chars: '"',
      multiChars: '',
      // Zig-flavoured escape sequences.
      escape: {
        n: '\n',
        r: '\r',
        t: '\t',
        '\\': '\\',
        '"': '"',
        '\'': '\'',
      },
      allowUnknown: false,
    },
    number: {
      lex: true,
    },
    // Only `//` line comments in ZON.
    comment: {
      lex: true,
      def: {
        hash: { lex: false },
        slash: { line: true, start: '//', lex: true, eatline: false },
        multi: { lex: false },
      },
    },
    value: {
      lex: true,
      def: {
        true: { val: true },
        false: { val: false },
        null: { val: null },
      },
    },
    // The default jsonic text matcher is disabled; identifiers are only
    // produced by the custom zonDot matcher below.
    text: {
      lex: false,
    },
    lex: {
      match: {
        zonDot: { order: 1e5, make: buildZonDotMatcher() },
        zonMultiString: { order: 1.1e5, make: buildZonMultiStringMatcher() },
        zonChar: { order: 1.2e5, make: buildZonCharMatcher(charAsNumber) },
      },
    },
  }

  // Tag every alt in this grammar with the 'zon' group so callers can
  // selectively exclude zon alts via `rule.exclude: 'zon'`.
  tn.grammar(grammarDef, { rule: { alt: { g: 'zon' } } })
}

// Custom lex matcher for `.`-prefixed tokens.
//   `.{`            -> #OB if followed by `.ident =`, otherwise #OS
//   `.identifier`   -> #TX (val = identifier, use.zonEnum = true)
// Runs ahead of the fixed-token matcher so it reliably owns the `.` prefix.
function buildZonDotMatcher() {
  return function makeZonDotMatcher(_cfg: Config, _opts: TabnasOptions) {
    return function zonDotMatcher(lex: Lex) {
      const { pnt } = lex
      const src: string = lex.src as unknown as string
      const { sI, cI } = pnt
      if ('.' !== src[sI]) return undefined

      // `.{` opens a struct literal. Decide map vs list by peeking ahead.
      if ('{' === src[sI + 1]) {
        const isMap = peekIsMapOpen(src, sI + 2)
        const tin = isMap ? '#OB' : '#OS'
        const tkn = lex.token(tin, undefined, '.{', pnt)
        pnt.sI = sI + 2
        pnt.cI = cI + 2
        return tkn
      }

      // `.identifier` - field name or enum literal.
      if (!isIdStart(src.charCodeAt(sI + 1))) return undefined
      let eI = sI + 1
      while (eI < src.length && isIdCont(src.charCodeAt(eI))) eI++

      const srcText = src.substring(sI, eI)
      const name = src.substring(sI + 1, eI)
      const tkn = lex.token('#TX', name, srcText, pnt, { zonEnum: true })
      pnt.sI = eI
      pnt.cI = cI + (eI - sI)
      return tkn
    }
  }
}

// Returns true if the position inside `.{ ... }` starts with `<ws>.ident<ws>=`,
// indicating a struct/map literal rather than a tuple/list.
function peekIsMapOpen(src: string, start: number): boolean {
  let i = skipInsig(src, start)
  if ('.' !== src[i]) return false
  i++
  if (!isIdStart(src.charCodeAt(i))) return false
  i++
  while (i < src.length && isIdCont(src.charCodeAt(i))) i++
  i = skipInsig(src, i)
  return '=' === src[i]
}

// Skip whitespace, newlines, and `//` line comments.
function skipInsig(src: string, i: number): number {
  while (i < src.length) {
    const c = src[i]
    if (' ' === c || '\t' === c || '\r' === c || '\n' === c) {
      i++
    } else if ('/' === c && '/' === src[i + 1]) {
      i += 2
      while (i < src.length && '\n' !== src[i] && '\r' !== src[i]) i++
    } else {
      break
    }
  }
  return i
}

function isIdStart(c: number): boolean {
  return (
    (65 <= c && c <= 90) || // A-Z
    (97 <= c && c <= 122) || // a-z
    95 === c // _
  )
}

function isIdCont(c: number): boolean {
  return (
    (48 <= c && c <= 57) || // 0-9
    (65 <= c && c <= 90) ||
    (97 <= c && c <= 122) ||
    95 === c
  )
}

// Multi-line Zig strings: consecutive lines starting with `\\`.
// Each `\\` line contributes its content verbatim (after the `\\`); lines
// are joined with `\n`.
function buildZonMultiStringMatcher() {
  return function makeZonMultiStringMatcher(cfg: Config, _opts: TabnasOptions) {
    return function zonMultiStringMatcher(lex: Lex) {
      const { pnt } = lex; const src: string = lex.src as unknown as string
      if ('\\' !== src[pnt.sI] || '\\' !== src[pnt.sI + 1]) return undefined

      const startI = pnt.sI
      const startCI = pnt.cI
      let sI = pnt.sI
      let rI = pnt.rI
      const parts: string[] = []

      while ('\\' === src[sI] && '\\' === src[sI + 1]) {
        sI += 2
        const lineStart = sI
        while (sI < src.length && !cfg.line.chars[src[sI]]) sI++
        parts.push(src.substring(lineStart, sI))

        // Consume line terminator (handle \r\n as one).
        if (sI < src.length && cfg.line.chars[src[sI]]) {
          const ch = src[sI]
          if (cfg.line.rowChars[ch]) rI++
          sI++
          if (sI < src.length && '\r' === ch && '\n' === src[sI]) sI++
        }

        // Look for another `\\` continuation after inter-line whitespace.
        let peek = sI
        while (peek < src.length && (src[peek] === ' ' || src[peek] === '\t')) {
          peek++
        }
        if ('\\' !== src[peek] || '\\' !== src[peek + 1]) break
        sI = peek
      }

      const val = parts.join('\n')
      const tsrc = src.substring(startI, sI)
      const tkn = lex.token('#ST', val, tsrc, pnt)
      pnt.sI = sI
      pnt.rI = rI
      pnt.cI = startCI + (sI - startI)
      return tkn
    }
  }
}

// Zig character literal: `'x'`, `'\n'`, `'\x41'`, `'\u{1F600}'`.
// Produces a numeric code point (if charAsNumber) or a one-char string.
function buildZonCharMatcher(charAsNumber: boolean) {
  return function makeZonCharMatcher(_cfg: Config, _opts: TabnasOptions) {
    return function zonCharMatcher(lex: Lex) {
      const { pnt } = lex; const src: string = lex.src as unknown as string
      const { sI, cI } = pnt
      if ('\'' !== src[sI]) return undefined

      let i = sI + 1
      let codepoint: number | null = null

      if ('\\' === src[i]) {
        i++
        const esc = src[i]
        switch (esc) {
          case 'n': codepoint = 10; i++; break
          case 'r': codepoint = 13; i++; break
          case 't': codepoint = 9; i++; break
          case '\\': codepoint = 92; i++; break
          case '\'': codepoint = 39; i++; break
          case '"': codepoint = 34; i++; break
          case '0': codepoint = 0; i++; break
          case 'x': {
            i++
            const hex = src.substring(i, i + 2)
            if (!/^[0-9a-fA-F]{2}$/.test(hex)) return undefined
            codepoint = parseInt(hex, 16)
            i += 2
            break
          }
          case 'u': {
            i++
            if ('{' !== src[i]) return undefined
            i++
            const endI = src.indexOf('}', i)
            if (-1 === endI) return undefined
            const hex = src.substring(i, endI)
            if (!/^[0-9a-fA-F]+$/.test(hex)) return undefined
            codepoint = parseInt(hex, 16)
            i = endI + 1
            break
          }
          default:
            return undefined
        }
      } else if (src[i] && '\'' !== src[i]) {
        codepoint = src.codePointAt(i) as number
        i += codepoint > 0xffff ? 2 : 1
      } else {
        return undefined
      }

      if ('\'' !== src[i]) return undefined
      i++

      const tsrc = src.substring(sI, i)
      const val = charAsNumber ? codepoint : String.fromCodePoint(codepoint!)
      const tkn = lex.token('#NR', val, tsrc, pnt)
      pnt.sI = i
      pnt.cI = cI + (i - sI)
      return tkn
    }
  }
}

// Default option values.
Zon.defaults = {
  charAsNumber: false,
  enumTag: null,
} as ZonOptions

export { Zon }
export type { ZonOptions }
