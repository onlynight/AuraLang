"""
Validate the Aura.sublime-syntax file using Sublime Text's Python.
Run via: subl.exe -n -c "python validate_syntax.py"
Or manually place in Sublime Text's Plugins folder and run.
"""
import sublime
import sublime_plugin
import json


class ValidateSyntax(sublime_plugin.TextCommand):
    def run(self, edit):
        # Check if Aura syntax file is valid by trying to load it
        try:
            # Check if the syntax file exists
            path = sublime.load_resource("Packages/AuraLanguage/Aura.sublime-syntax")
            if path:
                self.view.run_command("show_popup", {
                    "content": "Syntax file loaded successfully!",
                    "max_width": 400,
                    "max_height": 200,
                })
            else:
                self.view.run_command("show_popup", {
                    "content": "Syntax file NOT found!",
                    "max_width": 400,
                    "max_height": 200,
                })
        except Exception as e:
            self.view.run_command("show_popup", {
                "content": f"Error: {e}",
                "max_width": 400,
                "max_height": 200,
            })


class ValidateHighlight(sublime_plugin.TextCommand):
    def run(self, edit):
        view = self.view
        content = view.substr(sublime.Region(0, view.size()))
        
        # Get scopes at each line start
        scopes = []
        pos = 0
        for line_num, line in enumerate(content.split('\n')):
            if pos >= view.size():
                break
            scope = view.scope_name(pos)
            scopes.append(f"L{line_num+1:3d}: {scope[:80]}")
            pos += len(line) + 1
        
        # Show in console
        print("=" * 60)
        print("AURA SYNTAX HIGHLIGHTING VALIDATION")
        print("=" * 60)
        for s in scopes:
            print(s)
        print("=" * 60)
        
        # Also check syntax
        syntax = view.settings().get("syntax", "")
        print(f"\nCurrent syntax: {syntax}")
        print(f"File: {view.file_name()}")
