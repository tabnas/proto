# wire-poc — Protocol Buffers over the wire, driven by `@tabnas/proto`

A proof of concept: define a real `.proto` schema, parse it with
**`@tabnas/proto`**, and use the resulting descriptor to exchange **real
protobuf binary** between two Node.js processes over a TCP socket — with **no
other protobuf library**. The descriptor `@tabnas/proto` produces is enough to
drive actual wire encoding.

## Run it

From the package root (`ts/`), build the package once, then:

```sh
npm run build                       # ensure dist/proto.js is current
node examples/wire-poc/demo.js      # one-command end-to-end demo
```

Or as two real processes:

```sh
node examples/wire-poc/server.js    # terminal 1
node examples/wire-poc/client.js    # terminal 2
```

And the codec tests (golden wire bytes + round-trip):

```sh
node --test examples/wire-poc/codec.test.js
```

## What happens

1. `schema.js` reads [`chat.proto`](chat.proto) and calls `parse()` from
   `@tabnas/proto` → a FileDescriptorProto.
2. `codec.js` builds a registry from that descriptor and provides
   `encode` / `decode` — a small proto3 wire codec that reads field numbers,
   types, labels and nested type descriptors straight from the descriptor and
   emits/parses real protobuf binary.
3. `client.js` encodes a `ChatMessage`, length-prefixes it, and writes it to a
   TCP socket; `server.js` decodes it and replies with an encoded `Ack`.

The client prints the exact wire bytes, e.g.:

```
[client] 71 wire bytes: 080112036164611a1368656c6c6f...
```

Those bytes are standard proto3 — `protoc`/protobufjs would decode them from the
same `chat.proto`.

## Files

| File | Role |
|---|---|
| `chat.proto` | the schema (proto3): `ChatMessage` + nested `Meta`/`Priority`, and `Ack` |
| `codec.js` | descriptor-driven proto3 wire encode/decode (the interesting bit) |
| `schema.js` | parse `chat.proto` via `@tabnas/proto`, bind the codec |
| `frame.js` | 4-byte length-prefix framing for the TCP stream |
| `server.js` / `client.js` | the two TCP endpoints |
| `demo.js` | runs both end-to-end in one command |
| `codec.test.js` | golden bytes + round-trip assertions |

## Scope / honest limits

The codec covers exactly what the demo needs: varint scalars
(`int32`/`int64`/`uint32`/`uint64`/`bool`), enums, length-delimited
`string`/`bytes`, nested messages, and repeated **string**. It is **not** a full
protobuf runtime — no `fixed32/64`, `sint` zigzag, packed repeated scalars,
maps, groups, or oneof wire rules.

Because `@tabnas/proto` defers type resolution, a message field and an enum
field both arrive as `TYPE_MESSAGE` + `typeName`; the codec disambiguates by
looking the name up in the registry (enum → varint, message → length-delimited).
Type lookup is by simple name — a deliberate simplification, not full protobuf
scope resolution.
