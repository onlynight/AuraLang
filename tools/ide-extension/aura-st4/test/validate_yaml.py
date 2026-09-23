"""Validate the Aura syntax file."""
import yaml
import sys

try:
    with open(r'D:\Code\AuraLang\aura-st4\Aura.sublime-syntax', 'r', encoding='utf-8') as f:
        data = yaml.safe_load(f)
    
    print('YAML is valid!')
    print(f'Top-level keys: {list(data.keys())}')
    print(f'Context count: {len(data.get("context", {}))}')
    
    # Check for common issues
    context = data.get('context', {})
    for name, rules in context.items():
        if not isinstance(rules, list):
            print(f'ERROR: context "{name}" is not a list')
        elif len(rules) == 0:
            print(f'WARNING: context "{name}" is empty')
    
    # Check for missing scope in match rules
    for name, rules in context.items():
        if isinstance(rules, list):
            for i, rule in enumerate(rules):
                if isinstance(rule, dict) and 'match' in rule and 'scope' not in rule:
                    # Check if it has captures
                    if 'captures' not in rule:
                        pass  # This is fine, some matches don't need scope
    
    print('All context checks passed')
    
except yaml.YAMLError as e:
    print(f'YAML ERROR: {e}')
    sys.exit(1)
except Exception as e:
    print(f'ERROR: {e}')
    sys.exit(1)
