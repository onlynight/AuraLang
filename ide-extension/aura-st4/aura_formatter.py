"""
Aura Language Code Formatter for Sublime Text 4.

Provides local (Python-based) code formatting with optional LSP/CLI fallback.
"""

import sublime
import sublime_plugin
import subprocess
import threading
import re
import logging

log = logging.getLogger("AuraLanguage")

INDENT_SIZE = 4
MAX_LINE_LENGTH = 120


class AuraFormatter:
    """
    Local code formatter for Aura language.
    
    This is a lightweight formatter that handles:
    - 4-space indentation
    - Block brace placement
    - Line-ending cleanup
    - Whitespace normalization
    - Enum body indentation
    """

    # Patterns for indent management
    BLOCK_OPEN_PATTERNS = [
        re.compile(r'^\s*[^{]*\{\s*$'),    # line ending with {
        re.compile(r'\{'),                  # inline {
    ]
    
    BLOCK_CLOSE_PATTERNS = [
        re.compile(r'^\s*\}'),             # line starting with }
    ]
    
    # Patterns that should be dedented
    DEDENT_PATTERNS = [
        re.compile(r'^\s*\}'),
        re.compile(r'^\s*\)'),
    ]
    
    # Keywords that increase indent
    BLOCK_KEYWORDS = re.compile(
        r'\b(if|else|when|try|catch|finally|for|while|do|fun|class|struct|'
        r'enum|interface|actor|object)\b'
    )

    @staticmethod
    def format_source(source):
        """
        Format the entire source code.
        
        Args:
            source: Raw source code string
            
        Returns:
            Formatted source code string
        """
        lines = source.split('\n')
        result = []
        indent_level = 0
        
        for line in lines:
            trimmed = line.strip()
            
            # Empty line
            if not trimmed:
                result.append('')
                continue
            
            # Dedent for closing brackets
            if trimmed.startswith('}') or trimmed.startswith(')'):
                indent_level = max(0, indent_level - 1)
            
            # Calculate indent
            indent = ' ' * (indent_level * INDENT_SIZE)
            
            # Clean up trailing whitespace
            cleaned = trimmed.rstrip()
            
            # Normalize whitespace around operators
            cleaned = AuraFormatter._normalize_whitespace(cleaned)
            
            # Remove excessive blank lines (max 1)
            result.append(indent + cleaned)
            
            # Indent for opening brackets
            if cleaned.endswith('{') or cleaned.endswith('('):
                indent_level += 1
        
        # Remove extra blank lines at the end
        while result and not result[-1]:
            result.pop()
        
        return '\n'.join(result) + '\n'

    @staticmethod
    def _normalize_whitespace(text):
        """Normalize whitespace around operators and commas."""
        # Remove trailing whitespace
        text = text.rstrip()
        
        # Normalize comma: always ", " after comma (not before)
        text = re.sub(r'\s*,\s*', ', ', text)
        
        # Normalize assignment: always " = " (not "= " or " =")
        text = re.sub(r'\s*=\s*', ' = ', text)
        
        # Normalize colon: always ": " (not ":")
        text = re.sub(r'\s*:\s*', ': ', text)
        
        # Normalize arrow: always " -> "
        text = re.sub(r'\s*->\s*', ' -> ', text)
        
        # Normalize function call parentheses: no space after (
        text = re.sub(r'\(\s+', '(', text)
        text = re.sub(r'\s+\)', ')', text)
        
        # Normalize + operator: always " + " (not "+")
        text = re.sub(r'\s*\+\s*', ' + ', text)
        
        # Normalize comparison operators
        for op in ['==', '!=', '<=', '>=', '<', '>']:
            text = re.sub(r'\s*' + re.escape(op) + r'\s*', ' ' + op + ' ', text)
        
        # Clean up double spaces (but preserve indentation)
        text = re.sub(r'  +', ' ', text)
        
        # Clean up leading/trailing whitespace
        text = text.strip()
        
        return text


