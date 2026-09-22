/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// Walk the {rule, src, kids} CST produced by the proto ABNF grammar and
// assemble a FileDescriptorProto-shaped value. Lexical atoms arrive as
// whole-word tokens, and abnf's leading-ref inlining means the specific
// statement rule (message / field / enum …) is folded into the enclosing
// topLevelDef / messageElement dispatch node; we recover the statement kind
// from the keyword that precedes the node's first child (kw).
//
// Go port of ts/src/build-descriptor.ts. The CST node is the engine's
// map[string]any{"rule": string, "src": string, "kids": []any}.

package tabnasproto

import (
	"regexp"
	"strconv"
	"strings"
)

// ---- CST node helpers -----------------------------------------------------

func nrule(n map[string]any) string { s, _ := n["rule"].(string); return s }
func nsrc(n map[string]any) string  { s, _ := n["src"].(string); return s }

func nkids(n map[string]any) []map[string]any {
	if n == nil {
		return nil
	}
	raw, _ := n["kids"].([]any)
	out := make([]map[string]any, 0, len(raw))
	for _, k := range raw {
		if m, ok := k.(map[string]any); ok {
			out = append(out, m)
		}
	}
	return out
}

// childRules returns the children that are real rule nodes (terminals fold
// into src). The TS `R(n)`.
func childRules(n map[string]any) []map[string]any {
	var out []map[string]any
	for _, k := range nkids(n) {
		if nrule(k) != "" {
			out = append(out, k)
		}
	}
	return out
}

// kw is the keyword(s) consumed before this node's first child — the part of
// src ahead of the first child's src. For `message Foo {…}` the first child
// is `Foo`, so kw is `message`; for an unlabelled field it is "".
func kw(n map[string]any) string {
	k := childRules(n)
	if len(k) == 0 {
		return nsrc(n)
	}
	i := strings.Index(nsrc(n), nsrc(k[0]))
	if i <= 0 {
		return ""
	}
	return nsrc(n)[:i]
}

// child finds the first rule child with the given rule name (nil if absent).
func child(n map[string]any, rule string) map[string]any {
	for _, k := range childRules(n) {
		if nrule(k) == rule {
			return k
		}
	}
	return nil
}

// gaps returns the source text immediately ahead of each rule child, one
// entry per member of childRules(n) and in the same order.
//
// src is the node's tokens run together, so every child's text is a
// contiguous slice of it — but SEARCHING for that text can land on the wrong
// copy. In the enum element `A1=1;` the fieldNumber node's `1` also occurs
// inside the name ahead of it, and in `rpc M (stream A)` the `stream`
// modifier is a bare terminal that never becomes a node. The scan therefore
// runs from the END: each child is bounded above by the child after it, so
// the last occurrence below that bound is the child itself. That leaves the
// text between two children exactly, which is where the grammar's own
// terminals (`=`, `-`, `(`, `stream`, `returns`) are, and reading one of
// those is a structural question answered from the tree rather than a
// pattern matched against the whole statement.
func gaps(n map[string]any) []string {
	kids := childRules(n)
	src := nsrc(n)
	at := make([]int, len(kids))
	hi := len(src)
	for i := len(kids) - 1; i >= 0; i-- {
		text := nsrc(kids[i])
		found := -1
		if text != "" && len(text) <= hi {
			found = strings.LastIndex(src[:hi], text)
		}
		if found < 0 {
			found = hi
		}
		at[i] = found
		hi = found
	}
	out := make([]string, len(kids))
	end := 0
	for i, k := range kids {
		start := at[i]
		if start < end {
			start = end
		}
		out[i] = src[end:start]
		end = start + len(nsrc(k))
	}
	return out
}

// gapsBefore returns the gaps ahead of the children carrying this rule name,
// in source order.
func gapsBefore(n map[string]any, rule string) []string {
	kids := childRules(n)
	g := gaps(n)
	var out []string
	for i, k := range kids {
		if nrule(k) == rule {
			out = append(out, g[i])
		}
	}
	return out
}

// ---- small helpers --------------------------------------------------------

