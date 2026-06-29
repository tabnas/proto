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

export interface FieldDescriptorProto {
  name: string
  number: number
  label?: FieldLabel
  type?: FieldType
  // Set for message/enum/group field types (resolution deferred).
  typeName?: string
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
  reservedRange?: { start: number; end: number }[]
  reservedName?: string[]
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
  extensionRange?: { start: number; end: number }[]
  reservedRange?: { start: number; end: number }[]
  reservedName?: string[]
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
  messageType: DescriptorProto[]
  enumType: EnumDescriptorProto[]
  service: ServiceDescriptorProto[]
  extension: FieldDescriptorProto[]
  options?: Record<string, OptionValue>
  // 'proto2' | 'proto3' for syntax files; absent for edition files.
  syntax?: string
  // 'EDITION_2023' | 'EDITION_2024' for edition files; absent otherwise.
  edition?: string
}

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
