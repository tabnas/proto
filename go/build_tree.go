/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// Walk the {rule, src, kids} CST into the source-ordered tree, the schema
// proto-ast (alchemy's docs/streaming-plan.md, section 13.1): the file's
// declarations in the order the file writes them, each node an ordered
// map whose first member, type, names it and whose other members follow
// in the order the statement spells them. A scalar is the text the source
// spells, its tokens run together as a CST node's src keeps them: a name,
// a type, a number with its sign, an option's name, and a string literal
// with its quotes and escapes as written. An aggregate option value is its
// fields, [{name, value}], each value text or an aggregate again. An empty
// statement declares nothing and is left out.
//
// The transducer part (alchemy/transduce.alc) makes the same tree's events
// from the parse's rule passes; this walk is the tree a host reads whole,
// and the one the part is held to. It finds each statement the way
// build_descriptor.go does, by the keyword ahead of the node's first child
// (kw), since the ABNF compiler inlines a leading rule into the dispatch
// node that holds it.
//
// Go port of ts/src/build-tree.ts.

package tabnasproto

import (
	"strings"

	tabnas "github.com/tabnas/parser/go"
)

// node is a tree node with its members set in order.
func astNode(members ...any) *tabnas.OrderedMap {
	out := tabnas.NewOrderedMap()
	for i := 0; i+1 < len(members); i += 2 {
		out.Set(members[i].(string), members[i+1])
	}
	return out
}

// astConstant is a constant's value: its text, or an aggregate's fields.
func astConstant(n map[string]any) any {
	if n == nil {
		return ""
	}
	if !strings.HasPrefix(nsrc(n), "{") {
		return nsrc(n)
	}
	fields := []any{}
	for _, k := range childRules(n) {
		if nrule(k) == "messageValueEntry" {
			fields = append(fields, astAggregateField(k))
		}
	}
	return fields
}

// astAggregateField is `messageValueEntry = ident [":"] (strLit 1*strLit /
// constant) [","/";"]`. The leading ident is inlined, so the name is the
// text ahead of the first child, less the colon; adjacent string literals
// are their texts run together, as the entry's src keeps them.
func astAggregateField(e map[string]any) *tabnas.OrderedMap {
	kids := childRules(e)
	name := ""
	if g := gaps(e); len(g) > 0 {
		name = strings.TrimSuffix(g[0], ":")
	}
	var value any
	if len(kids) == 1 && nrule(kids[0]) == "constant" {
		value = astConstant(kids[0])
	} else {
		var b strings.Builder
		for _, k := range kids {
			b.WriteString(nsrc(k))
		}
		value = b.String()
	}
	return astNode("name", name, "value", value)
}

// astOption is `option name = constant;`, as a statement.
func astOption(el map[string]any) *tabnas.OrderedMap {
	cst := child(el, "constant")
	return astNode("type", "option", "name", optionNameOf(el, cst), "value", astConstant(cst))
}

// astOptions is `[name = constant, ...]`: a field's, an enum value's or an
// extension range's options.
func astOptions(opts map[string]any) []any {
	out := []any{}
	for _, fo := range childRules(opts) {
		if nrule(fo) != "fieldOption" {
			continue
		}
		cst := child(fo, "constant")
		out = append(out, astNode("name", optionNameOf(fo, cst), "value", astConstant(cst)))
	}
	return out
}

// astWithOptions appends the statement's `[options]` where it writes them.
func astWithOptions(out *tabnas.OrderedMap, el map[string]any) *tabnas.OrderedMap {
	if opts := child(el, "fieldOptions"); opts != nil {
		out.Set("options", astOptions(opts))
	}
	return out
}

// astField is `[label] fieldType ident "=" fieldNumber [fieldOptions] ";"`.
func astField(n map[string]any) *tabnas.OrderedMap {
	out := astNode("type", "field")
	if label := child(n, "label"); label != nil {
		out.Set("label", nsrc(label))
	}
	out.Set("fieldType", typeTextOf(n))
	out.Set("name", srcOr(child(n, "ident")))
	out.Set("number", srcOr(child(n, "fieldNumber")))
	return astWithOptions(out, n)
}

// astGroup is `[label] "group" ident "=" fieldNumber messageBody`.
func astGroup(n map[string]any) *tabnas.OrderedMap {
	out := astNode("type", "group")
	if label := child(n, "label"); label != nil {
		out.Set("label", nsrc(label))
	}
	out.Set("name", srcOr(child(n, "ident")))
	out.Set("number", srcOr(child(n, "fieldNumber")))
	out.Set("body", astMessageBody(child(n, "messageBody")))
	return out
}

