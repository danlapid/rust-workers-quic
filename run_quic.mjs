// Node runner for the built demo (MODULARIZE=instance, EXPORT_ES6).
// Resolves the module instance and invokes the wasm-bindgen `quic_demo` export.
import * as mod from './quic-demo.js';

async function inst() {
  const d = mod.default;
  if (typeof d === 'function') { const r = d(); return r && r.then ? await r : r; }
  if (d && d.then) return await d;
  return d;
}

const i = await inst();
const fn = [i, mod, globalThis].find(c => c && typeof c.quic_demo === 'function')?.quic_demo;
if (!fn) { console.error('quic_demo not found; module keys:', Object.keys(mod)); process.exit(3); }

console.log('=== calling quic_demo() ===');
try {
  const out = await fn.call(null);
  console.log('quic_demo returned:', JSON.stringify(out));
} catch (e) {
  console.error('quic_demo threw:', e && (e.stack || e.message || e));
  process.exit(1);
}
console.log('RUNNER-OK');
