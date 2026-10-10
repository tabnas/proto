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

// DescriptorValue returns the descriptor as the tree the canonical parse
// returns: a *tabnas.OrderedMap for each message, a []any for each list,
// and every member and option named and ordered as the canonical object
// has it. Numbers are float64, as the engine's values are.
//
// This is what a host that walks the value reads, rather than the
// struct's encoding/json form, which holds the same members in the order
// the struct declares them, with option names sorted. The tree holds the
// members the descriptor holds, under the same rule its JSON follows: an
// empty string, a false flag, an empty optional list or an empty option
// map is absent.
//
// The members a statement places, such as a file's package and options,
// and the names in an option map, follow the order the walk recorded (see
// "The order records" in descriptor.go). A descriptor built by hand
// carries no record: its statement-ordered members then come in the order
// listed there, and an option map's names sorted, as are names added by
// hand to a parsed one.
func DescriptorValue(file FileDescriptorProto) *tabnas.OrderedMap {
	out := tabnas.NewOrderedMap()
	out.Set("dependency", stringList(file.Dependency))
	out.Set("publicDependency", intList(file.PublicDependency))
	out.Set("weakDependency", intList(file.WeakDependency))
	messages := make([]any, len(file.MessageType))
	for i, m := range file.MessageType {
		messages[i] = messageValue(m)
	}
	out.Set("messageType", messages)
	enums := make([]any, len(file.EnumType))
	for i, e := range file.EnumType {
		enums[i] = enumValue(e)
	}
	out.Set("enumType", enums)
	services := make([]any, len(file.Service))
	for i, s := range file.Service {
		services[i] = serviceValue(s)
	}
	out.Set("service", services)
	out.Set("extension", fieldList(file.Extension))
	// An edition file assigns edition and then syntax, a syntax file
	// syntax alone, both before any statement is read.
	setString(out, "edition", file.Edition)
	setString(out, "syntax", file.Syntax)
	setOrdered(out, file.memberOrder, []member{
		{"package", file.Package != "", func() any { return file.Package }},
		{"optionDependency", len(file.OptionDependency) > 0,
			func() any { return stringList(file.OptionDependency) }},
		{"options", len(file.Options) > 0,
			func() any { return optionsValue(file.Options, file.optionOrder) }},
	})
	// The walk never sets name; a caller who does sets it after the walk,
	// which places it last.
	setString(out, "name", file.Name)
	return out
}

// ParseValue parses a .proto source string to the descriptor as a tree:
// Parse, then DescriptorValue. Like Parse it builds a fresh engine each
// time; for repeated parsing reuse an engine with Proto and ToDescriptor,
// and call DescriptorValue on each result.
func ParseValue(src string, opts *ProtoOptions) (*tabnas.OrderedMap, error) {
	file, err := Parse(src, opts)
	if err != nil {
		return nil, err
	}
	return DescriptorValue(file), nil
}

// member is one statement-ordered member: its name, whether the
// descriptor holds it, and its value.
type member struct {
	name  string
	set   bool
	value func() any
}

