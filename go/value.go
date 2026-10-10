/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// The descriptor as a tree: the plain value the canonical parse returns,
// member for member and in its member order.
//
// The canonical walk (ts/src/build-descriptor.ts) builds the descriptor as
// a JavaScript object, and an object lists its members in the order they
// were first assigned. encoding/json writes a FileDescriptorProto's
// members in the order its fields are declared and an option map's names
// sorted, so its JSON holds the same members in a different order: syntax
// after options where the canonical object has it before, proto3Optional
// after typeName where the canonical has it straight after label. A host
// that walks the value, as a translation does when it streams a tree's
// events, sees the order, so it reads this tree instead.

package tabnasproto

import (
	"sort"

	tabnas "github.com/tabnas/parser/go"
)

// DescriptorValue returns a descriptor as a tree in the canonical shape: a
// *tabnas.OrderedMap for each message, a []any for each list, and every
// member and option named as the canonical object names it, with the
// fixed members in the canonical order. Numbers are float64, as the
// engine's values are.
//
// This is what a host that walks the value reads, rather than the
// struct's encoding/json form, which holds the same members in the order
// the struct declares them, with option names sorted. The tree holds the
// members the descriptor holds, under the same rule its JSON follows: an
// empty string, a false flag, an empty optional list or an empty option
// map is absent.
//
// A descriptor does not say in what order its source set the members a
// statement places, such as a file's package and options, or the names in
// an option map: the walk records that beside it (order.go), and
// ParseValue and ToDescriptorValue read the record. Given a descriptor
// alone, DescriptorValue gives those members in the order order.go lists,
// and an option map's names sorted, at every depth.
func DescriptorValue(file FileDescriptorProto) *tabnas.OrderedMap {
	return descriptorValue(file, nil)
}

// descriptorValue is DescriptorValue, ordered by the walk's record where
// it has one.
func descriptorValue(file FileDescriptorProto, rec *order) *tabnas.OrderedMap {
	out := tabnas.NewOrderedMap()
	out.Set("dependency", stringList(file.Dependency))
	out.Set("publicDependency", intList(file.PublicDependency))
	out.Set("weakDependency", intList(file.WeakDependency))
	messages := make([]any, len(file.MessageType))
	for i, m := range file.MessageType {
		messages[i] = messageValue(m, rec.kid("messageType", i))
	}
	out.Set("messageType", messages)
	enums := make([]any, len(file.EnumType))
	for i, e := range file.EnumType {
		enums[i] = enumValue(e, rec.kid("enumType", i))
	}
	out.Set("enumType", enums)
	services := make([]any, len(file.Service))
	for i, s := range file.Service {
		services[i] = serviceValue(s, rec.kid("service", i))
	}
	out.Set("service", services)
	out.Set("extension", fieldList(file.Extension, rec, "extension"))
	// An edition file assigns edition and then syntax, a syntax file
	// syntax alone, both before any statement is read.
	setString(out, "edition", file.Edition)
	setString(out, "syntax", file.Syntax)
	// The canonical walk assigns package at every package statement, but
	// only the text of a name, so a statement without one leaves it unset.
	setOrdered(out, rec.memberOrder(), []member{
		{"package", file.Package != "", false, func() any { return file.Package }},
		{"optionDependency", len(file.OptionDependency) > 0, true,
			func() any { return stringList(file.OptionDependency) }},
		{"options", len(file.Options) > 0, true,
			func() any { return optionsValue(file.Options, rec.optionOrder()) }},
	})
	// The walk never sets name; a caller who does sets it after the walk,
	// which places it last.
	setString(out, "name", file.Name)
	return out
}

// ParseValue parses a .proto source string to the descriptor as the tree
// the canonical parse returns: DescriptorValue's tree, with every member
// and option name in the canonical object's order, the walk's record of
// the source's order included. Like Parse it builds a fresh engine each
// time; for repeated parsing reuse an engine with Proto and call
// ToDescriptorValue on each CST.
func ParseValue(src string, opts *ProtoOptions) (*tabnas.OrderedMap, error) {
	cst, err := parseCST(src)
	if err != nil {
		return nil, err
	}
	return ToDescriptorValue(cst, opts)
}

// ToDescriptorValue turns a parsed proto CST into the tree ParseValue
// gives, as ToDescriptor turns one into a FileDescriptorProto: for a
// caller that reuses an engine, running Preflight on each source first.
func ToDescriptorValue(cst any, opts *ProtoOptions) (*tabnas.OrderedMap, error) {
	file, rec, err := toDescriptor(cst, opts)
	if err != nil {
		return nil, err
	}
	return descriptorValue(file, rec), nil
}

