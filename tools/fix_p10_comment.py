from pathlib import Path
p = Path(r'D:\Code\AuraLang\compiler\src\vm\native.rs')
lines = p.read_text(encoding='utf-8').split('\n')
lines[207] = '// P10: concurrent runtime (std-concurrent feature, imports aura.lang.std.{Coroutine,Actor,Channel})'
p.write_text('\n'.join(lines), encoding='utf-8')
print('Fixed line 208')
