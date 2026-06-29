'use strict'

// Parse chat.proto with @tabnas/proto and expose the descriptor-driven
// encode/decode bound to its registry. This is the only place the project
// under test is used — everything downstream runs off its descriptor.

const fs = require('fs')
const path = require('path')

// `../..` resolves to the package root (ts/), i.e. the built dist/proto.js.
const { parse } = require('../..')
const { makeRegistry, encode, decode } = require('./codec')

const protoSource = fs.readFileSync(path.join(__dirname, 'chat.proto'), 'utf8')

// @tabnas/proto turns the .proto text into a FileDescriptorProto.
const fdp = parse(protoSource)

// The codec works purely off that descriptor.
const registry = makeRegistry(fdp)

module.exports = {
  fdp,
  registry,
  encode: (typeName, obj) => encode(registry, typeName, obj),
  decode: (typeName, buf) => decode(registry, typeName, buf),
}
