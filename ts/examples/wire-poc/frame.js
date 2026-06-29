'use strict'

// TCP is a byte stream, so we length-prefix each protobuf message with a
// 4-byte big-endian frame length to recover message boundaries.

function frame(payload) {
  const header = Buffer.allocUnsafe(4)
  header.writeUInt32BE(payload.length, 0)
  return Buffer.concat([header, payload])
}

// Returns a function you feed socket chunks to; it calls `onMessage(payload)`
// once per complete frame.
function makeFrameReader(onMessage) {
  let buf = Buffer.alloc(0)
  return (chunk) => {
    buf = Buffer.concat([buf, chunk])
    for (;;) {
      if (buf.length < 4) return
      const len = buf.readUInt32BE(0)
      if (buf.length < 4 + len) return
      const payload = buf.subarray(4, 4 + len)
      buf = buf.subarray(4 + len)
      onMessage(payload)
    }
  }
}

module.exports = { frame, makeFrameReader }