// member is one statement-ordered member: its name, whether the
// descriptor holds a value for it, whether a statement that placed it
// makes it present even without one, as an assigned empty list or map
// is, and its value.
type member struct {
	name  string
	set   bool
	keep  bool
	value func() any
}

// setOrdered sets the statement-ordered members: those the record holds,
// in its order, then the rest in the order given. A member the record
// holds is present when it has a value or keeps its place without one,
// as `extensions 1_0;` assigns an empty extensionRange; any other only
// with a value.
func setOrdered(out *tabnas.OrderedMap, recorded []string, members []member) {
	done := make([]bool, len(members))
	for _, name := range recorded {
		for i, m := range members {
			if !done[i] && m.name == name {
				done[i] = true
				if m.set || m.keep {
					out.Set(m.name, m.value())
				}
			}
		}
	}
	for i, m := range members {
		if !done[i] && m.set {
			out.Set(m.name, m.value())
		}
	}
}

// optionNames is the order an option map's names are given in: the order
// the walk recorded, as far as the map holds those names, then any others
// sorted.
func optionNames(opts map[string]OptionValue, recorded []string) []string {
	names := make([]string, 0, len(opts))
	placed := make(map[string]bool, len(opts))
	for _, name := range recorded {
		if _, ok := opts[name]; ok && !placed[name] {
			placed[name] = true
			names = append(names, name)
		}
	}
	var rest []string
	for name := range opts {
		if !placed[name] {
			rest = append(rest, name)
		}
	}
	sort.Strings(rest)
	return append(names, rest...)
}

func optionsValue(opts map[string]OptionValue, recorded []string) *tabnas.OrderedMap {
	out := tabnas.NewOrderedMap()
	for _, name := range optionNames(opts, recorded) {
		out.Set(name, optionTree(opts[name]))
	}
	return out
}

// optionTree is an option's value in the tree. A nested option map, which
// only a descriptor built by hand holds, becomes an ordered map with its
// names sorted, at every depth, since a Go map keeps no order, and a list
// holding one is rebuilt around it; anything else is as it is.
func optionTree(value OptionValue) any {
	switch v := value.(type) {
	case map[string]OptionValue:
		out := tabnas.NewOrderedMap()
		for _, name := range optionNames(v, nil) {
			out.Set(name, optionTree(v[name]))
		}
		return out
	case []any:
		out := make([]any, len(v))
		for i, item := range v {
			out[i] = optionTree(item)
		}
		return out
	}
	return value
}

func setString(out *tabnas.OrderedMap, name, value string) {
	if value != "" {
		out.Set(name, value)
	}
}

func setOptions(out *tabnas.OrderedMap, opts map[string]OptionValue, recorded []string) {
	if len(opts) > 0 {
		out.Set("options", optionsValue(opts, recorded))
	}
}

func stringList(items []string) []any {
	out := make([]any, len(items))
	for i, s := range items {
		out[i] = s
	}
	return out
}

func intList(items []int) []any {
	out := make([]any, len(items))
	for i, n := range items {
		out[i] = float64(n)
	}
	return out
}

// rangeList and fieldList give a list, each item ordered by its record
// in rec's list of that name.
func rangeList(ranges []Range, rec *order, list string) []any {
	out := make([]any, len(ranges))
	for i, r := range ranges {
		v := tabnas.NewOrderedMap()
		v.Set("start", float64(r.Start))
		v.Set("end", float64(r.End))
		setOptions(v, r.Options, rec.kid(list, i).optionOrder())
		out[i] = v
	}
	return out
}

func fieldList(fields []FieldDescriptorProto, rec *order, list string) []any {
	out := make([]any, len(fields))
	for i, f := range fields {
		out[i] = fieldValue(f, rec.kid(list, i))
	}
	return out
}

