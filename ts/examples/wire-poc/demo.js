'use strict'

// One-command end-to-end demo: starts the TCP server, runs the client
// against it over a real loopback socket, then shuts down. Bytes really
// traverse the network stack — the only "magic" is that the schema and the
// codec are both derived from chat.proto via @tabnas/proto.

const { fdp } = require('./schema')
const server = require('./server')
const client = require('./client')

console.log('Parsed chat.proto with @tabnas/proto: %s, %d top-level messages (%s)\n',
  fdp.syntax, fdp.messageType.length, fdp.messageType.map((m) => m.name).join(', '))

const srv = server.start(0, (s) => {
  const port = s.address().port
  client.send({
    id: 1,
    user: 'ada',
    text: 'hello over the wire',
    timestamp: Date.now(),
    tags: ['greeting', 'demo'],
    priority: 'HIGH',
    meta: { client: 'wire-poc/1.0', encrypted: true },
  }, port, () => {
    srv.close()
    console.log('\nDone — schema parsed by @tabnas/proto drove a real protobuf exchange over TCP.')
  })
})
