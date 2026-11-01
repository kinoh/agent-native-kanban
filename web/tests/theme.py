"""Verify the saved light/dark preference against an isolated built frontend.

Run after ``npm --prefix web run build``: ``python3 web/tests/theme.py``.
It uses only port 18812 and writes screenshots beneath the KAN-7 state directory.
"""
import json
import threading
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parents[1]
STATE = Path.home() / ".local/state/agent-native-kanban/kan-7"
SCREENSHOTS = STATE / "screenshots"
SCREENSHOTS.mkdir(parents=True, exist_ok=True)


class Handler(SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(WEB / "dist"), **kwargs)

    def do_GET(self):
        if not self.path.startswith("/assets/"):
            self.path = "/index.html"
        super().do_GET()

    def log_message(self, *_args):
        pass


stage = {"id": "stage-1", "name": "実装", "position": 0, "requires_work": True, "claim_permission": "claim", "is_final": False}
project = {"id": "project-1", "key": "THEME", "name": "テーマ確認", "description": "", "stages": [stage], "transition_rules": []}
me = {"id": "human-1", "kind": "human", "display_name": "Theme User", "email": "theme@example.test", "organization": {"id": "org-1", "name": "Theme Org"}, "is_org_admin": True, "grants": [{"project_id": None, "permission": "admin"}]}


def task():
    work = {"readiness": "ready", "claimable": True, "can_claim": True, "claim_permission": "claim", "has_claim_permission": True, "reasons": [], "occupant": None, "activity": None, "is_working": False, "acquired_at": None, "claim_id": None, "hold_reason": None, "blockers": [], "hidden_blockers": 0}
    return {"id": "task-1", "key": "THEME-1", "project_id": project["id"], "title": "配色を確認する", "description": "", "purpose": "", "acceptance_criteria": "", "stage": stage, "responsible": None, "assignee": None, "priority": "none", "due_at": None, "parent_id": None, "version": 1, "created_at": "2026-11-01T00:00:00.000Z", "updated_at": "2026-11-01T00:00:00.000Z", "work": work}


def route(response, request):
    path = request.url.split("/api/v1", 1)[1]
    if path == "/me":
        body = me
    elif path == "/projects":
        body = {"projects": [project]}
    elif path == "/me/inbox":
        body = {"requests": []}
    elif path.startswith("/tasks?"):
        body = {"tasks": [task()], "next_cursor": None}
    elif path.startswith("/events/stream?"):
        response.fulfill(status=200, content_type="text/event-stream", body="")
        return
    elif path == "/signup/test-token":
        body = {"organization": "Theme Org", "note": "theme", "expires_at": "2099-01-01T00:00:00.000Z"}
    else:
        raise AssertionError(f"unexpected API request: {path}")
    response.fulfill(status=200, content_type="application/json", body=json.dumps(body))


def rgb(value):
    return [int(part) for part in value.removeprefix("rgb(").removesuffix(")").split(", ")]


def luminance(channels):
    normalized = [channel / 255 for channel in channels]
    linear = [channel / 12.92 if channel <= 0.04045 else ((channel + 0.055) / 1.055) ** 2.4 for channel in normalized]
    return 0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2]


def contrast(foreground, background):
    first, second = luminance(rgb(foreground)), luminance(rgb(background))
    return (max(first, second) + 0.05) / (min(first, second) + 0.05)


server = ThreadingHTTPServer(("127.0.0.1", 18812), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
base = "http://127.0.0.1:18812"
try:
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(headless=True)
        for theme in ("light", "dark"):
            context = browser.new_context(color_scheme=theme)
            context.route("**/api/v1/**", route)
            context.add_init_script(
                """() => {
                  localStorage.setItem('agent-native-kanban.theme', '""" + theme + """');
                  window.EventSource = class { close() {} };
                }"""
            )
            page = context.new_page()
            page.goto(f"{base}/projects/THEME/board", wait_until="domcontentloaded")
            page.get_by_role("button", name="ダーク" if theme == "dark" else "ライト", exact=True).wait_for()
            assert page.locator("html").get_attribute("data-theme") == theme
            assert page.evaluate("getComputedStyle(document.documentElement).colorScheme") == theme
            foreground, background = page.evaluate("[getComputedStyle(document.body).color, getComputedStyle(document.body).backgroundColor]")
            assert contrast(foreground, background) >= 4.5
            page.screenshot(path=str(SCREENSHOTS / f"board-{theme}.png"), full_page=True)
            page.goto(f"{base}/signup/test-token")
            page.get_by_role("heading", name="Theme Org に参加").wait_for()
            assert page.locator("html").get_attribute("data-theme") == theme
            page.screenshot(path=str(SCREENSHOTS / f"signup-{theme}.png"), full_page=True)
            page.goto(f"{base}/projects/THEME/board")
            page.get_by_role("button", name="ダーク" if theme == "dark" else "ライト", exact=True).wait_for()
            page.get_by_role("button", name="ライト" if theme == "dark" else "ダーク", exact=True).click()
            chosen = "light" if theme == "dark" else "dark"
            assert page.locator("html").get_attribute("data-theme") == chosen
            assert page.evaluate("localStorage.getItem('agent-native-kanban.theme')") == chosen
            page.reload()
            page.get_by_role("button", name="ダーク" if chosen == "dark" else "ライト", exact=True).wait_for()
            assert page.locator("html").get_attribute("data-theme") == chosen
            context.close()

        login = browser.new_context(color_scheme="dark")
        login.route("**/api/v1/**", lambda request: request.fulfill(status=401, content_type="application/json", body=json.dumps({"error": {"code": "unauthenticated", "message": "not signed in"}})))
        login.add_init_script("localStorage.setItem('agent-native-kanban.theme', 'light')")
        page = login.new_page()
        page.goto(base)
        page.get_by_role("heading", name="ログイン").wait_for()
        assert page.locator("html").get_attribute("data-theme") == "light"
        page.screenshot(path=str(SCREENSHOTS / "login-light.png"), full_page=True)
        login.close()

        fallback = browser.new_context(color_scheme="dark")
        fallback.route("**/api/v1/**", route)
        warnings = []
        fallback.add_init_script("Storage.prototype.getItem = () => { throw new Error('storage unavailable'); }")
        page = fallback.new_page()
        page.on("console", lambda message: warnings.append(message.text) if message.type == "warning" else None)
        page.goto(f"{base}/projects/THEME/board")
        page.get_by_role("button", name="ダーク", exact=True).wait_for()
        assert page.locator("html").get_attribute("data-theme") == "dark"
        assert any("Could not read theme preference" in warning for warning in warnings)
        fallback.close()
        browser.close()
    print(f"theme checks passed; screenshots: {SCREENSHOTS}")
finally:
    server.shutdown()
    server.server_close()
