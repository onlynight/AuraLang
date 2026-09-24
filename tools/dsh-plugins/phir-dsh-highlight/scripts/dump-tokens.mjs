import { tokenizePHIR } from '../lib/highlight.js';

const code = [
  '// a line comment',
  'fun main() {',
  '    println("Hello, PHIR!")',
  '}',
].join('\n');

const tokens = await tokenizePHIR(code, { theme: 'light' });
tokens.forEach((line, i) => {
  for (const token of line) {
    console.log(`L${i} ${JSON.stringify(token.content)}  ->  ${token.color}`);
  }
});


