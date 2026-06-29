/* Copyright (c) 2025 Richard Rodger, MIT License */

// Package zon is a jsonic plugin that parses Zig Object Notation (ZON)
// syntax. ZON is a data format based on Zig anonymous struct literals.
//
// Example:
//
//	.{
//	    .name = "example",
//	    .version = "0.0.1",
//	    .deps = .{ .foo = .{ .url = "https://..." } },
//	    .paths = .{ "build.zig", "src" },
//	}
package tabnaszon

import (
	"fmt"
	"strconv"
	"strings"
	"sync"
	"unicode/utf8"

	jsonic "github.com/tabnas/jsonic/go"
)

const Version = "0.2.0"

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

// Zon is a jsonic plugin that adds ZON parsing support.
// Options are pre-merged with Defaults by jsonic.UseDefaults.
func Zon(j *jsonic.Jsonic, options map[string]any) error {
	// Guard against re-invocation: SetOptions triggers plugin re-application.
	if j.Decoration("zon-init") != nil {
		return nil
	}
	j.Decorate("zon-init", true)

	charAsNumber := toBool(options["charAsNumber"])
	enumTag := toString(options["enumTag"])

	// If enumTag is set, wrap enum-literal values (produced by zonDot) into
	// `{ [enumTag]: name }` objects. jsonic's relaxed grammar takes ownership
	// of the `@val-bc` (val close) phase via `@val-bc/replace`, which resolves
	// r.Node from the matched token; once a phase is "replaced" the engine
	// SUPPRESSES any `/prepend` on it. So run in the `@val-ac` (after-close)
	// phase — mirroring the TS side — and rewrap the node the close handler
	// produced from the enum token. The val rule is declared in the grammar
	// text, so wireStateActions binds this plain `@val-ac` name as an append
	// to the val rule's AC phase (after jsonic's openval-restore @val-ac).
	refs := map[jsonic.FuncRef]any{}
	if enumTag != "" {
		refs["@val-ac"] = jsonic.StateAction(func(r *jsonic.Rule, _ *jsonic.Context) {
			if r.Child != nil && !jsonic.IsUndefined(r.Child.Node) {
				return
			}
			if r.OS == 0 || r.O0 == nil {
				return
			}
			tkn := r.O0
			if tkn.Use == nil {
				return
			}
			if _, ok := tkn.Use["zonEnum"]; !ok {
				return
			}
			if name, ok := tkn.Val.(string); ok {
				r.Node = map[string]any{enumTag: name}
			}
		})
	}

	gs, err := parseGrammarText(grammarText, refs)
	if err != nil {
		return err
	}
	// All jsonic option overrides live on the grammar object so the plugin
	// applies them atomically alongside its rule alts.
	eqSrc := "="
	gs.Options = &jsonic.Options{
		Rule: &jsonic.RuleOptions{
			// Remove jsonic extensions (implicit maps/lists, top-level commas,
			// path dives). ZON uses explicit struct literals only.
			Exclude: "jsonic,imp",
			Start:   "val",
		},
		Fixed: &jsonic.FixedOptions{
			Token: map[string]*string{
				// Bare `{`, `[`, `]` are not valid in ZON. `.{` is handled by
				// the custom zonDot lex matcher below.
				"#OB": nil,
				"#OS": nil,
				"#CS": nil,
				// `=` replaces `:` as the key/value separator.
				"#CL": &eqSrc,
			},
		},
		TokenSet: map[string][]string{
			// ZON field names are identifiers only.
			"KEY": {"#TX"},
		},
		String: &jsonic.StringOptions{
			Chars:        "\"",
			MultiChars:   "",
			Escape:       map[string]string{"n": "\n", "r": "\r", "t": "\t", "\\": "\\", "\"": "\"", "'": "'"},
			AllowUnknown: boolPtr(false),
		},
		Number: &jsonic.NumberOptions{
			Lex: boolPtr(true),
			Hex: boolPtr(true),
			Oct: boolPtr(true),
			Bin: boolPtr(true),
			Sep: "_",
		},
		Comment: &jsonic.CommentOptions{
			Lex: boolPtr(true),
			Def: map[string]*jsonic.CommentDef{
				"hash":  {Line: true, Start: "#", Lex: boolPtr(false)},
				"slash": {Line: true, Start: "//", Lex: boolPtr(true)},
				"multi": {Line: false, Start: "/*", End: "*/", Lex: boolPtr(false)},
			},
		},
		Value: &jsonic.ValueOptions{
			Lex: boolPtr(true),
			Def: map[string]*jsonic.ValueDef{
				"true":  {Val: true},
				"false": {Val: false},
				"null":  {Val: nil},
			},
		},
		Text: &jsonic.TextOptions{
			// Disabled: the default text matcher would consume identifiers,
			// but in ZON identifiers only appear as `.ident` and are handled
			// by the custom zonDot matcher.
			Lex: boolPtr(false),
		},
		Lex: &jsonic.LexOptions{
			Match: map[string]*jsonic.MatchSpec{
				"zonDot":         {Order: 100000, Make: buildZonDotMatcher()},
				"zonMultiString": {Order: 110000, Make: buildZonMultiStringMatcher()},
				"zonChar":        {Order: 120000, Make: buildZonCharMatcher(charAsNumber)},
			},
		},
	}
	// Tag every alt in this grammar with the 'zon' group so callers can
	// selectively exclude zon alts via rule.exclude.
	setting := &jsonic.GrammarSetting{
		Rule: &jsonic.GrammarSettingRule{
			Alt: &jsonic.GrammarSettingAlt{G: "zon"},
		},
	}
	if err := j.Grammar(gs, setting); err != nil {
		return fmt.Errorf("zon: failed to apply grammar: %w", err)
	}

	return nil
}

