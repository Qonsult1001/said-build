// Minimal, robust MCP stdio JSON-RPC client for the said-mcp server.
// Spawns the server ONCE, does the initialize handshake, then exposes call(tool, args)
// that returns { text, ms, reqTokens, respTokens } — real latency + token estimate per call.
// Protocol: MCP 2025-11-25 over newline-delimited JSON-RPC on stdio.
'use strict';
const { spawn } = require('child_process');

const MCP_BIN = process.env.SAID_MCP_BIN
  || 'G:/development/said-build/target/production/said-mcp.exe';

// ~4 chars/token is the standard rough estimate; good enough for a relative token comparison.
const estTokens = (s) => Math.ceil((s ? s.length : 0) / 4);

class McpClient {
  constructor(brainPath, extraEnv = {}) {
    this.brainPath = brainPath;
    this.extraEnv = extraEnv;
    this.nextId = 1;
    this.pending = new Map();   // id -> {resolve, reject, t0}
    this.buf = '';
    this.proc = null;
  }

  start() {
    const args = ['--path', this.brainPath];
    this.proc = spawn(MCP_BIN, args, {
      stdio: ['pipe', 'pipe', 'pipe'],
      env: { ...process.env, ...this.extraEnv },
    });
    this.proc.stdout.on('data', (d) => this._onData(d));
    // server prints "[SCA] Loaded embedded model" etc. to stderr — ignore, but keep for debug
    this.proc.stderr.on('data', () => {});
    this.proc.on('exit', (code) => {
      for (const [, p] of this.pending) p.reject(new Error(`server exited ${code}`));
      this.pending.clear();
    });
  }

  _onData(chunk) {
    this.buf += chunk.toString('utf8');
    let nl;
    while ((nl = this.buf.indexOf('\n')) >= 0) {
      const line = this.buf.slice(0, nl).trim().replace(/^﻿/, '');
      this.buf = this.buf.slice(nl + 1);
      if (!line.startsWith('{')) continue;
      let msg;
      try { msg = JSON.parse(line); } catch { continue; }
      if (msg.id != null && this.pending.has(msg.id)) {
        const p = this.pending.get(msg.id);
        this.pending.delete(msg.id);
        p.resolve({ msg, ms: Date.now() - p.t0 });
      }
    }
  }

  _send(method, params, expectReply = true) {
    const id = this.nextId++;
    const req = { jsonrpc: '2.0', id, method, params };
    const line = JSON.stringify(req) + '\n';
    const p = new Promise((resolve, reject) => {
      if (expectReply) this.pending.set(id, { resolve, reject, t0: Date.now() });
      else resolve({ msg: null, ms: 0 });
    });
    this.proc.stdin.write(line);
    return p;
  }

  _notify(method, params) {
    this.proc.stdin.write(JSON.stringify({ jsonrpc: '2.0', method, params }) + '\n');
  }

  async initialize() {
    const { msg } = await this._send('initialize', {
      protocolVersion: '2025-11-25',
      capabilities: {},
      clientInfo: { name: 'said-harness', version: '1.0' },
    });
    this._notify('notifications/initialized', {});
    return msg && msg.result;
  }

  // call a tool; returns { text, ms, reqTokens, respTokens, raw }
  async call(tool, args = {}, timeoutMs = 600000) {
    const reqStr = JSON.stringify(args);
    const race = Promise.race([
      this._send('tools/call', { name: tool, arguments: args }),
      new Promise((_, rej) => setTimeout(() => rej(new Error(`timeout ${tool}`)), timeoutMs)),
    ]);
    const { msg, ms } = await race;
    let text = '';
    if (msg && msg.result && Array.isArray(msg.result.content)) {
      text = msg.result.content.map((c) => c.text || '').join('\n');
    } else if (msg && msg.error) {
      text = 'ERROR: ' + JSON.stringify(msg.error);
    }
    return { text, ms, reqTokens: estTokens(reqStr), respTokens: estTokens(text), raw: msg };
  }

  stop() { try { this.proc.stdin.end(); this.proc.kill(); } catch {} }
}

module.exports = { McpClient, estTokens };
