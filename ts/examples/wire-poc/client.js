'use strict'

// TCP client: encodes a ChatMessage with the @tabnas/proto-derived codec,
// sends it length-prefixed, and decodes the server's Ack.

const net = require('net')
const { encode, decode } = require('./schema')
const { frame, makeFrameReader } = require('./frame')

const PORT = Number(process.env.PORT || 7777)

function send(message, port = PORT, onDone) {
  const socket = net.createConnection(port, '127.0.0.1', () => {
    const payload = encode('ChatMessage', message)
    console.log('[client] sending ChatMessage', message)
    console.log('[client] %d wire bytes: %s', payload.length, payload.toString('hex'))
    socket.write(frame(payload))
  })

  const read = makeFrameReader((payload) => {
    const ack = decode('Ack', payload)
    console.log('[client] received Ack ->', ack)
    socket.end()
    if (onDone) onDone(ack)
  })
  socket.on('data', read)
  socket.on('error', (err) => { console.error('[client] error', err.message); if (onDone) onDone(null) })
  return socket
}

module.exports = { send }

if (require.main === module) {
  send({
    id: 1,
    user: 'ada',
    text: 'hello over the wire',
    timestamp: Date.now(),
    tags: ['greeting', 'demo'],
    priority: 'HIGH',
    meta: { client: 'wire-poc/1.0', encrypted: true },
  }, PORT, () => process.exit(0))
}
