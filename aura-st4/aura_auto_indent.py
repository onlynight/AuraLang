"""
Aura Language Auto-Indentation Engine for Sublime Text 4.

Provides context-aware automatic indentation for the Aura language.
Handles block-level keywords, bracket matching, and context detection
(strings, comments, multi-line strings).
"""

import sublime
import sublime_plugin
import re
import logging

log = logging.getLogger("AuraLanguage")

INDENT_SIZE = 4  # 4 spaces

# ─────────────────────────────────────────────────────────────────────
# Indentation rules
# ─────────────────────────────────────────────────────────────────────

# Patterns that should increase indent when at end of line
INDENT_INCREASE_PATTERNS = [
    # Opening braces
    re.compile(r'^\s*[^{]*\{\s*$'),         # line ending with {
    re.compile(r'\{'),                       # inline {
    # Opening parens (for constructor params, function calls)
    re.compile(r'^\s*[^()]*\(\s*$'),       # line ending with (
    # After colon (type annotation or lambda)
    re.compile(r'\s*:\s*$'),               # line ending with :
    re.compile(r'\s*=\s*$'),               # line ending with =
    # Keywords that open a block
    re.compile(r'\b(if|else|when|try|catch|finally|for|while|do)\b'),
    re.compile(r'\b(fun|class|struct|enum|interface|actor|object)\b.*\{'),
    # "else ->" or "else {"
    re.compile(r'\belse\b.*\{'),
]

# Patterns that should decrease indent
INDENT_DECREASE_PATTERNS = [
    re.compile(r'^\s*\}'),                 # line starting with }
    re.compile(r'^\s*\)'),                 # line starting with )
]

# Patterns that should NOT increase indent (continuation lines)
NO_INCREASE_PATTERNS = [
    re.compile(r'^\s*//'),                  # comment lines
    re.compile(r'^\s*///'),                 # doc comment lines
    re.compile(r'^\s*/\*'),                 # block comment start
    re.compile(r'^\s*\*'),                  # block comment continuation
    re.compile(r'^\s*\x22\x22\x22'),       # multiline string (triple-quote)
]

# Keywords that trigger indent increase when followed by block
BLOCK_KEYWORDS = re.compile(
    r'\b(if|else|when|try|catch|finally|for|while|do|fun|class|struct|'
    r'enum|interface|actor|object)\b'
)


class AuraAutoIndent(sublime_plugin.EventListener):
    """
    Auto-indentation event listener for Aura language files.

    Listens to text modification events and adjusts indentation
    automatically based on Aura language rules.
    """

    def is_aura_view(self, view):
        """Check if view is an Aura language file."""
        if view is None:
            return False
        syntax = view.settings().get("syntax", "")
        return "Aura" in syntax or view.file_name().endswith(".aura")

    def on_text_inserted(self, view, text, regions):
        """Called after text is inserted in the view."""
        if not self.is_aura_view(view):
            return
        if len(regions) == 0:
            return

        # Get the first region
        region = regions[0]
        # Check if we're inside a string or comment
        scope = view.scope_name(region.a)
        if 'comment' in scope or 'string' in scope:
            return

        # Get current line
        line = view.full_line(region.a)
        line_start = line.begin()
        line_end = line.end()
        line_text = view.substr(line)

        # Handle newlines
        if '\n' in text:
            self._handle_newline(view, line_start)
        elif text == ' ':
            # User typed space, do nothing
            pass
        elif text in ['{', '}', '(', ')', '[', ']']:
            # Bracket handling - let the built-in bracket matcher handle
            # opening brackets, but we can handle closing brackets
            if text in ['}', ')']:
                self._handle_closing_bracket(view, line_start, text)

    def on_modified(self, view):
        """Called when the document is modified."""
        pass

    def on_pre_load(self, view):
        """Called before a document is loaded."""
        pass

    def _handle_newline(self, view, line_start):
        """Handle newline insertion: calculate and apply correct indent."""
        # Find the previous non-empty line
        prev_line_offset = view.text_point_before(line_start) - 1
        if prev_line_offset < 0:
            return

        prev_line = view.full_line(prev_line_offset)
        prev_text = view.substr(prev_line).rstrip()

        if not prev_text:
            return

        # Get the current indentation of the previous line
        prev_indent = self._get_indent(prev_text)

        # Determine indentation adjustment
        new_indent = prev_indent
        adjustment = 0

        # Check for closing bracket at start of new line
        # (handled by _handle_closing_bracket, skip here)

        # Check if previous line ends with opening pattern
        for pattern in INDENT_INCREASE_PATTERNS:
            if pattern.search(prev_text):
                adjustment = INDENT_SIZE
                break
        else:
            # Check for block keywords
            if BLOCK_KEYWORDS.search(prev_text):
                adjustment = INDENT_SIZE

        new_indent = max(0, prev_indent + adjustment)

        # If the previous line was a closing bracket, the indent stays the same
        # (handled separately)

        # Apply indentation
        # First, clear any existing content on the new line
        new_line_end = view.line_from_region(sublime.Region(line_start)).end()
        current_text = view.substr(sublime.Region(line_start, new_line_end))

        if current_text.strip() == '':
            # Only apply if the line is empty
            spaces = ' ' * new_indent
            view.insert(line_start, spaces)

    def _handle_closing_bracket(self, view, line_start, bracket):
        """Handle closing bracket: dedent if appropriate."""
        # Find matching opening bracket
        pos = line_start + 1  # +1 because bracket is at position 0
        if pos >= view.size():
            return

        # Get the rest of the line
        line = view.full_line(line_start)
        line_text = view.substr(line).rstrip()

        # If line starts with closing bracket, dedent
        if line_text.startswith(bracket):
            # Check if we're at the start of the line (only whitespace before bracket)
            stripped = line_text.strip()
            if stripped == bracket or stripped.startswith(bracket):
                # Remove indent and add proper indent (one level less)
                indent_match = re.match(r'^(\s*)', view.substr(line))
                if indent_match:
                    current_indent = len(indent_match.group(1))
                    # Find the matching opening bracket line
                    match_line = self._find_matching_bracket_line(view, line_start, bracket)
                    if match_line:
                        match_line_start = view.text_point_before(match_line)
                        if match_line_start >= 0:
                            match_line = view.full_line(match_line_start)
                            match_text = view.substr(match_line)
                            match_indent = self._get_indent(match_text)
                            target_indent = max(0, match_indent)

                            # Replace indentation
                            indent_text = ' ' * target_indent
                            if current_indent != target_indent:
                                # Delete old indent
                                indent_end = line_start + current_indent
                                view.erase(sublime.Region(line_start, indent_end))
                                # Insert new indent
                                view.insert(line_start, indent_text)

    def _find_matching_bracket_line(self, view, pos, bracket):
        """Find the line containing the matching opening bracket."""
        # Map closing brackets to opening
        bracket_pairs = {')': '(', ']': '[', '}': '{'}
        open_bracket = bracket_pairs.get(bracket)
        if not open_bracket:
            return None

        depth = 1
        scan_pos = pos + 1  # Start after closing bracket
        while scan_pos < view.size() and depth > 0:
            ch = view.substr(sublime.Region(scan_pos, scan_pos + 1))
            if ch == open_bracket:
                depth -= 1
            elif ch == bracket:
                depth += 1
            if depth == 0:
                # Found matching opening bracket
                return scan_pos
            scan_pos += 1
        return None

    @staticmethod
    def _get_indent(line_text):
        """Get the indentation of a line text."""
        stripped = line_text.lstrip()
        if not stripped:
            return 0
        return len(line_text) - len(stripped)

    @staticmethod
    def _is_in_string_or_comment(view, pos):
        """Check if position is inside a string or comment."""
        if pos < 0 or pos >= view.size():
            return False
        scope = view.scope_name(pos)
        return 'comment' in scope or 'string' in scope

    def on_post_load(self, view):
        """Called after a document is loaded."""
        pass

    def on_post_save(self, view):
        """Called after a document is saved."""
        pass