// messageValue is { name, field, nestedType, enumType, oneofDecl,
// extension }, then options, which the walk reads in a pass of its own
// before the other statements, then the statement-ordered ranges and
// names, then the visibility an edition-2024 export or local adds last.
func messageValue(msg DescriptorProto, rec *order) *tabnas.OrderedMap {
	out := tabnas.NewOrderedMap()
	out.Set("name", msg.Name)
	out.Set("field", fieldList(msg.Field, rec, "field"))
	nested := make([]any, len(msg.NestedType))
	for i, m := range msg.NestedType {
		nested[i] = messageValue(m, rec.kid("nestedType", i))
	}
	out.Set("nestedType", nested)
	enums := make([]any, len(msg.EnumType))
	for i, e := range msg.EnumType {
		enums[i] = enumValue(e, rec.kid("enumType", i))
	}
	out.Set("enumType", enums)
	oneofs := make([]any, len(msg.OneofDecl))
	for i, o := range msg.OneofDecl {
		v := tabnas.NewOrderedMap()
		v.Set("name", o.Name)
		setOptions(v, o.Options, rec.kid("oneofDecl", i).optionOrder())
		oneofs[i] = v
	}
	out.Set("oneofDecl", oneofs)
	out.Set("extension", fieldList(msg.Extension, rec, "extension"))
	setOptions(out, msg.Options, rec.optionOrder())
	setOrdered(out, rec.memberOrder(), []member{
		{"extensionRange", len(msg.ExtensionRange) > 0, true,
			func() any { return rangeList(msg.ExtensionRange, rec, "extensionRange") }},
		{"reservedRange", len(msg.ReservedRange) > 0, true,
			func() any { return rangeList(msg.ReservedRange, rec, "reservedRange") }},
		{"reservedName", len(msg.ReservedName) > 0, true,
			func() any { return stringList(msg.ReservedName) }},
	})
	setString(out, "visibility", msg.Visibility)
	return out
}

// enumValue is { name, value }, then the statement-ordered ranges, names
// and options, then visibility.
func enumValue(e EnumDescriptorProto, rec *order) *tabnas.OrderedMap {
	out := tabnas.NewOrderedMap()
	out.Set("name", e.Name)
	values := make([]any, len(e.Value))
	for i, v := range e.Value {
		ev := tabnas.NewOrderedMap()
		ev.Set("name", v.Name)
		ev.Set("number", float64(v.Number))
		setOptions(ev, v.Options, rec.kid("value", i).optionOrder())
		values[i] = ev
	}
	out.Set("value", values)
	setOrdered(out, rec.memberOrder(), []member{
		{"reservedRange", len(e.ReservedRange) > 0, true,
			func() any { return rangeList(e.ReservedRange, rec, "reservedRange") }},
		{"reservedName", len(e.ReservedName) > 0, true,
			func() any { return stringList(e.ReservedName) }},
		{"options", len(e.Options) > 0, true,
			func() any { return optionsValue(e.Options, rec.optionOrder()) }},
	})
	setString(out, "visibility", e.Visibility)
	return out
}

// fieldValue is the canonical field: { name, number, label,
// proto3Optional?, type? or typeName? } as one literal, then the
// pseudo-options and options the field's own list sets, then extendee for
// an extend member, and last the oneofIndex a oneof, declared or
// synthesised, assigns once the field is built.
func fieldValue(f FieldDescriptorProto, rec *order) *tabnas.OrderedMap {
	out := tabnas.NewOrderedMap()
	out.Set("name", f.Name)
	out.Set("number", float64(f.Number))
	setString(out, "label", f.Label)
	if f.Proto3Optional {
		out.Set("proto3Optional", true)
	}
	setString(out, "type", f.Type)
	setString(out, "typeName", f.TypeName)
	setString(out, "jsonName", f.JsonName)
	setString(out, "defaultValue", f.DefaultValue)
	setOptions(out, f.Options, rec.optionOrder())
	setString(out, "extendee", f.Extendee)
	if f.OneofIndex != nil {
		out.Set("oneofIndex", float64(*f.OneofIndex))
	}
	return out
}

func serviceValue(svc ServiceDescriptorProto, rec *order) *tabnas.OrderedMap {
	out := tabnas.NewOrderedMap()
	out.Set("name", svc.Name)
	methods := make([]any, len(svc.Method))
	for i, m := range svc.Method {
		v := tabnas.NewOrderedMap()
		v.Set("name", m.Name)
		v.Set("inputType", m.InputType)
		v.Set("outputType", m.OutputType)
		if m.ClientStreaming {
			v.Set("clientStreaming", true)
		}
		if m.ServerStreaming {
			v.Set("serverStreaming", true)
		}
		setOptions(v, m.Options, rec.kid("method", i).optionOrder())
		methods[i] = v
	}
	out.Set("method", methods)
	setOptions(out, svc.Options, rec.optionOrder())
	return out
}
