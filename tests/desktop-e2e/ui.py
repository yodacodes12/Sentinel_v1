"""Drive the real sentinel-desktop window through its accessibility tree (AT-SPI).

Nothing here touches the system: it reads the window's accessible objects and
invokes their default actions (the same as clicking them).
"""
import time

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402


def _children(node):
    try:
        n = node.get_child_count()
    except Exception:
        return []
    out = []
    for i in range(n):
        try:
            c = node.get_child_at_index(i)
        except Exception:
            c = None
        if c is not None:
            out.append(c)
    return out


def _text(node):
    try:
        t = node.get_text_iface()
        if t:
            return Atspi.Text.get_text(t, 0, Atspi.Text.get_character_count(t))
    except Exception:
        pass
    return ""


class UI:
    def __init__(self, app_name="sentinel-desktop", timeout=40, pid=None):
        self.app = None
        self.stale_cache_hits = []
        end = time.time() + timeout
        while time.time() < end and self.app is None:
            desk = Atspi.get_desktop(0)
            for c in _children(desk):
                try:
                    if c.get_name() == app_name and (pid is None or c.get_process_id() == pid):
                        self.app = c
                except Exception:
                    pass
            if self.app is None:
                time.sleep(0.5)
        if self.app is None:
            raise RuntimeError("desktop window not found in the accessibility tree")

    def doc(self, timeout=30):
        end = time.time() + timeout
        while True:
            stack = [self.app]
            while stack:
                n = stack.pop()
                try:
                    if n.get_role_name() == "document web":
                        return n
                except Exception:
                    continue
                stack.extend(_children(n))
            if time.time() > end:
                raise RuntimeError("no web document")
            time.sleep(0.5)

    def walk(self, root=None):
        stack = [root or self.doc()]
        while stack:
            n = stack.pop()
            yield n
            stack.extend(reversed(_children(n)))

    def find_all(self, role=None, name=None, contains=None, root=None):
        out = []
        for n in self.walk(root):
            try:
                if role and n.get_role_name() != role:
                    continue
                # Names are compared without case: WebKit exposes text after
                # CSS text-transform (ECO for "Eco").
                nm = (n.get_name() or "").lower()
                if name is not None and nm != name.lower():
                    continue
                if contains is not None and contains.lower() not in nm:
                    continue
                out.append(n)
            except Exception:
                pass
        return out

    def find(self, role=None, name=None, contains=None, timeout=10):
        end = time.time() + timeout
        while True:
            found = self.find_all(role, name, contains)
            if found:
                return found[0]
            if time.time() > end:
                raise LookupError(f"no {role} named {name or contains!r}")
            time.sleep(0.25)

    def click(self, node):
        act = node.get_action_iface()
        if act is None or Atspi.Action.get_n_actions(act) == 0:
            raise RuntimeError(f"{node.get_name()!r} has no action")
        Atspi.Action.do_action(act, 0)

    def press(self, role, name=None, contains=None, timeout=10):
        n = self.find(role, name, contains, timeout)
        self.click(n)
        return n

    def text(self):
        """All names and text in the page, in document order, one per line."""
        parts = []
        for n in self.walk():
            try:
                nm = n.get_name() or ""
                tx = _text(n)
            except Exception:
                continue
            for s in (nm, tx):
                s = s.replace("￼", "").strip()
                if s and (not parts or parts[-1] != s):
                    parts.append(s)
        return "\n".join(parts)

    def heading(self):
        hs = self.find_all(role="heading")
        return hs[0].get_name() if hs else None

    def wait_text(self, needle, timeout=15, absent=False):
        end = time.time() + timeout
        cleared = False
        start = time.time()
        while True:
            t = self.text()
            if (needle.lower() in t.lower()) != absent:
                if cleared:
                    # Found only after dropping libatspi's client-side cache:
                    # the window had it, this script's cached view did not.
                    self.stale_cache_hits.append(needle)
                return t
            # libatspi caches the tree in this process and can miss updates;
            # after a few seconds, re-read it from the application.
            if not cleared and time.time() - start > 4:
                try:
                    self.app.clear_cache()
                except Exception:
                    pass
                cleared = True
            if time.time() > end:
                raise TimeoutError(f"{'still' if absent else 'never'} saw {needle!r}")
            time.sleep(0.5)

    def set_value(self, node, value):
        """Type into an editable field: replace its whole contents."""
        et = node.get_editable_text_iface()
        n = Atspi.Text.get_character_count(node.get_text_iface())
        Atspi.EditableText.delete_text(et, 0, n)
        Atspi.EditableText.insert_text(et, 0, value, len(value))
