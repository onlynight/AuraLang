from pathlib import Path
root = Path(r'D:\Code\AuraLang')
count = 0
for p in root.rglob('*.rs'):
    rel = p.relative_to(root)
    print(f'{rel} -> starts with compiler/: {str(rel).startswith(\"compiler/\")}')
    count += 1
    if count > 5: break
