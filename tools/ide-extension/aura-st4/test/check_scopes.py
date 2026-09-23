"""
Check Aura syntax highlighting by reading the sublime module.
Run via: subl.exe -w -c "python check_scopes.py" file.aura
Or: Use sublime_text.exe with --command flag
"""
import sublime
import sublime_plugin
import sys


def check_scopes(view):
    """Check scopes at each position in the view."""
    content = view.substr(sublime.Region(0, view.size()))
    syntax = view.settings().get("syntax", "")
    
    print("=" * 60)
    print("AURA SYNTAX HIGHLIGHTING REPORT")
    print("=" * 60)
    print(f"ST Version: {sublime.version()}")
    print(f"Syntax: {syntax}")
    print(f"File: {view.file_name()}")
    print(f"Size: {view.size()}")
    print("")
    
    # Check syntax is Aura
    is_aura = "Aura" in syntax or (view.file_name() or "").endswith(".aura")
    if not is_aura:
        print("*** WARNING: NOT using Aura syntax! ***")
        print()
    
    # Check scopes at line starts
    pos = 0
    scope_counts = {}
    for line_num, line in enumerate(content.split('\n')):
        if pos >= view.size():
            break
        scope = view.scope_name(pos)
        # Count unique scopes
        scope_key = scope[:60]
        scope_counts[scope_key] = scope_counts.get(scope_key, 0) + 1
        pos += len(line) + 1
    
    print("Unique scopes found:")
    for scope, count in sorted(scope_counts.items(), key=lambda x: -x[1])[:30]:
        print(f"  [{scope}] x{count}")
    
    print()
    print("=" * 60)


class CheckScopesCommand(sublime_plugin.TextCommand):
    def run(self, edit):
        check_scopes(self.view)