// Defaults matches the TS Zon.defaults. Used with jsonic.UseDefaults.
var Defaults = map[string]any{
	"charAsNumber": false,
	"enumTag":      "",
}

// ZonOptions is a typed wrapper for common plugin options.
// Fields are pointers so callers can express "omit" (nil) vs "set".
type ZonOptions struct {
	// CharAsNumber, when true, parses Zig char literals ('x') as numeric
	// code points. When false (default), they are parsed as one-char strings.
	CharAsNumber *bool
	// EnumTag, when non-empty, wraps enum literals (.foo used as value) in
	// map[string]any{<EnumTag>: name} instead of producing the bare string.
	EnumTag string
}

func (o ZonOptions) toMap() map[string]any {
	m := map[string]any{}
	if o.CharAsNumber != nil {
		m["charAsNumber"] = *o.CharAsNumber
	}
	if o.EnumTag != "" {
		m["enumTag"] = o.EnumTag
	}
	return m
}

// MakeJsonic returns a reusable Jsonic instance configured for ZON parsing.
// Use this when parsing multiple ZON strings with the same options.
func MakeJsonic(opts ...ZonOptions) *jsonic.Jsonic {
	j := jsonic.Make()
	var m map[string]any
	if len(opts) > 0 {
		m = opts[0].toMap()
	}
	if err := j.UseDefaults(Zon, Defaults, m); err != nil {
		// Plugin registration errors are programming errors with static
		// inputs; surface them via panic rather than silent misbehavior.
		panic(fmt.Sprintf("zon: plugin initialisation failed: %v", err))
	}
	return j
}

// defaultParser is a lazily-created instance reused by the default (no-option)
// Parse path, so repeated calls don't rebuild the engine and grammar each time
// (building the ZON grammar dominates a parse — see perf_test.go). Parsing
// builds a fresh context per call and only reads instance state, so the shared
// instance is safe for concurrent use. Mirrors @tabnas/json's Parse.
var (
	defaultOnce   sync.Once
	defaultParser *jsonic.Jsonic
)