var (
	wsRe       = regexp.MustCompile(`\s+`)
	unquoteRe  = regexp.MustCompile(`(?s)^["'](.*)["']$`)
	quoteRe    = regexp.MustCompile(`^["']`)
	numLeadRe  = regexp.MustCompile(`(?i)^[-+]?(?:\d|\.\d|0x|0o|0b)`)
	rangeRe    = regexp.MustCompile(`^(-?\d+)(?:to(-?\d+|max))?$`)
	plusPrefix = regexp.MustCompile(`^\+`)
	// One reserved name: a double- or single-quoted literal, or an identifier.
	reservedNameRe = regexp.MustCompile(
		`"((?:[^"\\]|\\.)*)"|'((?:[^'\\]|\\.)*)'|([A-Za-z_][A-Za-z0-9_]*)`)
)

func stripWS(s string) string { return wsRe.ReplaceAllString(s, "") }

func unquote(s string) string {
	if m := unquoteRe.FindStringSubmatch(s); m != nil {
		return m[1]
	}
	return s
}

func srcOr(n map[string]any) string {
	if n == nil {
		return ""
	}
	return nsrc(n)
}

func srcAt(ns []map[string]any, i int) string {
	if i < len(ns) {
		return nsrc(ns[i])
	}
	return ""
}

// toInt parses an integer-valued token (field/enum numbers, ranges). It must
// accept every literal form the lexer hands back — decimal, but also the
// 0x / 0o / 0b integer literals protoc allows for enum and field numbers
// (`HEX_MAX = 0x7FFFFFFF;`) — so it goes through the same jsNumber path the
// TS side's Number() does.
func toInt(s string) int {
	s = strings.TrimSpace(s)
	if n, err := strconv.Atoi(s); err == nil {
		return n
	}
	if f, ok := jsNumber(s); ok {
		return int(f)
	}
	return 0
}

func numOr(n map[string]any) int {
	if n == nil {
		return 0
	}
	return toInt(nsrc(n))
}

// jsNumber mirrors JS Number(s) closely enough for option constants: decimal,
// float, exponent, and 0x/0o/0b integer literals. Returns ok=false (so the
// caller keeps the raw text) when the text is not numeric.
func jsNumber(s string) (float64, bool) {
	t := plusPrefix.ReplaceAllString(s, "")
	neg := false
	u := t
	if strings.HasPrefix(u, "-") {
		neg, u = true, u[1:]
	}
	lu := strings.ToLower(u)
	base := 0
	switch {
	case strings.HasPrefix(lu, "0x"):
		base, lu = 16, lu[2:]
	case strings.HasPrefix(lu, "0o"):
		base, lu = 8, lu[2:]
	case strings.HasPrefix(lu, "0b"):
		base, lu = 2, lu[2:]
	}
	if base != 0 {
		iv, err := strconv.ParseInt(lu, base, 64)
		if err != nil {
			return 0, false
		}
		f := float64(iv)
		if neg {
			f = -f
		}
		return f, true
	}
	f, err := strconv.ParseFloat(t, 64)
	if err != nil {
		return 0, false
	}
	return f, true
}

// ---- constants / option values --------------------------------------------

func constantValue(n map[string]any) OptionValue {
	s := nsrc(n)
	if s == "true" {
		return true
	}
	if s == "false" {
		return false
	}
	if quoteRe.MatchString(s) {
		return unquote(s)
	}
	if numLeadRe.MatchString(s) {
		if num, ok := jsNumber(s); ok {
			return num
		}
	}
	return s // identifier (enum value name, inf, nan, …) kept verbatim
}

// optionNameOf reads the option name in an `optionName "=" constant` statement.
//
// optionStmt keeps optionName as a child node, so use it when present. Inside
// fieldOption abnf inlines the leading optionNamePart, so there the name has
// to be read out of src — as everything BEFORE the trailing `"=" constant`.
// Anchoring at the end matters: `option (file_opt1) = 1;` would otherwise find
// the value `1` inside the name `file_opt1`.
func optionNameOf(stmt, value map[string]any) string {
	if named := child(stmt, "optionName"); named != nil {
		return nsrc(named)
	}
	if value == nil {
		return ""
	}
	s := strings.TrimSuffix(nsrc(stmt), ";")
	tail := "=" + nsrc(value)
	if strings.HasSuffix(s, tail) {
		return s[:len(s)-len(tail)]
	}
	i := strings.Index(s, nsrc(value))
	if i <= 0 {
		return ""
	}
	return strings.TrimSuffix(s[:i], "=")
}