// setOrdered sets the statement-ordered members: those the record holds,
// in its order, then the rest in the order given.
func setOrdered(out *tabnas.OrderedMap, order []string, members []member) {
	done := make([]bool, len(members))
	for _, name := range order {
		for i, m := range members {
			if !done[i] && m.name == name {
				done[i] = true
				if m.set {
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
func optionNames(opts map[string]OptionValue, order []string) []string {
	names := make([]string, 0, len(opts))
	placed := make(map[string]bool, len(opts))
	for _, name := range order {
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

func optionsValue(opts map[string]OptionValue, order []string) *tabnas.OrderedMap {
	out := tabnas.NewOrderedMap()
	for _, name := range optionNames(opts, order) {
		out.Set(name, opts[name])
	}
	return out
}

func setString(out *tabnas.OrderedMap, name, value string) {
	if value != "" {
		out.Set(name, value)
	}
}

func setOptions(out *tabnas.OrderedMap, opts map[string]OptionValue, order []string) {
	if len(opts) > 0 {
		out.Set("options", optionsValue(opts, order))
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

func rangeList(ranges []Range) []any {
	out := make([]any, len(ranges))
	for i, r := range ranges {
		v := tabnas.NewOrderedMap()
		v.Set("start", float64(r.Start))
		v.Set("end", float64(r.End))
		setOptions(v, r.Options, r.optionOrder)
		out[i] = v
	}
	return out
}

func fieldList(fields []FieldDescriptorProto) []any {
	out := make([]any, len(fields))
	for i, f := range fields {
		out[i] = fieldValue(f)
	}
	return out
}

// messageValue is { name, field, nestedType, enumType, oneofDecl,
// extension }, then options, which the walk reads in a pass of its own
// before the other statements, then the statement-ordered ranges and
// names, then the visibility an edition-2024 export or local adds last.
func messageValue(msg DescriptorProto) *tabnas.OrderedMap {
	out := tabnas.NewOrderedMap()
	out.Set("name", msg.Name)
	out.Set("field", fieldList(msg.Field))
	nested := make([]any, len(msg.NestedType))
	for i, m := range msg.NestedType {
		nested[i] = messageValue(m)
	}
	out.Set("nestedType", nested)
	enums := make([]any, len(msg.EnumType))
	for i, e := range msg.EnumType {
		enums[i] = enumValue(e)
	}
	out.Set("enumType", enums)
	oneofs := make([]any, len(msg.OneofDecl))
	for i, o := range msg.OneofDecl {
		v := tabnas.NewOrderedMap()
		v.Set("name", o.Name)
		setOptions(v, o.Options, o.optionOrder)
		oneofs[i] = v
	}
	out.Set("oneofDecl", oneofs)
	out.Set("extension", fieldList(msg.Extension))
	setOptions(out, msg.Options, msg.optionOrder)
	setOrdered(out, msg.memberOrder, []member{
		{"extensionRange", len(msg.ExtensionRange) > 0,
			func() any { return rangeList(msg.ExtensionRange) }},
		{"reservedRange", len(msg.ReservedRange) > 0,
			func() any { return rangeList(msg.ReservedRange) }},
		{"reservedName", len(msg.ReservedName) > 0,
			func() any { return stringList(msg.ReservedName) }},
	})
	setString(out, "visibility", msg.Visibility)
	return out
}

// enumValue is { name, value }, then the statement-ordered ranges, names
// and options, then visibility.
func enumValue(e EnumDescriptorProto) *tabnas.OrderedMap {
	out := tabnas.NewOrderedMap()
	out.Set("name", e.Name)
	values := make([]any, len(e.Value))
	for i, v := range e.Value {
		ev := tabnas.NewOrderedMap()
		ev.Set("name", v.Name)
		ev.Set("number", float64(v.Number))
		setOptions(ev, v.Options, v.optionOrder)
		values[i] = ev
	}
	out.Set("value", values)
	setOrdered(out, e.memberOrder, []member{
		{"reservedRange", len(e.ReservedRange) > 0,
			func() any { return rangeList(e.ReservedRange) }},
		{"reservedName", len(e.ReservedName) > 0,
			func() any { return stringList(e.ReservedName) }},
		{"options", len(e.Options) > 0,
			func() any { return optionsValue(e.Options, e.optionOrder) }},
	})
	setString(out, "visibility", e.Visibility)
	return out
}

// fieldValue is the canonical field: { name, number, label,
// proto3Optional?, type? or typeName? } as one literal, then the
// pseudo-options and options the field's own list sets, then extendee for
// an extend member, and last the oneofIndex a oneof, declared or
// synthesised, assigns once the field is built.
func fieldValue(f FieldDescriptorProto) *tabnas.OrderedMap {
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
	setOptions(out, f.Options, f.optionOrder)
	setString(out, "extendee", f.Extendee)
	if f.OneofIndex != nil {
		out.Set("oneofIndex", float64(*f.OneofIndex))
	}
	return out
}

func serviceValue(svc ServiceDescriptorProto) *tabnas.OrderedMap {
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
		setOptions(v, m.Options, m.optionOrder)
		methods[i] = v
	}
	out.Set("method", methods)
	setOptions(out, svc.Options, svc.optionOrder)
	return out
}
