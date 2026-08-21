// Minimal local SSH server for end-to-end testing of the client (uses the
// ssh2 package). Not part of the app. Echo shell + PTY/resize logging.
//
//   node scripts/test-sshd.cjs [port]
//
// Credentials: user "test", password "secret123".
// Keyboard-interactive: answers "42" to the OTP prompt.
const { generateKeyPairSync } = require('node:crypto')
const { Server } = require('ssh2')

const PORT = Number(process.argv[2]) || 2222
const { privateKey } = generateKeyPairSync('rsa', {
  modulusLength: 2048,
  privateKeyEncoding: { type: 'pkcs1', format: 'pem' },
  publicKeyEncoding: { type: 'pkcs1', format: 'pem' }
})

const server = new Server({ hostKeys: [privateKey] }, (client) => {
  console.log('[sshd] client connected')
  client.on('error', (err) => console.log(`[sshd] client error: ${err.message}`))

  client.on('authentication', (ctx) => {
    // "mfa" user: PAM-style keyboard-interactive only — first a hidden
    // password round (the client should auto-answer with the typed password),
    // then an OTP round (the client should show its MFA dialog).
    if (ctx.username === 'mfa') {
      if (ctx.method !== 'keyboard-interactive') {
        return ctx.reject(['keyboard-interactive'])
      }
      console.log('[sshd] kbd-interactive start (mfa)')
      ctx.prompt([{ prompt: 'Password: ', echo: false }], (a1) => {
        const pwOk = a1 && a1[0] === 'secret123'
        console.log(`[sshd] kbd round 1 (password): ${pwOk ? 'OK' : 'REJECTED'}`)
        if (!pwOk) return ctx.reject()
        ctx.prompt([{ prompt: 'OTP code: ', echo: false }], (a2) => {
          const ok = a2 && a2[0] === '42'
          console.log(`[sshd] kbd round 2 (OTP): ${ok ? 'OK' : 'REJECTED'}`)
          ok ? ctx.accept() : ctx.reject()
        })
      })
      return
    }
    // "keyuser": any public key is accepted (client-side signing still runs).
    if (ctx.username === 'keyuser' && ctx.method === 'publickey') {
      console.log(`[sshd] publickey auth (${ctx.key.algo}): OK`)
      return ctx.accept()
    }
    if (ctx.method === 'password') {
      const ok = ctx.username === 'test' && ctx.password === 'secret123'
      console.log(`[sshd] password auth (${ctx.username}): ${ok ? 'OK' : 'REJECTED'}`)
      return ok ? ctx.accept() : ctx.reject(['password', 'keyboard-interactive'])
    }
    ctx.reject(['password', 'keyboard-interactive', 'publickey'])
  })

  client.on('ready', () => {
    console.log('[sshd] client authenticated')
    // direct-tcpip (local port forwarding): greet then echo, for tunnel tests.
    client.on('tcpip', (accept, _reject, info) => {
      console.log(`[sshd] direct-tcpip ${info.srcIP}:${info.srcPort} -> ${info.destIP}:${info.destPort}`)
      const stream = accept()
      stream.write('tcpip-echo ready\r\n')
      stream.on('data', (chunk) => stream.write(chunk))
      stream.on('close', () => console.log('[sshd] tcpip stream closed'))
    })
    client.on('session', (accept) => {
      const session = accept()
      let pty = { cols: 0, rows: 0, term: '?' }
      session.on('pty', (acceptPty, _reject, info) => {
        pty = info
        console.log(`[sshd] pty: ${info.term} ${info.cols}x${info.rows}`)
        acceptPty && acceptPty()
      })
      session.on('window-change', (acceptWc, _reject, info) => {
        console.log(`[sshd] resize: ${info.cols}x${info.rows}`)
        acceptWc && acceptWc()
      })
      session.on('shell', (acceptShell) => {
        const stream = acceptShell()
        console.log('[sshd] shell opened')
        stream.write(`Welcome to test-sshd (${pty.term}, ${pty.cols}x${pty.rows})\r\n`)
        stream.write('UTF-8 check: héllo wörld — ✓ 日本語 🚀\r\n')
        // ANSI colour demo — exercises xterm's 16-colour rendering.
        stream.write(
          'Colours: \x1b[31mRED\x1b[0m \x1b[32mGREEN\x1b[0m \x1b[33mYELLOW\x1b[0m ' +
            '\x1b[34mBLUE\x1b[0m \x1b[35mMAGENTA\x1b[0m \x1b[36mCYAN\x1b[0m NORMAL\r\n$ '
        )
        // Echo shell: print back what you type; Enter gives a new prompt.
        // One command exists: "flood <MB>" streams that many MB of output as
        // fast as the channel accepts it (for client backpressure testing).
        let line = ''
        const flood = (mb) => {
          const chunk = `${'x'.repeat(120)}\r\n`.repeat(512) // ~61 KB
          let remaining = mb * 1024 * 1024
          const started = Date.now()
          console.log(`[sshd] flood ${mb} MB starting`)
          const pump = () => {
            while (remaining > 0) {
              remaining -= chunk.length
              if (!stream.write(chunk)) {
                stream.once('drain', pump)
                return
              }
            }
            const secs = ((Date.now() - started) / 1000).toFixed(1)
            console.log(`[sshd] flood done in ${secs}s`)
            stream.write(`\r\n[flood ${mb} MB done in ${secs}s]\r\n$ `)
          }
          pump()
        }
        stream.on('data', (chunk) => {
          const text = chunk.toString('latin1')
          if (text === '\r') {
            const match = /^flood(?:\s+(\d+))?$/.exec(line.trim())
            line = ''
            if (match) {
              stream.write('\r\n')
              flood(Number(match[1]) || 100)
              return
            }
            stream.write('\r\n$ ')
          } else {
            line += text
            stream.write(chunk)
          }
        })
        stream.on('close', () => console.log('[sshd] shell closed'))
      })
    })
  })

  client.on('close', () => console.log('[sshd] client disconnected'))
})

server.listen(PORT, '127.0.0.1', () => {
  console.log(`[sshd] listening on 127.0.0.1:${PORT}`)
})
