// Node runner for the built quiche demo (MODULARIZE=instance, EXPORT_ES6).
// Resolves the module instance and invokes the wasm-bindgen `quiche_demo` export.
import * as mod from './quiche-demo.js';

async function inst() {
  const d = mod.default;
  if (typeof d === 'function') { const r = d(); return r && r.then ? await r : r; }
  if (d && d.then) return await d;
  return d;
}

const i = await inst();
const fn = [i, mod, globalThis].find(c => c && typeof c.quiche_demo === 'function')?.quiche_demo;
if (!fn) { console.error('quiche_demo not found; module keys:', Object.keys(mod)); process.exit(3); }

console.log('=== calling quiche_demo() ===');
try {
  const out = await fn.call(null);
  console.log('quiche_demo returned:', JSON.stringify(out));
} catch (e) {
  console.error('quiche_demo threw:', e && (e.stack || e.message || e));
  process.exit(1);
}
console.log('RUNNER-OK');