class AuraAutoIndentCommand(sublime_plugin.TextCommand):
    """
    Manual auto-indent command for Aura files.
    Can be triggered via Ctrl+Shift+I or Command Palette.
    """

    def is_enabled(self):
        return "aura" in (self.view.syntax() or "").lower()

    def run(self, edit):
        view = self.view
        # Get all selections
        for region in view.sel():
            # Skip empty selections
            if region.empty():
                continue

            # For multi-line selections, indent all lines
            if region.a != region.b and view.substr(sublime.Region(region.a, region.b)).count('\n') > 0:
                self._indent_region(view, edit, region)
            else:
                # Single line: align to block structure
                self._auto_indent_line(view, edit, region)

    def _indent_region(self, view, edit, region):
        """Indent a multi-line region."""
        # Get the text in the region
        text = view.substr(sublime.Region(region.a, region.b))
        lines = text.split('\n')

        # Calculate base indent (minimum indent among lines)
        indents = [len(line) - len(line.lstrip()) for line in lines]
        base_indent = min(indents) if indents else 0

        # Create new text with adjusted indentation
        new_lines = []
        for line in lines:
            stripped = line.lstrip()
            indent_len = len(line) - len(stripped)
            if stripped == '':
                new_lines.append('')
            else:
                new_lines.append(' ' * (INDENT_SIZE) + stripped)

        new_text = '\n'.join(new_lines)
        view.replace(sublime.Region(region.a, region.b), new_text)

    def _auto_indent_line(self, view, edit, region):
        """Auto-indent a single line."""
        pos = region.a
        line_start = view.line_from_point(pos).begin()
        line_end = view.line_from_point(pos).end()
        line = view.substr(sublime.Region(line_start, line_end))

        # Get previous line
        prev_end = view.text_point_before(line_start) - 1
        if prev_end < 0:
            return

        prev_line = view.line_from_point(prev_end)
        prev_text = view.substr(prev_line).rstrip()

        # Calculate target indent
        target_indent = self._calculate_indent_for_line(view, line_start, prev_text)

        # Apply
        current_indent = len(line) - len(line.lstrip())
        if current_indent != target_indent:
            # Remove old indent
            indent_region = sublime.Region(line_start, line_start + current_indent)
            view.erase(indent_region)
            # Insert new indent
            view.insert(line_start, ' ' * target_indent)

    def _calculate_indent_for_line(self, view, line_start, prev_text):
        """Calculate the target indent for a line."""
        prev_indent = len(prev_text) - len(prev_text.lstrip())
        adjustment = 0

        for pattern in INDENT_INCREASE_PATTERNS:
            if pattern.search(prev_text):
                adjustment = INDENT_SIZE
                break
        else:
            if BLOCK_KEYWORDS.search(prev_text):
                adjustment = INDENT_SIZE

        return max(0, prev_indent + adjustment)