// pseudoOptions is a field's option set with `json_name` and `default` split
// out: protoc lifts those two out of the option set into descriptor fields.
type pseudoOptions struct {
	options      map[string]OptionValue
	jsonName     string
	defaultValue string
	hasJSONName  bool
	hasDefault   bool
}

// readFieldOptions reads `"[" fieldOption *( "," fieldOption ) "]"`.
func readFieldOptions(opts map[string]any) pseudoOptions {
	var out pseudoOptions
	if opts == nil {
		return out
	}
	m := map[string]OptionValue{}
	for _, fo := range childRules(opts) {
		cst := child(fo, "constant")
		name := optionNameOf(fo, cst)
		if cst == nil || name == "" {
			continue
		}
		switch name {
		case "json_name":
			out.jsonName, out.hasJSONName = unquote(nsrc(cst)), true
		case "default":
			out.defaultValue, out.hasDefault = unquote(nsrc(cst)), true
		default:
			m[name] = constantValue(cst)
		}
	}
	if len(m) > 0 {
		out.options = m
	}
	return out
}

// plainOptions is the option map for the places that cannot carry
// json_name / default — extension ranges, enum values.
func plainOptions(opts map[string]any) map[string]OptionValue {
	return readFieldOptions(opts).options
}

// features is the subset of an option map whose names are rooted at
// `features`; those govern a map entry's key/value fields too.
func features(opts map[string]OptionValue) map[string]OptionValue {
	if opts == nil {
		return nil
	}
	out := map[string]OptionValue{}
	for k, v := range opts {
		if k == "features" || strings.HasPrefix(k, "features.") {
			out[k] = v
		}
	}
	if len(out) == 0 {
		return nil
	}
	return out
}

// ---- fields ---------------------------------------------------------------

func fieldLabel(labelNode map[string]any, version ProtoVersion) (label string, proto3Optional bool) {
	lab := ""
	if labelNode != nil {
		lab = nsrc(labelNode)
	}
	switch lab {
	case "required":
		return "LABEL_REQUIRED", false
	case "repeated":
		return "LABEL_REPEATED", false
	case "optional":
		if version == "proto3" {
			return "LABEL_OPTIONAL", true
		}
		return "LABEL_OPTIONAL", false
	}
	// Implicit label.
	return "LABEL_OPTIONAL", false
}

func fieldTypeName(typeText string) (typ, typeName string) {
	// A LEADING DOT makes the reference fully qualified, so `.int32` names a
	// type called `int32` at the root and is never the scalar `int32`. protoc
	// accepts `message int32 {}` and records `.int32` as the field's type
	// name; only an unqualified spelling may reach the scalar table.
	if !strings.HasPrefix(typeText, ".") {
		if scalar, ok := ScalarTypes[typeText]; ok {
			return scalar, ""
		}
	}
	// A named reference: could be a message OR an enum, and telling them apart
	// needs symbol resolution this parser deliberately does not do. protoc
	// leaves Type unset here too, so record only TypeName, as written.
	return "", typeText
}

// typeNodeOf finds the field type node. Normally a `fieldType` child; but when
// `fieldType` is a leading ref (an unlabelled oneof member `string x = 1;`)
// abnf inlines it, so the type surfaces as a bare `messageType` / `fullIdent`.
func typeNodeOf(n map[string]any) map[string]any {
	if t := child(n, "fieldType"); t != nil {
		return t
	}
	if t := child(n, "messageType"); t != nil {
		return t
	}
	return child(n, "fullIdent")
}

// applyFieldOptions attaches an option list to a field, splitting out the
// json_name / default pseudo-options.
func applyFieldOptions(f *FieldDescriptorProto, opts map[string]any) {
	po := readFieldOptions(opts)
	if po.hasJSONName {
		f.JsonName = po.jsonName
	}
	if po.hasDefault {
		f.DefaultValue = po.defaultValue
	}
	if po.options != nil {
		f.Options = po.options
	}
}

