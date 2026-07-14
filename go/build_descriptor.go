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

// ---- small helpers --------------------------------------------------------

var (
	wsRe       = regexp.MustCompile(`\s+`)
	unquoteRe  = regexp.MustCompile(`(?s)^["'](.*)["']$`)
	quoteRe    = regexp.MustCompile(`^["']`)
	numLeadRe  = regexp.MustCompile(`(?i)^[-+]?(?:\d|\.\d|0x|0o|0b)`)
	rangeRe    = regexp.MustCompile(`^(-?\d+)(?:to(-?\d+|max))?$`)
	plusPrefix = regexp.MustCompile(`^\+`)
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

// toInt parses an integer-valued token (field/enum numbers, ranges).
func toInt(s string) int {
	s = strings.TrimSpace(s)
	if n, err := strconv.Atoi(s); err == nil {
		return n
	}
	if f, err := strconv.ParseFloat(s, 64); err == nil {
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

// optionNameOf reads the option name in an `optionName "=" constant` statement
// as everything before the constant, with the trailing `=` removed.
func optionNameOf(stmt, value map[string]any) string {
	if value == nil {
		return ""
	}
	i := strings.Index(nsrc(stmt), nsrc(value))
	pre := ""
	if i > 0 {
		pre = nsrc(stmt)[:i]
	}
	return strings.TrimSuffix(pre, "=")
}

// readFieldOptions reads `"[" fieldOption *( "," fieldOption ) "]"`.
func readFieldOptions(opts map[string]any) map[string]OptionValue {
	if opts == nil {
		return nil
	}
	out := map[string]OptionValue{}
	for _, fo := range childRules(opts) {
		cst := child(fo, "constant")
		name := optionNameOf(fo, cst)
		if cst != nil && name != "" {
			out[name] = constantValue(cst)
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
	bare := strings.TrimPrefix(typeText, ".")
	if scalar, ok := ScalarTypes[bare]; ok {
		return scalar, ""
	}
	// Message or enum reference; resolution deferred, store as written.
	return "TYPE_MESSAGE", typeText
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

func buildField(n map[string]any, version ProtoVersion) FieldDescriptorProto {
	label := child(n, "label")
	typeNode := typeNodeOf(n)
	name := child(n, "ident")
	number := child(n, "fieldNumber")
	opts := child(n, "fieldOptions")

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
	if fo := readFieldOptions(opts); fo != nil {
		f.Options = fo
	}
	return f
}

// mapEntryName mirrors the TS `fname.replace(/(^|_)([a-z])/g, …)` — upper-case
// the first letter and any letter following an underscore (the underscore is
// preserved) — then append "Entry".
func mapEntryName(fname string) string {
	b := make([]byte, 0, len(fname)+5)
	for i := 0; i < len(fname); i++ {
		c := fname[i]
		if (i == 0 || fname[i-1] == '_') && c >= 'a' && c <= 'z' {
			b = append(b, c-('a'-'A'))
		} else {
			b = append(b, c)
		}
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
	into.NestedType = append(into.NestedType, entry)

	f := FieldDescriptorProto{
		Name:     fname,
		Number:   numOr(number),
		Label:    "LABEL_REPEATED",
		Type:     "TYPE_MESSAGE",
		TypeName: entryName,
	}
	if fo := readFieldOptions(child(n, "fieldOptions")); fo != nil {
		f.Options = fo
	}
	return f
}

// ---- enums ----------------------------------------------------------------

func buildEnum(n map[string]any) EnumDescriptorProto {
	e := EnumDescriptorProto{Name: srcOr(child(n, "ident")), Value: []EnumValueDescriptorProto{}}
	for _, el := range childRules(n) {
		if nrule(el) != "enumElement" {
			continue
		}
		k := kw(el)
		if strings.HasPrefix(k, "reserved") {
			addReserved(el, &e.ReservedRange, &e.ReservedName)
			continue
		}
		if strings.HasPrefix(k, "option") {
			continue // enum-level option
		}
		// enumField: ident "=" ["-"] fieldNumber  -> name is the kw before "="
		name := kw(el)
		if i := strings.Index(name, "="); i >= 0 {
			name = name[:i]
		}
		num := child(el, "fieldNumber")
		if name != "" && num != nil {
			n := toInt(nsrc(num))
			if strings.Contains(stripWS(nsrc(el)), "=-") {
				n = -n
			}
			e.Value = append(e.Value, EnumValueDescriptorProto{Name: name, Number: n})
		}
	}
	return e
}

// ---- reserved / extensions ranges -----------------------------------------

// ranges parses `range *( "," range )`. The leading range is inlined into the
// node's src, so parse the (whitespace-stripped) text rather than kids.
func ranges(rangesNode map[string]any) []Range {
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
		end := start
		if m[2] == "max" {
			end = 536870911
		} else if m[2] != "" {
			end = toInt(m[2])
		}
		out = append(out, Range{Start: start, End: end})
	}
	return out
}

func addReserved(n map[string]any, rr *[]Range, rnames *[]string) {
	if rn := child(n, "ranges"); rn != nil {
		*rr = append(*rr, ranges(rn)...)
		return
	}
	if names := child(n, "fieldNames"); names != nil {
		for _, k := range childRules(names) {
			if nrule(k) == "strLit" {
				*rnames = append(*rnames, unquote(nsrc(k)))
			}
		}
	}
}

// ---- messages -------------------------------------------------------------

func buildMessage(n map[string]any, version ProtoVersion) DescriptorProto {
	msg := DescriptorProto{
		Name:  srcOr(child(n, "ident")),
		Field: []FieldDescriptorProto{}, NestedType: []DescriptorProto{},
		EnumType: []EnumDescriptorProto{}, OneofDecl: []OneofDescriptorProto{},
		Extension: []FieldDescriptorProto{},
	}
	body := child(n, "messageBody")
	if body != nil {
		for _, el := range childRules(body) {
			if nrule(el) == "messageElement" {
				addMessageElement(el, version, &msg)
			}
		}
	}
	return msg
}

func addMessageElement(el map[string]any, version ProtoVersion, msg *DescriptorProto) {
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
		if m := child(el, "message"); m != nil {
			msg.NestedType = append(msg.NestedType, buildMessage(m, version))
		} else if e := child(el, "enumDef"); e != nil {
			msg.EnumType = append(msg.EnumType, buildEnum(e))
		}
		return
	case strings.HasPrefix(k, "message"):
		msg.NestedType = append(msg.NestedType, buildMessage(el, version))
		return
	case strings.HasPrefix(k, "enum"):
		msg.EnumType = append(msg.EnumType, buildEnum(el))
		return
	case strings.HasPrefix(k, "reserved"):
		addReserved(el, &msg.ReservedRange, &msg.ReservedName)
		return
	case strings.HasPrefix(k, "extensions"):
		msg.ExtensionRange = append(msg.ExtensionRange, ranges(child(el, "ranges"))...)
		return
	case strings.HasPrefix(k, "extend"):
		addExtend(el, version, &msg.Extension)
		return
	case strings.HasPrefix(k, "option"):
		if msg.Options == nil {
			msg.Options = map[string]OptionValue{}
		}
		for kk, vv := range optionFrom(el) {
			msg.Options[kk] = vv
		}
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

func addOneof(el map[string]any, version ProtoVersion, msg *DescriptorProto) {
	name := srcOr(child(el, "ident"))
	index := len(msg.OneofDecl)
	msg.OneofDecl = append(msg.OneofDecl, OneofDescriptorProto{Name: name})
	for _, of := range childRules(el) {
		if nrule(of) != "oneofElement" {
			continue
		}
		if strings.HasPrefix(kw(of), "option") {
			continue
		}
		if nsrc(of) == ";" {
			continue
		}
		f := buildField(of, version)
		idx := index
		f.OneofIndex = &idx
		f.Proto3Optional = false // explicit oneof members aren't proto3-optional
		msg.Field = append(msg.Field, f)
	}
}

func addExtend(el map[string]any, version ProtoVersion, into *[]FieldDescriptorProto) {
	// extend messageType "{" *field "}" — fields inline as messageElement-like.
	for _, f := range childRules(el) {
		if nrule(f) == "field" || nrule(f) == "messageElement" {
			*into = append(*into, buildField(f, version))
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
	name := strings.TrimPrefix(optionNameOf(el, cst), "option")
	return map[string]OptionValue{name: constantValue(cst)}
}

// ---- services -------------------------------------------------------------

func buildService(n map[string]any) ServiceDescriptorProto {
	svc := ServiceDescriptorProto{Name: srcOr(child(n, "ident")), Method: []MethodDescriptorProto{}}
	for _, el := range childRules(n) {
		if nrule(el) == "serviceElement" && strings.HasPrefix(kw(el), "rpc") {
			svc.Method = append(svc.Method, buildRpc(el))
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
	flat := stripWS(nsrc(el))
	m := MethodDescriptorProto{
		Name:       srcAt(ids, 0),
		InputType:  srcAt(types, 0),
		OutputType: srcAt(types, 1),
	}
	// Split request vs response on the `returns` keyword so a `(stream …)` is
	// attributed to the right side even when in/out types are identical.
	request, response := flat, ""
	if ri := strings.Index(flat, "returns("); ri >= 0 {
		request, response = flat[:ri], flat[ri:]
	}
	if strings.Contains(request, "(stream") {
		m.ClientStreaming = true
	}
	if strings.Contains(response, "(stream") {
		m.ServerStreaming = true
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
				idx := len(file.Dependency)
				file.Dependency = append(file.Dependency, unquote(nsrc(s)))
				if strings.Contains(k, "public") {
					file.PublicDependency = append(file.PublicDependency, idx)
				}
				if strings.Contains(k, "weak") {
					file.WeakDependency = append(file.WeakDependency, idx)
				}
			}
		case strings.HasPrefix(k, "option"):
			if file.Options == nil {
				file.Options = map[string]OptionValue{}
			}
			for kk, vv := range optionFrom(def) {
				file.Options[kk] = vv
			}
		case strings.HasPrefix(k, "export"), strings.HasPrefix(k, "local"):
			if m := child(def, "message"); m != nil {
				file.MessageType = append(file.MessageType, buildMessage(m, version))
			} else if e := child(def, "enumDef"); e != nil {
				file.EnumType = append(file.EnumType, buildEnum(e))
			}
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
