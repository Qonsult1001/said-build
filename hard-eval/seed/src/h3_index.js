// Existing consumer of the store (multi-file context). Must keep working.
const { Store } = require('./h3_store.js');

function makeSession() {
  const s = new Store();
  return {
    login(user) { s.set('user', user); },
    current() { return s.get('user'); },
    // A short-lived token helper that SHOULD use the new TTL feature.
    setToken(tok, ttlMs) { s.set('token', tok, ttlMs); },
    token() { return s.get('token'); },
    _store: s,
  };
}
module.exports = { makeSession };
