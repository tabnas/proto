/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

//! The order records: what the canonical descriptor orders by statement,
//! kept beside the descriptor rather than in it.
//!
//! The canonical descriptor is a JavaScript object, and an object lists
//! its members in the order they were first assigned. The walk assigns
//! most of them in a fixed order, but three in each of the file, a
//! message and an enum are assigned by the statement that first needs
//! them, so their order is the source's:
//!
//! | container | statement-ordered members |
//! |---|---|
//! | `FileDescriptorProto` | `package`, `optionDependency`, `options` |
//! | `DescriptorProto` | `extensionRange`, `reservedRange`, `reservedName` |
//! | `EnumDescriptorProto` | `reservedRange`, `reservedName`, `options` |
//!
//! The walk records those, by their descriptor JSON names, for every
//! container it builds, in a tree of records beside the descriptor's own,
//! which [`crate::parse_value`], [`crate::parse_value_with`] and
//! [`crate::to_descriptor_value`] read. The descriptor holds none of it,
//! so its public types are what they were before the tree, and two
//! descriptors holding the same values are equal however their sources
//! ordered the statements. [`crate::descriptor_value`], given a
//! descriptor alone, has no record and gives those members in the order
//! the table lists. An option map needs no record: [`crate::Options`]
//! keeps its names in the order the source set them. `go/order.go` is the
//! Go port's record.

use std::collections::HashMap;

/// The record for one container the walk built: its statement-ordered
/// members, in the order the source first set them, and the records of
/// the messages and enums in its lists, by the list's JSON name and the
/// index there. A container with nothing to record files none.
#[derive(Debug, Default)]
pub(crate) struct Order {
    members: Vec<&'static str>,
    kids: HashMap<(&'static str, usize), Order>,
}

impl Order {
    /// Record `member` as set. A member already recorded keeps the place
    /// its first statement gave it, as a JavaScript object member does.
    pub(crate) fn record(&mut self, member: &'static str) {
        if !self.members.contains(&member) {
            self.members.push(member);
        }
    }

    /// The members recorded, in the order they were first set.
    pub(crate) fn members(&self) -> &[&'static str] {
        &self.members
    }

    /// File the record of the `index`-th container in `list`.
    pub(crate) fn put(&mut self, list: &'static str, index: usize, kid: Order) {
        if !kid.members.is_empty() || !kid.kids.is_empty() {
            self.kids.insert((list, index), kid);
        }
    }

    /// The record of the `index`-th container in `list`, if one was filed.
    pub(crate) fn kid(&self, list: &'static str, index: usize) -> Option<&Order> {
        self.kids.get(&(list, index))
    }
}
