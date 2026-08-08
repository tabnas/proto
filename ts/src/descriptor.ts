/* Copyright (c) 2026 Richard Rodger and other contributors, MIT License */

// FileDescriptorProto-shaped output types. This mirrors the canonical
// `descriptor.proto` message set (the shape `protoc --descriptor_set_out`
// produces, in its JSON form): camelCase fields, enum values as their
// string names (`TYPE_INT32`, `LABEL_REPEATED`). Only fields the parser
// can populate from a single `.proto` source are included; cross-file
// type resolution is left to a later pass, so `typeName` is stored as
// written.

export type FieldLabel = 'LABEL_OPTIONAL' | 'LABEL_REQUIRED' | 'LABEL_REPEATED'

export type FieldType =
  | 'TYPE_DOUBLE' | 'TYPE_FLOAT' | 'TYPE_INT64' | 'TYPE_UINT64'
  | 'TYPE_INT32' | 'TYPE_FIXED64' | 'TYPE_FIXED32' | 'TYPE_BOOL'
  | 'TYPE_STRING' | 'TYPE_GROUP' | 'TYPE_MESSAGE' | 'TYPE_BYTES'
  | 'TYPE_UINT32' | 'TYPE_ENUM' | 'TYPE_SFIXED32' | 'TYPE_SFIXED64'
  | 'TYPE_SINT32' | 'TYPE_SINT64'

export type OptionValue = string | number | boolean | { [k: string]: OptionValue }

// Symbol visibility, an edition-2024 feature (`export` / `local`).
export type SymbolVisibility = 'VISIBILITY_EXPORT' | 'VISIBILITY_LOCAL'

// A numeric range. `end` is EXCLUSIVE for message extension/reserved ranges
// and INCLUSIVE for enum reserved ranges — the same asymmetry protoc has.
export interface DescriptorRange {
  start: number
  end: number
  options?: Record<string, OptionValue>
}

export interface FieldDescriptorProto {
  name: string
  number: number
  label?: FieldLabel
  type?: FieldType
  // Set for message/enum/group field types (resolution deferred).
  typeName?: string
  // For an `extend` member: the message being extended, as written.
  extendee?: string
  // The `json_name = "..."` pseudo-option, lifted out of `options`.
  jsonName?: string
  // The `default = ...` pseudo-option, lifted out of `options`. Always a
  // string, as in descriptor.proto; the literal is stored as written.
  defaultValue?: string
  // proto3 explicit `optional` (synthesises a single-field oneof in protoc).
  proto3Optional?: boolean
  // The oneof this field belongs to, as an index into the message's
  // `oneofDecl`.
  oneofIndex?: number
  options?: Record<string, OptionValue>
}

export interface EnumValueDescriptorProto {
  name: string
  number: number
  options?: Record<string, OptionValue>
}

export interface EnumDescriptorProto {
  name: string
  value: EnumValueDescriptorProto[]
  reservedRange?: DescriptorRange[]
  reservedName?: string[]
  visibility?: SymbolVisibility
  options?: Record<string, OptionValue>
}

export interface OneofDescriptorProto {
  name: string
  options?: Record<string, OptionValue>
}

export interface DescriptorProto {
  name: string
  field: FieldDescriptorProto[]
  nestedType: DescriptorProto[]
  enumType: EnumDescriptorProto[]
  oneofDecl: OneofDescriptorProto[]
  extension: FieldDescriptorProto[]
  extensionRange?: DescriptorRange[]
  reservedRange?: DescriptorRange[]
  reservedName?: string[]
  visibility?: SymbolVisibility
  options?: Record<string, OptionValue>
}

export interface MethodDescriptorProto {
  name: string
  inputType: string
  outputType: string
  clientStreaming?: boolean
  serverStreaming?: boolean
  options?: Record<string, OptionValue>
}

export interface ServiceDescriptorProto {
  name: string
  method: MethodDescriptorProto[]
  options?: Record<string, OptionValue>
}

export interface FileDescriptorProto {
  // File name is not present in source; callers may set it.
  name?: string
  package?: string
  dependency: string[]
  publicDependency: number[]
  weakDependency: number[]
  // `import option "..."` targets (edition 2024). Present only when used.
  optionDependency?: string[]
  messageType: DescriptorProto[]
  enumType: EnumDescriptorProto[]
  service: ServiceDescriptorProto[]
  extension: FieldDescriptorProto[]
  options?: Record<string, OptionValue>
  // 'proto2' | 'proto3' for syntax files; 'editions' for edition files.
  syntax?: string
  // 'EDITION_2023' | 'EDITION_2024' for edition files; absent otherwise.
  edition?: string
}

// protoc's sentinels for `to max` in a range.
// Message field numbers stop at 2^29-1, so an exclusive `end` is 2^29;
// a message_set_wire_format message may use the full 32-bit space.
export const MAX_FIELD_NUMBER_END = 536870912
export const MAX_MESSAGE_SET_END = 2147483647
// Enum numbers are plain int32 and enum reserved ranges are inclusive.
export const MAX_ENUM_NUMBER = 2147483647

// Scalar protobuf types -> FieldDescriptorProto.type. A field whose type
// is not in this table is a message/enum/group reference (resolution
// deferred) and gets a `typeName` instead.
export const SCALAR_TYPES: Record<string, FieldType> = {
  double: 'TYPE_DOUBLE',
  float: 'TYPE_FLOAT',
  int32: 'TYPE_INT32',
  int64: 'TYPE_INT64',
  uint32: 'TYPE_UINT32',
  uint64: 'TYPE_UINT64',
  sint32: 'TYPE_SINT32',
  sint64: 'TYPE_SINT64',
  fixed32: 'TYPE_FIXED32',
  fixed64: 'TYPE_FIXED64',
  sfixed32: 'TYPE_SFIXED32',
  sfixed64: 'TYPE_SFIXED64',
  bool: 'TYPE_BOOL',
  string: 'TYPE_STRING',
  bytes: 'TYPE_BYTES',
}