func buildField(n map[string]any, version ProtoVersion) FieldDescriptorProto {
	label := child(n, "label")
	typeNode := typeNodeOf(n)
	name := child(n, "ident")
	number := child(n, "fieldNumber")

	lbl, p3 := fieldLabel(label, version)
	typ, typeName := fieldTypeName(srcOr(typeNode))
	f := FieldDescriptorProto{
		Name:           srcOr(name),
		Number:         numOr(number),
		Label:          lbl,
		Proto3Optional: p3,
		Type:           typ,
		TypeName:       typeName,
	}
	applyFieldOptions(&f, child(n, "fieldOptions"))
	return f
}

// buildGroup expands a group into a field plus an implicit nested message.
// protoc lowercases the field name and keeps the declared name for the
// message and for TypeName:
//
//	`optional group TestGroup = 1 { … }`
//	  -> field { name: "testgroup", type: TYPE_GROUP, typeName: "TestGroup" }
//	  -> nestedType { name: "TestGroup", … }
func buildGroup(n map[string]any, version ProtoVersion, into *DescriptorProto) FieldDescriptorProto {
	groupName := srcOr(child(n, "ident"))
	into.NestedType = append(into.NestedType,
		messageFromBody(groupName, child(n, "messageBody"), version))

	lbl, _ := fieldLabel(child(n, "label"), version)
	f := FieldDescriptorProto{
		Name:     strings.ToLower(groupName),
		Number:   numOr(child(n, "fieldNumber")),
		Label:    lbl,
		Type:     "TYPE_GROUP",
		TypeName: groupName,
	}
	applyFieldOptions(&f, child(n, "fieldOptions"))
	return f
}

// isGroup: a group is the one construct with BOTH a field number and a message
// body; `message` has a body but no number, `map`/`field` a number but no body.
func isGroup(n map[string]any) bool {
	return child(n, "fieldNumber") != nil && child(n, "messageBody") != nil
}

// mapEntryName is protoc's map-entry name: strip `_`, upper-case the letter
// that follows (and the first letter), then append "Entry". `map_field` ->
// `MapFieldEntry`.
func mapEntryName(fname string) string {
	b := make([]byte, 0, len(fname)+5)
	capNext := true
	for i := 0; i < len(fname); i++ {
		c := fname[i]
		if c == '_' {
			capNext = true
			continue
		}
		if capNext && c >= 'a' && c <= 'z' {
			c -= 'a' - 'A'
		}
		capNext = false
		b = append(b, c)
	}
	return string(b) + "Entry"
}

// buildMapField turns `map<K,V> name = N;` into a repeated message field whose
// type is a synthesised nested `<Name>Entry` message with mapEntry=true.
func buildMapField(n map[string]any, version ProtoVersion, into *DescriptorProto) FieldDescriptorProto {
	var types []map[string]any
	for _, k := range childRules(n) {
		if nrule(k) == "fieldType" {
			types = append(types, k)
		}
	}
	name := child(n, "ident")
	number := child(n, "fieldNumber")
	keyText := srcAt(types, 0)
	valText := srcAt(types, 1)
	fname := srcOr(name)
	entryName := mapEntryName(fname)

	kType, kTypeName := fieldTypeName(keyText)
	vType, vTypeName := fieldTypeName(valText)
	entry := DescriptorProto{
		Name: entryName,
		Field: []FieldDescriptorProto{
			{Name: "key", Number: 1, Label: "LABEL_OPTIONAL", Type: kType, TypeName: kTypeName},
			{Name: "value", Number: 2, Label: "LABEL_OPTIONAL", Type: vType, TypeName: vTypeName},
		},
		NestedType: []DescriptorProto{}, EnumType: []EnumDescriptorProto{},
		OneofDecl: []OneofDescriptorProto{}, Extension: []FieldDescriptorProto{},
		Options: map[string]OptionValue{"mapEntry": true},
	}

	f := FieldDescriptorProto{
		Name:     fname,
		Number:   numOr(number),
		Label:    "LABEL_REPEATED",
		TypeName: entryName,
	}
	applyFieldOptions(&f, child(n, "fieldOptions"))

	// `features` on a map field govern the synthesised entry's key and value
	// fields too, so protoc copies them down. Nothing else is propagated.
	if feat := features(f.Options); feat != nil {
		entry.Field[0].Options = feat
		entry.Field[1].Options = feat
	}
	into.NestedType = append(into.NestedType, entry)
	return f
}

