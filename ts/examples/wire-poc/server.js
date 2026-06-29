'use strict'

// TCP server: receives length-prefixed ChatMessage frames, decodes them with
// the @tabnas/proto-derived codec, and replies with an encoded Ack.

const net = require('net')
const { encode, decode } = require('./schema')
const { frame, makeFrameReader } = require('./frame')

const PORT = Number(process.env.PORT || 7777)

function start(port = PORT, onReady) {
  const server = net.createServer((socket) => {
    const read = makeFrameReader((payload) => {
      const msg = decode('ChatMessage', payload)
      console.log('[server] received %d wire bytes ->', payload.length, msg)

      const ack = encode('Ack', {
        id: msg.id,
        ok: true,
        note: 'received "' + msg.text + '" from ' + msg.user,
      })
      socket.write(frame(ack))
    })
    socket.on('data', read)
    socket.on('error', () => {})
  })
  server.listen(port, '127.0.0.1', () => {
    console.log('[server] listening on 127.0.0.1:%d', server.address().port)
    if (onReady) onReady(server)
  })
  return server
}

module.exports = { start }

if (require.main === module) start()