// Parse parses a ZON string and returns the resulting value. Convenience
// wrapper around MakeJsonic(opts...).Parse(src).
//
// The default (no-options) path reuses a single cached instance, so repeated
// calls don't rebuild the engine + grammar. Option-taking calls still build a
// dedicated instance, since their configuration differs per call.
func Parse(src string, opts ...ZonOptions) (any, error) {
	if len(opts) == 0 {
		defaultOnce.Do(func() { defaultParser = MakeJsonic() })
		return defaultParser.Parse(src)
	}
	return MakeJsonic(opts...).Parse(src)
}

// Custom lex matcher for `.`-prefixed tokens:
//
//	`.{`          -> #OB if followed by `.ident =`, else #OS
//	`.identifier` -> #TX (Val = identifier, Use["zonEnum"] = true)
//
// Runs ahead of the fixed-token matcher so it reliably owns the `.` prefix.
func buildZonDotMatcher() jsonic.MakeLexMatcher {
	return func(_ *jsonic.LexConfig, _ *jsonic.Options) jsonic.LexMatcher {
		return func(lex *jsonic.Lex, _ *jsonic.Rule) *jsonic.Token {
			pnt := lex.Cursor()
			src := lex.Src
			sI := pnt.SI
			if sI >= len(src) || src[sI] != '.' {
				return nil
			}

			// `.{` opens a struct literal. Decide map vs list by peeking.
			if sI+1 < len(src) && src[sI+1] == '{' {
				var tkn *jsonic.Token
				if peekIsMapOpen(src, sI+2) {
					tkn = lex.Token("#OB", jsonic.TinOB, nil, ".{")
				} else {
					tkn = lex.Token("#OS", jsonic.TinOS, nil, ".{")
				}
				pnt.SI = sI + 2
				pnt.CI += 2
				return tkn
			}

			// `.identifier` - field name or enum literal.
			if sI+1 >= len(src) || !isIdStart(src[sI+1]) {
				return nil
			}
			eI := sI + 1
			for eI < len(src) && isIdCont(src[eI]) {
				eI++
			}

			srcText := src[sI:eI]
			name := src[sI+1 : eI]
			tkn := lex.Token("#TX", jsonic.TinTX, name, srcText)
			tkn.Use = map[string]any{"zonEnum": true}
			pnt.SI = eI
			pnt.CI += eI - sI
			return tkn
		}
	}
}

// peekIsMapOpen returns true if the source position inside `.{ ... }` begins
// with `<ws>.ident<ws>=`, meaning a struct/map literal rather than a tuple.
func peekIsMapOpen(src string, start int) bool {
	i := skipInsig(src, start)
	if i >= len(src) || src[i] != '.' {
		return false
	}
	i++
	if i >= len(src) || !isIdStart(src[i]) {
		return false
	}
	i++
	for i < len(src) && isIdCont(src[i]) {
		i++
	}
	i = skipInsig(src, i)
	return i < len(src) && src[i] == '='
}

// skipInsig advances past whitespace, newlines, and `//` line comments.
func skipInsig(src string, i int) int {
	for i < len(src) {
		c := src[i]
		if c == ' ' || c == '\t' || c == '\r' || c == '\n' {
			i++
		} else if c == '/' && i+1 < len(src) && src[i+1] == '/' {
			i += 2
			for i < len(src) && src[i] != '\n' && src[i] != '\r' {
				i++
			}
		} else {
			break
		}
	}
	return i
}

func isIdStart(c byte) bool {
	return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || c == '_'
}