// ---- enums ----------------------------------------------------------------

func buildEnum(n map[string]any) EnumDescriptorProto {
	e := EnumDescriptorProto{Name: srcOr(child(n, "ident")), Value: []EnumValueDescriptorProto{}}
	for _, el := range childRules(n) {
		if nrule(el) != "enumElement" {
			continue
		}
		// An enum body is the one place where the statement kind CANNOT come
		// from the leading keyword: enumField inlines its name, so kw on
		// `optionX = 1;` is `optionX=` and a prefix test reads it as an option
		// statement. The first child's rule name says what the statement is
		// without guessing — optionName for an option, ranges or fieldNames
		// for a `reserved`, fieldNumber for a value.
		kids := childRules(el)
		if len(kids) == 0 {
			continue
		}
		first := kids[0]
		switch nrule(first) {
		case "ranges", "fieldNames":
			// Enum reserved ranges are INCLUSIVE and span the whole int32 space.
			addReserved(el, &e.ReservedRange, &e.ReservedName,
				rangeOpts{exclusive: false, max: MaxEnumNumber})
			continue
		case "optionName":
			if e.Options == nil {
				e.Options = map[string]OptionValue{}
			}
			for kk, vv := range optionFrom(el) {
				e.Options[kk] = vv
			}
			continue
		case "fieldNumber":
		default:
			continue
		}
		// enumField: ident "=" ["-"] fieldNumber. Both the name and the sign
		// are in the text ahead of the number node, and nowhere else: reading
		// the whole element instead takes an OPTION's minus sign as the
		// value's, so `A = 1 [(x) = -2]` became -1.
		gap := ""
		if g := gapsBefore(el, "fieldNumber"); len(g) > 0 {
			gap = g[0]
		}
		name := gap
		if i := strings.Index(name, "="); i >= 0 {
			name = name[:i]
		}
		if name != "" {
			num := toInt(nsrc(first))
			if strings.HasSuffix(gap, "-") {
				num = -num
			}
			e.Value = append(e.Value, EnumValueDescriptorProto{
				Name: name, Number: num, Options: plainOptions(child(el, "fieldOptions")),
			})
		}
	}
	return e
}

// ---- reserved / extensions ranges -----------------------------------------

// rangeOpts says how a range's `end` is written down: message extension and
// reserved ranges are half-open (end exclusive), enum reserved ranges closed.
type rangeOpts struct {
	exclusive bool
	max       int
}

// ranges parses `range *( "," range )`. The leading range is inlined into the
// node's src, so parse the (whitespace-stripped) text rather than kids.
func ranges(rangesNode map[string]any, ro rangeOpts) []Range {
	if rangesNode == nil {
		return nil
	}
	var out []Range
	for _, part := range strings.Split(stripWS(nsrc(rangesNode)), ",") {
		m := rangeRe.FindStringSubmatch(part)
		if m == nil {
			continue
		}
		start := toInt(m[1])
		var end int
		switch {
		case m[2] == "":
			end = start
			if ro.exclusive {
				end = start + 1
			}
		case m[2] == "max":
			end = ro.max
		default:
			end = toInt(m[2])
			if ro.exclusive {
				end++
			}
		}
		out = append(out, Range{Start: start, End: end})
	}
	return out
}

// reservedNames reads the reserved-name list. Both the leading strLit/ident
// and (for a single-item list) the whole list can be inlined into src, so read
// the names out of the statement text; whole-word tokens make that
// unambiguous.
func reservedNames(n map[string]any) []string {
	body := strings.TrimSuffix(strings.TrimPrefix(nsrc(n), "reserved"), ";")
	var out []string
	for _, m := range reservedNameRe.FindAllStringSubmatch(body, -1) {
		switch {
		case m[1] != "":
			out = append(out, m[1])
		case m[2] != "":
			out = append(out, m[2])
		default:
			out = append(out, m[3])
		}
	}
	return out
}

func addReserved(n map[string]any, rr *[]Range, rnames *[]string, ro rangeOpts) {
	if rn := child(n, "ranges"); rn != nil {
		*rr = append(*rr, ranges(rn, ro)...)
		return
	}
	*rnames = append(*rnames, reservedNames(n)...)
}