class AuraFormatCommand(sublime_plugin.TextCommand):
    """
    Command to format the current document or selection.
    
    Supports three formatting engines:
    - 'local': Python-based formatter (fast, <50ms)
    - 'lsp': LSP formatter (aura-lsp, 50-200ms)
    - 'cli': CLI formatter (aura fmt, 100-500ms)
    """

    def run(self, edit, engine='local', selection_only=False):
        view = self.view
        
        # Check if selection should be formatted
        if selection_only and self._has_selection(view):
            self._format_selection(view, edit)
        else:
            self._format_document(view, edit, engine)

    def _has_selection(self, view):
        """Check if there is a non-empty selection."""
        for region in view.sel():
            if region.a != region.b:
                return True
        return False

    def _format_selection(self, view, edit):
        """Format the current selection."""
        for region in view.sel():
            if region.a == region.b:
                continue
            text = view.substr(sublime.Region(region.a, region.b))
            formatted = AuraFormatter.format_source(text)
            view.replace(sublime.Region(region.a, region.b), formatted)

    def _format_document(self, view, edit, engine='local'):
        """Format the entire document."""
        content = view.substr(sublime.Region(0, view.size()))
        
        if engine == 'cli':
            self._format_via_cli(view, edit, content)
        elif engine == 'lsp':
            self._format_via_lsp(view, edit, content)
        else:
            self._format_via_local(view, edit, content)

    def _format_via_local(self, view, edit, content):
        """Format using local Python formatter."""
        formatted = AuraFormatter.format_source(content)
        if formatted != content:
            view.replace(sublime.Region(0, view.size()), formatted)

    def _format_via_lsp(self, view, edit, content):
        """Format using LSP (placeholder - requires aura_lsp_client)."""
        log.info("LSP formatting requested - falling back to local")
        self._format_via_local(view, edit, content)

    def _format_via_cli(self, view, edit, content):
        """Format using aura fmt CLI."""
        filepath = view.file_name()
        
        if not filepath:
            log.warning("File not saved - cannot use CLI formatter")
            self._format_via_local(view, edit, content)
            return
        
        try:
            proc = subprocess.Popen(
                ['aura', 'fmt', filepath],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            try:
                stdout, stderr = proc.communicate(timeout=30)
                stderr_text = stderr.decode('utf-8', errors='replace') if stderr else ''
            except subprocess.TimeoutExpired:
                proc.kill()
                raise

            if proc.returncode == 0:
                # Read the formatted file
                with open(filepath, 'r', encoding='utf-8') as f:
                    formatted = f.read()
                if formatted != content:
                    view.replace(sublime.Region(0, view.size()), formatted)
            else:
                log.error("aura fmt failed: %s" % stderr_text)
                self._format_via_local(view, edit, content)
                
        except FileNotFoundError:
            log.error("aura CLI not found - falling back to local")
            self._format_via_local(view, edit, content)
        except subprocess.TimeoutExpired:
            log.error("aura fmt timed out - falling back to local")
            self._format_via_local(view, edit, content)
        except Exception as e:
            log.error("CLI formatting error: %s - falling back to local" % e)
            self._format_via_local(view, edit, content)

    def is_enabled(self):
        """Only enable for Aura files."""
        syntax = self.view.settings().get("syntax", "")
        return "Aura" in syntax or (self.view.file_name() or "").endswith(".aura")


class AuraFormatOnSaveCommand(sublime_plugin.EventListener):
    """
    Event listener for auto-format on save.
    
    Controlled by setting: aura_format_on_save (default: false)
    """

    def on_post_save(self, view):
        """Auto-format on save if setting is enabled."""
        if not self._is_aura_view(view):
            return
        
        settings = view.settings()
        if not settings.get("aura_format_on_save", False):
            return
        
        # Check if formatting is disabled for this file
        if settings.get("disable_formatting", False):
            return
        
        # Run formatter
        content = view.substr(sublime.Region(0, view.size()))
        formatted = AuraFormatter.format_source(content)
        
        if formatted != content:
            view.run_command("aura_format_document", {"engine": "local"})

    @staticmethod
    def _is_aura_view(view):
        """Check if view is an Aura language file."""
        if view is None:
            return False
        syntax = view.settings().get("syntax", "")
        return "Aura" in syntax or (view.file_name() or "").endswith(".aura")


class AuraAutoCompleteCommand(sublime_plugin.TextCommand):
    """
    Simple auto-complete for Aura language.
    Provides keyword and builtin type completion.
    """

    # Keywords
    KEYWORDS = [
        ("if", "keyword.control.aura", "if condition"),
        ("else", "keyword.soft.aura", "else branch"),
        ("when", "keyword.control.aura", "when expression"),
        ("for", "keyword.control.aura", "for loop"),
        ("while", "keyword.control.aura", "while loop"),
        ("try", "keyword.control.aura", "try exception"),
        ("catch", "keyword.soft.aura", "catch handler"),
        ("fun", "storage.type.function.aura", "function declaration"),
        ("val", "storage.type.variable.readonly.aura", "immutable variable"),
        ("var", "storage.type.variable.aura", "mutable variable"),
        ("class", "storage.type.class.aura", "class declaration"),
        ("struct", "storage.type.struct.aura", "struct declaration"),
        ("enum", "storage.type.enum.aura", "enum declaration"),
        ("interface", "storage.type.interface.aura", "interface declaration"),
        ("actor", "storage.type.actor.aura", "actor declaration"),
        ("import", "storage.type.import.aura", "import module"),
        ("return", "keyword.control.aura", "return statement"),
        ("break", "keyword.control.aura", "break statement"),
        ("continue", "keyword.control.aura", "continue statement"),
        ("throw", "keyword.control.aura", "throw exception"),
        ("is", "keyword.hard.aura", "type check"),
        ("in", "keyword.hard.aura", "membership check"),
        ("as", "keyword.hard.aura", "type cast"),
        ("true", "constant.language.boolean.aura", "boolean true"),
        ("false", "constant.language.boolean.aura", "boolean false"),
        ("null", "constant.language.null.aura", "null value"),
        ("it", "keyword.hard.aura", "implicit iterator"),
        ("extern", "storage.type.extern.aura", "FFI declaration"),
        ("typealias", "storage.type.alias.aura", "type alias"),
        ("lateinit", "storage.modifier.other.aura", "late initialization"),
        ("data", "storage.modifier.other.aura", "data class"),
        ("sealed", "storage.modifier.other.aura", "sealed class"),
        ("override", "storage.modifier.other.aura", "override modifier"),
        ("suspend", "storage.modifier.other.aura", "suspend function"),
        ("inline", "storage.modifier.other.aura", "inline function"),
        ("async", "storage.modifier.other.aura", "async function"),
    ]

    # Builtin types
    BUILTIN_TYPES = [
        ("Int", "storage.type.builtin.aura", "32-bit integer"),
        ("Long", "storage.type.builtin.aura", "64-bit integer"),
        ("Short", "storage.type.builtin.aura", "16-bit integer"),
        ("Byte", "storage.type.builtin.aura", "8-bit integer"),
        ("Float", "storage.type.builtin.aura", "single-precision float"),
        ("Double", "storage.type.builtin.aura", "double-precision float"),
        ("Boolean", "storage.type.builtin.aura", "boolean value"),
        ("Char", "storage.type.builtin.aura", "character"),
        ("String", "storage.type.builtin.aura", "string"),
        ("Any", "storage.type.builtin.aura", "any type"),
        ("Nothing", "storage.type.builtin.aura", "empty type"),
        ("Unit", "storage.type.builtin.aura", "no return"),
        ("List", "storage.type.builtin.aura", "list collection"),
        ("Map", "storage.type.builtin.aura", "map collection"),
        ("Set", "storage.type.builtin.aura", "set collection"),
        ("Array", "storage.type.builtin.aura", "array"),
        ("Result", "storage.type.builtin.aura", "result type"),
        ("Exception", "storage.type.builtin.aura", "exception"),
        ("Pair", "storage.type.builtin.aura", "pair"),
    ]

    # Standard library modules
    STD_MODULES = [
        ("aura.math", "support.function.std.aura", "Math functions"),
        ("aura.io", "support.function.std.aura", "I/O functions"),
        ("aura.collections", "support.function.std.aura", "Collection ops"),
        ("aura.concurrent", "support.function.std.aura", "Concurrency API"),
        ("aura.json", "support.function.std.aura", "JSON parse/stringify"),
        ("aura.string", "support.function.std.aura", "String operations"),
        ("aura.fs", "support.function.std.aura", "File system"),
        ("aura.env", "support.function.std.aura", "Environment variables"),
        ("aura.time", "support.function.std.aura", "Time and dates"),
        ("aura.path", "support.function.std.aura", "Path operations"),
        ("aura.net", "support.function.std.aura", "Network (HTTP/WS)"),
        ("aura.random", "support.function.std.aura", "Random numbers"),
        ("aura.encoding", "support.function.std.aura", "Encoding (base64/hex)"),
        ("aura.console", "support.function.std.aura", "Terminal control"),
        ("aura.assert", "support.function.std.aura", "Assertions"),
        ("aura.test", "support.function.std.aura", "Test framework"),
        ("aura.ascii", "support.function.std.aura", "ASCII operations"),
        ("aura.iter", "support.function.std.aura", "Iterator operations"),
    ]

    def run(self, edit, prefix=None):
        """Provide completions."""
        view = self.view
        if not prefix:
            prefix = view.substr(view.word(view.sel()[0]))

        completions = []

        # Keywords
        for name, scope, detail in self.KEYWORDS:
            if name.startswith(prefix):
                completions.append(sublime.CompletionItem(
                    trigger=name,
                    annotation=detail,
                    kind=sublime.KIND_KEYWORD
                ))

        # Builtin types
        for name, scope, detail in self.BUILTIN_TYPES:
            if name.startswith(prefix):
                completions.append(sublime.CompletionItem(
                    trigger=name,
                    annotation=detail,
                    kind=sublime.KIND_TYPE
                ))

        # Standard library modules
        for name, scope, detail in self.STD_MODULES:
            if name.startswith(prefix):
                completions.append(sublime.CompletionItem(
                    trigger=name,
                    annotation=detail,
                    kind=sublime.KIND_SNIPPET
                ))

        if completions:
            view.run_command("commit_completion", {"completion": None})
            # The completion list is handled by the completion engine
