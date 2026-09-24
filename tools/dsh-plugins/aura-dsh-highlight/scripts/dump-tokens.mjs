import { tokenizeAura } from '../lib/highlight.js';

const code = [
  '// a line comment',
  'fun main() {',
  '    println("Hello, Aura!")',
  '}',
].join('\n');

const tokens = await tokenizeAura(code, { theme: 'light' });
tokens.forEach((line, i) => {
  for (const token of line) {
    console.log(`L${i} ${JSON.stringify(token.content)}  ->  ${token.color}`);
  }
});