// ---- messages -------------------------------------------------------------

func buildMessage(n map[string]any, version ProtoVersion) DescriptorProto {
	return messageFromBody(srcOr(child(n, "ident")), child(n, "messageBody"), version)
}

func messageFromBody(name string, body map[string]any, version ProtoVersion) DescriptorProto {
	msg := DescriptorProto{
		Name:  name,
		Field: []FieldDescriptorProto{}, NestedType: []DescriptorProto{},
		EnumType: []EnumDescriptorProto{}, OneofDecl: []OneofDescriptorProto{},
		Extension: []FieldDescriptorProto{},
	}
	var elements []map[string]any
	if body != nil {
		for _, el := range childRules(body) {
			if nrule(el) == "messageElement" {
				elements = append(elements, el)
			}
		}
	}

	// Options first: message_set_wire_format changes what `to max` means in an
	// extension/reserved range, and protoc applies it wherever the option sits.
	for _, el := range elements {
		if isOptionStmt(el) {
			if msg.Options == nil {
				msg.Options = map[string]OptionValue{}
			}
			for kk, vv := range optionFrom(el) {
				msg.Options[kk] = vv
			}
		}
	}
	ro := rangeOpts{exclusive: true, max: MaxFieldNumberEnd}
	if msg.Options["message_set_wire_format"] == true {
		ro.max = MaxMessageSetEnd
	}

	for _, el := range elements {
		if !isOptionStmt(el) {
			addMessageElement(el, version, &msg, ro)
		}
	}
	generateSyntheticOneofs(&msg)
	return msg
}

// isOptionStmt reports an `option` statement, as opposed to an
// `optional`-labelled field (whose kw is empty because the label starts src).
func isOptionStmt(el map[string]any) bool {
	return strings.HasPrefix(kw(el), "option")
}

// generateSyntheticOneofs mirrors protoc: a single-field oneof is synthesised
// for every proto3 explicit `optional` field, appended after the declared
// oneofs. The name is the field name prefixed with `_`, then prefixed with `X`
// until unique.
func generateSyntheticOneofs(msg *DescriptorProto) {
	names := map[string]bool{}
	for _, f := range msg.Field {
		names[f.Name] = true
	}
	for _, o := range msg.OneofDecl {
		names[o.Name] = true
	}
	for i := range msg.Field {
		f := &msg.Field[i]
		if !f.Proto3Optional || f.OneofIndex != nil {
			continue
		}
		oneofName := "_" + f.Name
		for names[oneofName] {
			oneofName = "X" + oneofName
		}
		names[oneofName] = true
		idx := len(msg.OneofDecl)
		f.OneofIndex = &idx
		msg.OneofDecl = append(msg.OneofDecl, OneofDescriptorProto{Name: oneofName})
	}
}

func addMessageElement(el map[string]any, version ProtoVersion, msg *DescriptorProto, ro rangeOpts) {
	k := kw(el)
	rs := childRules(el)
	var first map[string]any
	if len(rs) > 0 {
		first = rs[0]
	}
	switch {
	case strings.HasPrefix(k, "map<"):
		msg.Field = append(msg.Field, buildMapField(el, version, msg))
		return
	case strings.HasPrefix(k, "oneof"):
		addOneof(el, version, msg)
		return
	case strings.HasPrefix(k, "export"), strings.HasPrefix(k, "local"):
		// edition 2024 symbol visibility wraps the message/enum as a child node.
		addVisible(el, k, version, &msg.NestedType, &msg.EnumType)
		return
	case isGroup(el):
		msg.Field = append(msg.Field, buildGroup(el, version, msg))
		return
	case strings.HasPrefix(k, "message"):
		msg.NestedType = append(msg.NestedType, buildMessage(el, version))
		return
	case strings.HasPrefix(k, "enum"):
		msg.EnumType = append(msg.EnumType, buildEnum(el))
		return
	case strings.HasPrefix(k, "reserved"):
		addReserved(el, &msg.ReservedRange, &msg.ReservedName, ro)
		return
	case strings.HasPrefix(k, "extensions"):
		rs := ranges(child(el, "ranges"), ro)
		// A compound `extensions 2, 9 to 11 [(i) = 5];` puts the options on
		// every range, as protoc does.
		if opts := plainOptions(child(el, "fieldOptions")); opts != nil {
			for i := range rs {
				rs[i].Options = opts
			}
		}
		msg.ExtensionRange = append(msg.ExtensionRange, rs...)
		return
	case strings.HasPrefix(k, "extend"):
		addExtend(el, version, &msg.Extension)
		return
	}
	if nsrc(el) == ";" {
		return // emptyStmt
	}
	// No keyword and a fieldType/label lead => a field.
	if first != nil && (nrule(first) == "fieldType" || nrule(first) == "label") {
		msg.Field = append(msg.Field, buildField(el, version))
	}
}