// astMap is `"map" "<" fieldType "," fieldType ">" ident "=" fieldNumber
// [fieldOptions] ";"`.
func astMap(n map[string]any) *tabnas.OrderedMap {
	var types []map[string]any
	for _, k := range childRules(n) {
		if nrule(k) == "fieldType" {
			types = append(types, k)
		}
	}
	at := func(i int) string {
		if i < len(types) {
			return nsrc(types[i])
		}
		return ""
	}
	return astWithOptions(astNode(
		"type", "map",
		"keyType", at(0),
		"valueType", at(1),
		"name", srcOr(child(n, "ident")),
		"number", srcOr(child(n, "fieldNumber")),
	), n)
}

// astRanges is the ranges of a reserved or an extensions statement, each
// from and, where the range writes one, to. The leading range is inlined
// into ranges, so they are read from its text, which no number or max
// spells with a t.
func astRanges(n map[string]any) []any {
	out := []any{}
	if n == nil {
		return out
	}
	for _, part := range strings.Split(nsrc(n), ",") {
		if at := strings.Index(part, "to"); at >= 0 {
			out = append(out, astNode("from", part[:at], "to", part[at+2:]))
		} else {
			out = append(out, astNode("from", part))
		}
	}
	return out
}

// astReservedNames is the names of a reserved statement, each as written:
// a string literal with its quotes, or an identifier. They are read from
// the statement's text, cut at the commas outside a literal.
func astReservedNames(n map[string]any) []any {
	src := srcOr(child(n, "fieldNames"))
	out := []any{}
	from := 0
	for i := 0; i < len(src); i++ {
		c := src[i]
		if c == '"' || c == '\'' || c == '`' {
			for i++; i < len(src) && src[i] != c; i++ {
				if src[i] == '\\' {
					i++
				}
			}
		} else if c == ',' {
			out = append(out, src[from:i])
			from = i + 1
		}
	}
	return append(out, src[from:])
}

func astReserved(n map[string]any) *tabnas.OrderedMap {
	if ranges := child(n, "ranges"); ranges != nil {
		return astNode("type", "reserved", "ranges", astRanges(ranges))
	}
	return astNode("type", "reserved", "names", astReservedNames(n))
}

// astVisible reads the edition-2024 export or local modifier, which keeps
// the message or enum as a child node, with the modifier ahead of it.
func astVisible(el map[string]any, k string) *tabnas.OrderedMap {
	visibility := "local"
	if strings.HasPrefix(k, "export") {
		visibility = "export"
	}
	if m := child(el, "message"); m != nil {
		return astMessage(m, visibility)
	}
	if e := child(el, "enumDef"); e != nil {
		return astEnum(e, visibility)
	}
	return nil
}

func astMessage(n map[string]any, visibility string) *tabnas.OrderedMap {
	out := astNode("type", "message")
	if visibility != "" {
		out.Set("visibility", visibility)
	}
	out.Set("name", srcOr(child(n, "ident")))
	out.Set("body", astMessageBody(child(n, "messageBody")))
	return out
}

func astMessageBody(body map[string]any) []any {
	out := []any{}
	for _, el := range childRules(body) {
		if nrule(el) != "messageElement" {
			continue
		}
		if n := astMessageElement(el); n != nil {
			out = append(out, n)
		}
	}
	return out
}

func astMessageElement(el map[string]any) *tabnas.OrderedMap {
	k := kw(el)
	switch {
	case isOptionStmt(el):
		return astOption(el)
	case strings.HasPrefix(k, "map<"):
		return astMap(el)
	case strings.HasPrefix(k, "oneof"):
		return astOneof(el)
	case strings.HasPrefix(k, "export") || strings.HasPrefix(k, "local"):
		return astVisible(el, k)
	case isGroup(el):
		return astGroup(el)
	case strings.HasPrefix(k, "message"):
		return astMessage(el, "")
	case strings.HasPrefix(k, "enum"):
		return astEnum(el, "")
	case strings.HasPrefix(k, "reserved"):
		return astReserved(el)
	case strings.HasPrefix(k, "extensions"):
		return astWithOptions(astNode("type", "extensions", "ranges", astRanges(child(el, "ranges"))), el)
	case strings.HasPrefix(k, "extend"):
		return astExtend(el)
	case nsrc(el) == ";":
		return nil
	}
	if kids := childRules(el); len(kids) > 0 && (nrule(kids[0]) == "fieldType" || nrule(kids[0]) == "label") {
		return astField(el)
	}
	return nil
}

