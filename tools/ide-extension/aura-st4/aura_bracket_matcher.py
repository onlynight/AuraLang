"""
Aura Language Smart Bracket Matcher for Sublime Text 4.

Provides enhanced bracket matching, auto-pairing, and smart-skip
functionality for Aura language.
"""

import sublime
import sublime_plugin
import logging

log = logging.getLogger("AuraLanguage")

# Bracket pairs
PAIRS = {
    '(': ')',
    '{': '}',
    '[': ']',
    '"': '"',
    "'": "'",
    '`': '`',
}

# Reverse map for finding matching open bracket
REVERSE_PAIRS = {v: k for k, v in PAIRS.items()}

# Scopes where auto-pairing should be disabled
NO_PAIR_SCOPES = [
    'comment',
    'string',
]


class AuraBracketMatcher(sublime_plugin.EventListener):
    """
    Smart bracket matcher for Aura language files.
    
    Features:
    - Auto-pairing: insert closing bracket when opening bracket is typed
    - Smart-skip: skip closing bracket if already present
    - Auto-delete: remove empty paired brackets when backspace is pressed
    """

    def is_aura_view(self, view):
        """Check if view is an Aura language file."""
        if view is None:
            return False
        syntax = view.settings().get("syntax", "")
        return "Aura" in syntax or (view.file_name() or "").endswith(".aura")

    def on_text_inserted(self, view, text, regions):
        """Called after text insertion - handle auto-pairing."""
        if not self.is_aura_view(view):
            return
        
        if len(regions) == 0:
            return
        
        region = regions[0]
        pos = region.a

        # Skip if not in a pairing context
        if not self._should_auto_pair(view, pos):
            return

        # Check if inserted text is an opening bracket
        if text in PAIRS:
            self._auto_pair(view, pos, text, PAIRS[text])

    def on_pre_input(self, view, key, shift, ctrl, alt):
        """
        Handle input before it's inserted.
        Used for smart-skip of closing brackets and smart-delete of empty pairs.
        """
        if not self.is_aura_view(view):
            return

        # Smart-skip: if user types closing bracket and next char is also closing bracket
        if key in REVERSE_PAIRS:
            close_char = REVERSE_PAIRS.get(key)
            if close_char:
                # Check if closing bracket is next
                sel = view.sel()
                if sel and sel[0].a < view.size():
                    next_char = view.substr(sublime.Region(sel[0].a, sel[0].a + 1))
                    if next_char == key:
                        # Skip past the closing bracket
                        view.sel().clear()
                        view.sel().add(sublime.Region(sel[0].a + 1))
                        return  # Return to indicate this was handled

    def on_pre_backspace(self, view):
        """
        Handle backspace - auto-delete empty bracket pairs.
        """
        if not self.is_aura_view(view):
            return

        sel = view.sel()
        if not sel:
            return

        pos = sel[0].a
        if pos == 0 or pos >= view.size():
            return

        # Get surrounding characters
        prev_char = view.substr(sublime.Region(pos - 1, pos))
        next_char = view.substr(sublime.Region(pos, pos + 1))

        # Check if we're between empty brackets
        if prev_char in PAIRS and next_char == PAIRS[prev_char]:
            # Delete both brackets
            view.run_command("delete", {"forward": False})
            view.run_command("delete")
            return

    def _should_auto_pair(self, view, pos):
        """Check if auto-pairing should be applied at position."""
        if pos < 0 or pos >= view.size():
            return False

        # Check if we're inside a string or comment
        scope = view.scope_name(pos)
        for no_scope in NO_PAIR_SCOPES:
            if no_scope in scope:
                return False

        # Check if the character after position is already the closing bracket
        if pos < view.size():
            next_char = view.substr(sublime.Region(pos, pos + 1))
            if next_char in PAIRS.values():
                return False

        return True

    def _auto_pair(self, view, pos, open_char, close_char):
        """
        Insert closing bracket after opening bracket.
        
        Args:
            view: The view
            pos: Position of opening bracket
            open_char: The opening bracket character
            close_char: The corresponding closing bracket character
        """
        # Only auto-pair if the next character is not the closing bracket
        if pos < view.size():
            next_char = view.substr(sublime.Region(pos, pos + 1))
            if next_char == close_char:
                # Already has closing bracket, just move cursor past it
                view.sel().clear()
                view.sel().add(sublime.Region(pos + 2))  # After open + close
                return

        # Insert closing bracket after opening bracket
        view.run_command("insert", {"characters": close_char})
        
        # Move cursor between the brackets
        view.sel().clear()
        view.sel().add(sublime.Region(pos + 1))

    def on_post_save(self, view):
        """No-op, but required for the class structure."""
        pass

    def on_modified(self, view):
        """No-op, but required for the class structure."""
        pass
