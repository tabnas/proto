# `descriptor.proto`

`descriptor.proto` is protobuf's own descriptor definition, vendored
unchanged from the protocolbuffers/protobuf repository
(`src/google/protobuf/descriptor.proto`, fetched from the `main` branch on
2026-09-25). It carries its own copyright and BSD-style licence header; see
https://github.com/protocolbuffers/protobuf/blob/main/LICENSE.

It is here as a conformance input, not as a fixture row: at 1,500 lines
it is the one real-world file that uses nearly every keyword as a name
(`package`, `syntax`, `edition`, `weak`, `reserved`, `repeated`), declares
`enum Edition`, and mixes groups, extension ranges, aggregate options and
reserved lists. `ts/test/descriptor-proto.test.ts`,
`go/descriptor_proto_test.go` and `rs/tests/descriptor_proto_test.rs`
each parse it and check the same facts about the descriptor. Refresh it
by replacing the file and re-checking those facts; nothing else in the
repository reads it.