// addVisible handles an edition 2024 `export` / `local` message or enum
// declaration; the wrapped message/enumDef stays a child node.
func addVisible(el map[string]any, k string, version ProtoVersion,
	messages *[]DescriptorProto, enums *[]EnumDescriptorProto) {
	vis := VisibilityLocal
	if strings.HasPrefix(k, "export") {
		vis = VisibilityExport
	}
	if m := child(el, "message"); m != nil {
		d := buildMessage(m, version)
		d.Visibility = vis
		*messages = append(*messages, d)
	} else if e := child(el, "enumDef"); e != nil {
		d := buildEnum(e)
		d.Visibility = vis
		*enums = append(*enums, d)
	}
}

func addOneof(el map[string]any, version ProtoVersion, msg *DescriptorProto) {
	name := srcOr(child(el, "ident"))
	index := len(msg.OneofDecl)
	// protoc gives a oneof its own OneofOptions, and every other declaration
	// kind here keeps its option statements, so the oneof's are recorded
	// rather than dropped.
	msg.OneofDecl = append(msg.OneofDecl, OneofDescriptorProto{Name: name})
	// Held aside rather than written through a pointer into OneofDecl: the
	// loop below builds group members, which append to the message, and a
	// re-allocated slice would leave that pointer addressing the old array.
	var declOptions map[string]OptionValue
	for _, of := range childRules(el) {
		if nrule(of) != "oneofElement" {
			continue
		}
		if strings.HasPrefix(kw(of), "option") {
			if declOptions == nil {
				declOptions = map[string]OptionValue{}
			}
			for kk, vv := range optionFrom(of) {
				declOptions[kk] = vv
			}
			continue
		}
		if nsrc(of) == ";" {
			continue
		}
		var f FieldDescriptorProto
		if isGroup(of) {
			f = buildGroup(of, version, msg)
		} else {
			f = buildField(of, version)
		}
		idx := index
		f.OneofIndex = &idx
		f.Proto3Optional = false // explicit oneof members aren't proto3-optional
		msg.Field = append(msg.Field, f)
	}
	msg.OneofDecl[index].Options = declOptions
}

func addExtend(el map[string]any, version ProtoVersion, into *[]FieldDescriptorProto) {
	// extend messageType "{" *field "}" — fields inline as messageElement-like.
	extendee := srcOr(child(el, "messageType"))
	for _, f := range childRules(el) {
		if nrule(f) == "field" || nrule(f) == "messageElement" {
			fd := buildField(f, version)
			fd.Extendee = extendee
			*into = append(*into, fd)
		}
	}
}

// ---- options --------------------------------------------------------------

// optionFrom reads `"option" optionName "=" constant ";"`.
func optionFrom(el map[string]any) map[string]OptionValue {
	cst := child(el, "constant")
	if cst == nil {
		return map[string]OptionValue{}
	}
	name := optionNameOf(el, cst)
	if name == "" {
		return map[string]OptionValue{}
	}
	return map[string]OptionValue{name: constantValue(cst)}
}

// ---- services -------------------------------------------------------------

func buildService(n map[string]any) ServiceDescriptorProto {
	svc := ServiceDescriptorProto{Name: srcOr(child(n, "ident")), Method: []MethodDescriptorProto{}}
	for _, el := range childRules(n) {
		if nrule(el) != "serviceElement" {
			continue
		}
		if strings.HasPrefix(kw(el), "rpc") {
			svc.Method = append(svc.Method, buildRpc(el))
		} else if isOptionStmt(el) {
			if svc.Options == nil {
				svc.Options = map[string]OptionValue{}
			}
			for kk, vv := range optionFrom(el) {
				svc.Options[kk] = vv
			}
		}
	}
	return svc
}