func astOneof(el map[string]any) *tabnas.OrderedMap {
	body := []any{}
	for _, of := range childRules(el) {
		if nrule(of) != "oneofElement" {
			continue
		}
		switch {
		case isOptionStmt(of):
			body = append(body, astOption(of))
		case nsrc(of) == ";":
		case isGroup(of):
			body = append(body, astGroup(of))
		default:
			body = append(body, astField(of))
		}
	}
	return astNode("type", "oneof", "name", srcOr(child(el, "ident")), "body", body)
}

func astEnum(n map[string]any, visibility string) *tabnas.OrderedMap {
	out := astNode("type", "enum")
	if visibility != "" {
		out.Set("visibility", visibility)
	}
	out.Set("name", srcOr(child(n, "ident")))
	body := []any{}
	for _, el := range childRules(n) {
		if nrule(el) != "enumElement" {
			continue
		}
		// The first child says what the statement is (build_descriptor.go
		// has why): optionName an option, ranges or fieldNames a reserved,
		// fieldNumber a value.
		kids := childRules(el)
		if len(kids) == 0 {
			continue
		}
		first := kids[0]
		switch nrule(first) {
		case "ranges", "fieldNames":
			body = append(body, astReserved(el))
		case "optionName":
			body = append(body, astOption(el))
		case "fieldNumber":
			// ident "=" ["-"] fieldNumber: the name and the sign are in the
			// text ahead of the number node.
			gap := ""
			if g := gapsBefore(el, "fieldNumber"); len(g) > 0 {
				gap = g[0]
			}
			name := gap
			if eq := strings.IndexByte(gap, '='); eq >= 0 {
				name = gap[:eq]
			}
			number := nsrc(first)
			if strings.HasSuffix(gap, "-") {
				number = "-" + number
			}
			body = append(body, astWithOptions(astNode("type", "enumValue", "name", name, "number", number), el))
		}
	}
	out.Set("body", body)
	return out
}

// astExtend is `extend messageType "{" *( field / emptyStmt ) "}"`.
func astExtend(n map[string]any) *tabnas.OrderedMap {
	body := []any{}
	for _, f := range childRules(n) {
		if nrule(f) == "field" || nrule(f) == "messageElement" {
			body = append(body, astField(f))
		}
	}
	return astNode("type", "extend", "extendee", srcOr(child(n, "messageType")), "body", body)
}

func astService(n map[string]any) *tabnas.OrderedMap {
	body := []any{}
	for _, el := range childRules(n) {
		if nrule(el) != "serviceElement" {
			continue
		}
		if strings.HasPrefix(kw(el), "rpc") {
			body = append(body, astRpc(el))
		} else if isOptionStmt(el) {
			body = append(body, astOption(el))
		}
	}
	return astNode("type", "service", "name", srcOr(child(n, "ident")), "body", body)
}

// astRpc is `rpc ident "(" ["stream"] messageType ")" "returns" "("
// ["stream"] messageType ")" ( ";" / "{" *( optionStmt / emptyStmt ) "}" )`.
// Whether each side streams is read from its parenthesised span, as the
// descriptor's walk reads it; the block is there where the statement ends
// in a brace.
func astRpc(el map[string]any) *tabnas.OrderedMap {
	var types []map[string]any
	for _, k := range childRules(el) {
		if nrule(k) == "messageType" {
			types = append(types, k)
		}
	}
	inputType, outputType := "", ""
	if len(types) > 0 {
		inputType = nsrc(types[0])
	}
	if len(types) > 1 {
		outputType = nsrc(types[1])
	}
	spans := parenthesised(nsrc(el))
	out := astNode("type", "rpc", "name", srcOr(child(el, "ident")))
	if len(spans) > 0 && spans[0] == "stream"+inputType {
		out.Set("inputStream", true)
	}
	out.Set("inputType", inputType)
	if len(spans) > 1 && spans[1] == "stream"+outputType {
		out.Set("outputStream", true)
	}
	out.Set("outputType", outputType)
	if strings.HasSuffix(nsrc(el), "}") {
		body := []any{}
		for _, k := range childRules(el) {
			if nrule(k) == "optionStmt" {
				body = append(body, astOption(k))
			}
		}
		out.Set("body", body)
	}
	return out
}

