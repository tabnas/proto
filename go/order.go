/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// The order records: what the canonical descriptor orders by statement,
// kept beside the descriptor rather than in it.
//
// The canonical descriptor is a JavaScript object, and an object lists its
// members in the order they were first assigned. A Go option set is a map,
// which has no order. And three members of each of the file, a message and
// an enum are assigned by the statement that first needs them, so their
// order is the source's; a record names those by their descriptor JSON
// name:
//
//	FileDescriptorProto  package, optionDependency, options
//	DescriptorProto      extensionRange, reservedRange, reservedName
//	EnumDescriptorProto  reservedRange, reservedName, options
//
// The walk records both for every descriptor it builds, in a tree of
// records beside the descriptor's own, which ParseValue and
// ToDescriptorValue read. The descriptor holds none of it, so two
// descriptors with the same fields are equal under reflect.DeepEqual and
// go-cmp, however their sources ordered the statements. DescriptorValue,
// given a descriptor alone, has no record: it gives the statement-ordered
// members in the order listed above, and an option map's names sorted.

package tabnasproto

// order is the record for one descriptor the walk built: its
// statement-ordered members and its option names, each in the order the
// source first set them, and the records of the descriptors in its lists,
// by the list's JSON name and the index there. A descriptor with nothing
// to record has no record.
type order struct {
	members []string
	options []string
	kids    map[string]map[int]*order
}

// kid is the record of the i-th descriptor in a list, or nil.
func (o *order) kid(list string, i int) *order {
	if o == nil {
		return nil
	}
	return o.kids[list][i]
}

// put files the record of the i-th descriptor in a list.
func (o *order) put(list string, i int, kid *order) {
	if kid == nil {
		return
	}
	if o.kids == nil {
		o.kids = map[string]map[int]*order{}
	}
	if o.kids[list] == nil {
		o.kids[list] = map[int]*order{}
	}
	o.kids[list][i] = kid
}

// memberOrder is the statement-ordered members a record holds, if any.
func (o *order) memberOrder() []string {
	if o == nil {
		return nil
	}
	return o.members
}

// optionOrder is the option names a record holds, if any.
func (o *order) optionOrder() []string {
	if o == nil {
		return nil
	}
	return o.options
}

// orNil is the record, or nil when it holds nothing.
func (o *order) orNil() *order {
	if len(o.members) == 0 && len(o.options) == 0 && len(o.kids) == 0 {
		return nil
	}
	return o
}

// optionsOnly is a record of an option order alone, or nil for none.
func optionsOnly(names []string) *order {
	if len(names) == 0 {
		return nil
	}
	return &order{options: names}
}