// buildRpc reads
// `rpc ident "(" ["stream"] messageType ")" "returns" "(" ["stream"] messageType ")"`.
func buildRpc(el map[string]any) MethodDescriptorProto {
	var ids, types []map[string]any
	for _, k := range childRules(el) {
		switch nrule(k) {
		case "ident":
			ids = append(ids, k)
		case "messageType":
			types = append(types, k)
		}
	}
	m := MethodDescriptorProto{
		Name:       srcAt(ids, 0),
		InputType:  srcAt(types, 0),
		OutputType: srcAt(types, 1),
	}
	// `stream` is a bare terminal, so it never becomes a node — but it sits
	// immediately ahead of the type it modifies, which is exactly what the gap
	// holds. Searching the statement for `(stream` instead marks an ordinary
	// type whose name merely BEGINS with those letters, so
	// `rpc M (streaming.Request)` came back client-streaming.
	modifiers := gapsBefore(el, "messageType")
	if len(modifiers) > 0 && strings.HasSuffix(modifiers[0], "stream") {
		m.ClientStreaming = true
	}
	if len(modifiers) > 1 && strings.HasSuffix(modifiers[1], "stream") {
		m.ServerStreaming = true
	}
	for _, o := range childRules(el) {
		if nrule(o) != "optionStmt" {
			continue
		}
		if m.Options == nil {
			m.Options = map[string]OptionValue{}
		}
		for kk, vv := range optionFrom(o) {
			m.Options[kk] = vv
		}
	}
	return m
}

// ---- file -----------------------------------------------------------------

// BuildFile turns a parsed `proto` CST root into a FileDescriptorProto for
// the given (already resolved) version. Most callers want ToDescriptor or
// Parse, which resolve the version first.
// Go counterpart of the TS `buildFile` (ts/src/build-descriptor.ts).
func BuildFile(proto map[string]any, version ProtoVersion) FileDescriptorProto {
	file := FileDescriptorProto{
		Dependency: []string{}, PublicDependency: []int{}, WeakDependency: []int{},
		MessageType: []DescriptorProto{}, EnumType: []EnumDescriptorProto{},
		Service: []ServiceDescriptorProto{}, Extension: []FieldDescriptorProto{},
	}
	if IsEdition(version) {
		file.Edition = EditionEnum(version)
		file.Syntax = "editions"
	} else {
		file.Syntax = version
	}

	for _, def := range childRules(proto) {
		if nrule(def) != "topLevelDef" {
			continue
		}
		k := kw(def)
		switch {
		case strings.HasPrefix(k, "package"):
			if fi := child(def, "fullIdent"); fi != nil {
				file.Package = nsrc(fi)
			}
		case strings.HasPrefix(k, "import"):
			if s := child(def, "strLit"); s != nil {
				// `import option "x";` (edition 2024) is a separate list.
				if strings.Contains(k, "option") {
					file.OptionDependency = append(file.OptionDependency, unquote(nsrc(s)))
					break
				}
				idx := len(file.Dependency)
				file.Dependency = append(file.Dependency, unquote(nsrc(s)))
				if strings.Contains(k, "public") {
					file.PublicDependency = append(file.PublicDependency, idx)
				}
				if strings.Contains(k, "weak") {
					file.WeakDependency = append(file.WeakDependency, idx)
				}
			}
		case isOptionStmt(def):
			if file.Options == nil {
				file.Options = map[string]OptionValue{}
			}
			for kk, vv := range optionFrom(def) {
				file.Options[kk] = vv
			}
		case strings.HasPrefix(k, "export"), strings.HasPrefix(k, "local"):
			addVisible(def, k, version, &file.MessageType, &file.EnumType)
		case strings.HasPrefix(k, "message"):
			file.MessageType = append(file.MessageType, buildMessage(def, version))
		case strings.HasPrefix(k, "enum"):
			file.EnumType = append(file.EnumType, buildEnum(def))
		case strings.HasPrefix(k, "service"):
			file.Service = append(file.Service, buildService(def))
		case strings.HasPrefix(k, "extend"):
			addExtend(def, version, &file.Extension)
		}
	}
	return file
}