func isIdCont(c byte) bool {
	return (c >= '0' && c <= '9') || (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || c == '_'
}

// Multi-line Zig strings: consecutive lines starting with `\\`. Each `\\`
// line contributes its content verbatim (after the `\\`); lines join with `\n`.
func buildZonMultiStringMatcher() jsonic.MakeLexMatcher {
	return func(cfg *jsonic.LexConfig, _ *jsonic.Options) jsonic.LexMatcher {
		return func(lex *jsonic.Lex, _ *jsonic.Rule) *jsonic.Token {
			pnt := lex.Cursor()
			src := lex.Src
			if pnt.SI+1 >= len(src) || src[pnt.SI] != '\\' || src[pnt.SI+1] != '\\' {
				return nil
			}

			startI := pnt.SI
			startCI := pnt.CI
			sI := pnt.SI
			rI := pnt.RI
			var parts []string

			for sI+1 < len(src) && src[sI] == '\\' && src[sI+1] == '\\' {
				sI += 2
				lineStart := sI
				for sI < len(src) && !cfg.LineChars[rune(src[sI])] {
					sI++
				}
				parts = append(parts, src[lineStart:sI])

				// Consume line terminator (handle \r\n as one).
				if sI < len(src) && cfg.LineChars[rune(src[sI])] {
					ch := src[sI]
					if cfg.RowChars[rune(ch)] {
						rI++
					}
					sI++
					if sI < len(src) && ch == '\r' && src[sI] == '\n' {
						sI++
					}
				}

				// Look for another `\\` continuation after whitespace.
				peek := sI
				for peek < len(src) && (src[peek] == ' ' || src[peek] == '\t') {
					peek++
				}
				if peek+1 >= len(src) || src[peek] != '\\' || src[peek+1] != '\\' {
					break
				}
				sI = peek
			}

			val := strings.Join(parts, "\n")
			tsrc := src[startI:sI]
			tkn := lex.Token("#ST", jsonic.TinST, val, tsrc)
			pnt.SI = sI
			pnt.RI = rI
			pnt.CI = startCI + (sI - startI)
			return tkn
		}
	}
}

// Zig character literal: `'x'`, `'\n'`, `'\x41'`, `'\u{1F600}'`.
// Produces a numeric code point (if charAsNumber) or a one-char string.
func buildZonCharMatcher(charAsNumber bool) jsonic.MakeLexMatcher {
	return func(_ *jsonic.LexConfig, _ *jsonic.Options) jsonic.LexMatcher {
		return func(lex *jsonic.Lex, _ *jsonic.Rule) *jsonic.Token {
			pnt := lex.Cursor()
			src := lex.Src
			sI := pnt.SI
			if sI >= len(src) || src[sI] != '\'' {
				return nil
			}

			i := sI + 1
			if i >= len(src) {
				return nil
			}

			var codepoint int

			if src[i] == '\\' {
				i++
				if i >= len(src) {
					return nil
				}
				switch src[i] {
				case 'n':
					codepoint = '\n'
					i++
				case 'r':
					codepoint = '\r'
					i++
				case 't':
					codepoint = '\t'
					i++
				case '\\':
					codepoint = '\\'
					i++
				case '\'':
					codepoint = '\''
					i++
				case '"':
					codepoint = '"'
					i++
				case '0':
					codepoint = 0
					i++
				case 'x':
					i++
					if i+2 > len(src) {
						return nil
					}
					hex := src[i : i+2]
					if !isHex(hex) {
						return nil
					}
					n, err := strconv.ParseInt(hex, 16, 32)
					if err != nil {
						return nil
					}
					codepoint = int(n)
					i += 2
				case 'u':
					i++
					if i >= len(src) || src[i] != '{' {
						return nil
					}
					i++
					end := strings.IndexByte(src[i:], '}')
					if end < 0 {
						return nil
					}
					end += i
					hex := src[i:end]
					if !isHex(hex) {
						return nil
					}
					n, err := strconv.ParseInt(hex, 16, 32)
					if err != nil {
						return nil
					}
					codepoint = int(n)
					i = end + 1
				default:
					return nil
				}
			} else if src[i] != '\'' {
				r, size := utf8.DecodeRuneInString(src[i:])
				if r == utf8.RuneError && size <= 1 {
					return nil
				}
				codepoint = int(r)
				i += size
			} else {
				return nil
			}

			if i >= len(src) || src[i] != '\'' {
				return nil
			}
			i++

			var val any
			if charAsNumber {
				val = float64(codepoint)
			} else {
				val = string(rune(codepoint))
			}
			tsrc := src[sI:i]
			tkn := lex.Token("#NR", jsonic.TinNR, val, tsrc)
			pnt.SI = i
			pnt.CI += i - sI
			return tkn
		}
	}
}

func isHex(s string) bool {
	if len(s) == 0 {
		return false
	}
	for i := 0; i < len(s); i++ {
		c := s[i]
		if !((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F')) {
			return false
		}
	}
	return true
}

// parseGrammarText parses grammar text into a GrammarSpec with refs attached.
func parseGrammarText(text string, refs map[jsonic.FuncRef]any) (*jsonic.GrammarSpec, error) {
	parsed, err := jsonic.Make().Parse(text)
	if err != nil {
		return nil, fmt.Errorf("zon: failed to parse grammar text: %w", err)
	}
	parsedMap, ok := parsed.(map[string]any)
	if !ok {
		return nil, fmt.Errorf("zon: grammar text did not parse to a map")
	}
	gs := &jsonic.GrammarSpec{Ref: refs}
	ruleMap, ok := parsedMap["rule"].(map[string]any)
	if !ok {
		return gs, nil
	}
	gs.Rule = make(map[string]*jsonic.GrammarRuleSpec, len(ruleMap))
	for name, rDef := range ruleMap {
		rd, ok := rDef.(map[string]any)
		if !ok {
			continue
		}
		grs := &jsonic.GrammarRuleSpec{}
		if openDef, ok := rd["open"]; ok {
			grs.Open = buildGrammarAlts(openDef)
		}
		if closeDef, ok := rd["close"]; ok {
			grs.Close = buildGrammarAlts(closeDef)
		}
		gs.Rule[name] = grs
	}
	return gs, nil
}

// buildGrammarAlts converts a parsed-jsonic alt array into []*GrammarAltSpec.
func buildGrammarAlts(def any) []*jsonic.GrammarAltSpec {
	arr, ok := def.([]any)
	if !ok {
		return nil
	}
	alts := make([]*jsonic.GrammarAltSpec, 0, len(arr))
	for _, item := range arr {
		m, ok := item.(map[string]any)
		if !ok {
			alts = append(alts, &jsonic.GrammarAltSpec{})
			continue
		}
		ga := &jsonic.GrammarAltSpec{}
		if s, ok := m["s"]; ok {
			switch sv := s.(type) {
			case string:
				ga.S = sv
			case []any:
				strs := make([]string, len(sv))
				for i, v := range sv {
					strs[i], _ = v.(string)
				}
				ga.S = strs
			}
		}
		if b, ok := m["b"]; ok {
			switch bv := b.(type) {
			case float64:
				ga.B = int(bv)
			case int:
				ga.B = bv
			}
		}
		if p, ok := m["p"].(string); ok {
			ga.P = p
		}
		if r, ok := m["r"].(string); ok {
			ga.R = r
		}
		if a, ok := m["a"].(string); ok {
			ga.A = jsonic.FuncRef(a)
		}
		if c, ok := m["c"]; ok {
			switch cv := c.(type) {
			case string:
				ga.C = cv
			case map[string]any:
				ga.C = cv
			}
		}
		if u, ok := m["u"].(map[string]any); ok {
			ga.U = u
		}
		if g, ok := m["g"].(string); ok {
			ga.G = g
		}
		alts = append(alts, ga)
	}
	return alts
}

func toBool(v any) bool {
	b, _ := v.(bool)
	return b
}

func toString(v any) string {
	s, _ := v.(string)
	return s
}

func boolPtr(b bool) *bool {
	return &b
}
