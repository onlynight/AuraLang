"""
Validate Aura syntax file and report scopes.
Place in Sublime Text Packages folder and run via Command Palette.
"""
import sublime
import sublime_plugin


class AuraValidateSyntax(sublime_plugin.TextCommand):
    """Check if Aura syntax file is loaded and report scopes."""

    def run(self, edit):
        view = self.view
        syntax = view.settings().get("syntax", "")
        content = view.substr(sublime.Region(0, view.size()))
        
        results = []
        results.append("=" * 60)
        results.append("AURA SYNTAX VALIDATION REPORT")
        results.append("=" * 60)
        results.append(f"Syntax: {syntax}")
        results.append(f"File: {view.file_name()}")
        results.append("")

        # Check if syntax is Aura
        if "Aura" not in syntax and not (view.file_name() or "").endswith(".aura"):
            results.append("*** WARNING: This file is NOT using Aura syntax! ***")
            results.append("Please set the syntax to Aura via:")
            results.append("  Ctrl+Shift+P -> 'Select Syntax' -> 'Aura'")
            results.append("")

        # Check scope at various points
        pos = 0
        for line_num, line in enumerate(content.split('\n')):
            if pos >= view.size():
                break
            scope = view.scope_name(pos)
            # Only show interesting scopes
            if any(key in scope for key in ['keyword', 'storage', 'entity', 'string', 
                                            'comment', 'constant', 'variable', 'support',
                                            'punctuation', 'meta']):
                results.append(f"L{line_num+1:3d} [{scope[:90]}]")
            pos += len(line) + 1

        results.append("")
        results.append("=" * 60)
        results.append(f"Total lines checked: {len(content.split(chr(10)))}")
        results.append("=" * 60)

        # Show in console
        print("\n".join(results))
        
        # Also show in popup
        popup_content = "<html><body style='font-family: monospace; font-size: 12px;'>"
        for line in results:
            if line.startswith("***"):
                popup_content += f"<p style='color: red;'><b>{line}</b></p>"
            elif line.startswith("= "):
                popup_content += f"<hr>"
            else:
                popup_content += f"<div>{line}</div>"
        popup_content += "</body></html>"
        
        view.run_command("show_popup", {
            "content": popup_content,
            "max_width": 600,
            "max_height": 800,
        })


class AuraSyntaxList(sublime_plugin.TextCommand):
    """List available syntaxes."""

    def run(self, edit):
        view = self.view
        view.run_command("show_popup", {
            "content": f"Current syntax: {view.settings().get('syntax', '')}",
            "max_width": 400,
            "max_height": 200,
        })