func astTopLevel(def map[string]any) *tabnas.OrderedMap {
	k := kw(def)
	switch {
	case strings.HasPrefix(k, "import"):
		out := astNode("type", "import")
		if modifier := k[len("import"):]; modifier != "" {
			out.Set("modifier", modifier)
		}
		out.Set("path", srcOr(child(def, "strLit")))
		return out
	case strings.HasPrefix(k, "package"):
		return astNode("type", "package", "name", srcOr(child(def, "fullIdent")))
	case isOptionStmt(def):
		return astOption(def)
	case strings.HasPrefix(k, "export") || strings.HasPrefix(k, "local"):
		return astVisible(def, k)
	case strings.HasPrefix(k, "message"):
		return astMessage(def, "")
	case strings.HasPrefix(k, "enum"):
		return astEnum(def, "")
	case strings.HasPrefix(k, "service"):
		return astService(def)
	case strings.HasPrefix(k, "extend"):
		return astExtend(def)
	}
	return nil
}

// buildTree is the file's tree: its declarations in source order. An empty
// file, which parses to no node at all, is a file with an empty body.
func buildTree(proto map[string]any) *tabnas.OrderedMap {
	body := []any{}
	for _, k := range childRules(proto) {
		switch nrule(k) {
		case "syntaxOrEdition":
			kind := "syntax"
			if strings.HasPrefix(kw(k), "edition") {
				kind = "edition"
			}
			body = append(body, astNode("type", kind, "value", srcOr(child(k, "strLit"))))
		case "topLevelDef":
			if n := astTopLevel(k); n != nil {
				body = append(body, n)
			}
		}
	}
	return astNode("type", "file", "body", body)
}

// Two parsers: Make's for a tree, MakeStreaming's for a stream.
//
// Each is a fresh engine with the grammar installed, refuses a document
// nesting past MaxNestingDepth as it reads it, with the code proto_too_deep,
// and declares Lex.Window, so that its ParseReader reads a document a window
// at a time, never whole: see read.go. Both make the same rule passes, token
// for token, and fail the same documents at the same place. Their values
// differ, so neither stands in for the other.
//
// Each keeps the engine's default rewind history, 64 consumed tokens. The
// compiled grammar has no probe, so no parse rewinds, and the one reader of
// that history is recordAggregate, which reads the last token consumed
// (ctx.V1). Every document of the fixtures and the corpus parses alike at 64
// and at 8,192, the bound proto kept through 0.7.0 (history_test.go); a
// longer history would hold the window's text back to its oldest token.

// Make returns the parser for a tree. Its value is the {rule, src, kids} CST
// that ToTree and ToDescriptor walk, as Parse and ParseTree build it; a
// parse from a reader keeps an aggregate's text from the tokens between its
// braces, since a window never holds the whole source. A CST holds the
// whole document: for a stream, take MakeStreaming.
func Make() (*tabnas.Tabnas, error) {
	j := tabnas.Make()
	if err := Proto(j); err != nil {
		return nil, err
	}
	window := true
	j.SetOptions(tabnas.Options{Lex: &tabnas.LexOptions{Window: &window}})
	return j, nil
}

// MakeStreaming returns the parser a host streams a .proto document
// through: its rule passes, which the transducer part
// (alchemy/transduce.alc) reads a declaration at a time, are Make's, and it
// builds no tree. Its grammar is the compiled one without the CST's
// builders, so a parse holds no declaration it has closed, nor any of their
// text, and its value is not the CST: Parse returns nothing a walk takes.
// For a tree, take Make.
func MakeStreaming() (*tabnas.Tabnas, error) {
	gs, err := streamingGrammar()
	if err != nil {
		return nil, err
	}
	j := tabnas.Make()
	if err := j.Grammar(gs); err != nil {
		return nil, err
	}
	startUndefined(j)
	window := true
	j.SetOptions(tabnas.Options{
		Error: map[string]string{tooDeep: tooDeepMessage},
		Hint:  map[string]string{tooDeep: tooDeepHint},
		Lex:   &tabnas.LexOptions{Window: &window},
	})
	j.Sub(readNesting, nil)
	return j, nil
}

// ToTree returns the source-ordered tree of a parsed proto CST: the file's
// declarations in the order the file writes them (the schema proto-ast).
// It is the tree the translation parts read and write.
func ToTree(cst any) *tabnas.OrderedMap {
	root, _ := cst.(map[string]any)
	return buildTree(root)
}

// ParseTree parses a .proto source string to its source-ordered tree in one
// call, refusing a document that nests past MaxNestingDepth first, as Parse
// does.
func ParseTree(src string) (*tabnas.OrderedMap, error) {
	cst, err := parseCST(src)
	if err != nil {
		return nil, err
	}
	return ToTree(cst), nil
}
