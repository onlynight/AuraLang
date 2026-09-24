import { getHighlighter, createIncrementalState, tokenizeNextChunk, tokenizePHIR, THEME_BY_MODE } from '../lib/highlight.js';

// Typical PHIR line (~35 chars) so that the first 32 KB chunk lands near the
// 930-line mark the user reported.
const unit = 'fun main() { println("Hello, PHIR!") }\n';
const code = unit.repeat(Math.ceil((32 * 1024 * 6) / unit.length));
console.log('code:', code.length, 'chars,', code.split('\n').length, 'lines');

const highlighter = await getHighlighter();
const budget = { chars: 32 * 1024, lines: 4000 };

// 1. Incremental: must advance past the first chunk and finish.
let t0 = Date.now();
const state = createIncrementalState();
let result;
let frames = 0;
do {
  const tf = Date.now();
  result = tokenizeNextChunk(highlighter, code, state, budget, THEME_BY_MODE.light);
  console.log(`frame ${frames}: ${Date.now() - tf}ms consumed=${result.length} lines=${result.lines.length} complete=${result.complete}`);
  frames += 1;
  if (frames > 40) break;
} while (!result.complete);
console.log(`incremental total: ${Date.now() - t0} ms`);
console.log('incremental complete:', result.complete, 'consumed all:', result.length === code.length);

// 2. Whole-document reference.
t0 = Date.now();
const whole = await tokenizePHIR(code, { theme: 'light' });
console.log(`whole: ${Date.now() - t0} ms, lines=${whole.length}`);

// 3. Line-for-line parity with the whole-document result.
const flat = (lines) => lines.map((line) => line.map((t) => t.content).join('')).join('\n');
console.log('text parity:', flat(result.lines) === code);
console.log('line parity vs whole:', result.lines.length === whole.length);


