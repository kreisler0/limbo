// Fails the build when the UI bundle exceeds its budget (plan section 3):
// <= 150 KB JS and <= 40 KB CSS, gzip. The dev-only mock chunk is excluded.
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';

const dist = fileURLToPath(new URL('../dist/assets/', import.meta.url));
const BUDGET = { js: 150 * 1024, css: 40 * 1024 };
const totals = { js: 0, css: 0 };
for (const f of readdirSync(dist)) {
  const p = join(dist, f);
  if (!statSync(p).isFile() || f.startsWith('mock')) continue;
  const ext = f.endsWith('.js') ? 'js' : f.endsWith('.css') ? 'css' : null;
  if (!ext) continue;
  totals[ext] += gzipSync(readFileSync(p)).length;
}
const kb = (n) => (n / 1024).toFixed(1) + ' KB';
console.log(`UI bundle (gzip): JS ${kb(totals.js)} / ${kb(BUDGET.js)}, CSS ${kb(totals.css)} / ${kb(BUDGET.css)}`);
if (totals.js > BUDGET.js || totals.css > BUDGET.css) {
  console.error('UI bundle is over budget');
  process.exit(1);
}
