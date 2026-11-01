"""Verify theme persistence and readable content in an isolated built frontend.

Run after ``npm --prefix web run build``: ``python3 web/tests/theme.py``.
The static server uses an OS-assigned port. Screenshots are retained under the
shared theme-test state directory, not under an individual worktree's state.
"""
import json
import threading
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from playwright.sync_api import sync_playwright

WEB = Path(__file__).resolve().parents[1]
SCREENSHOTS = Path.home() / ".local/state/agent-native-kanban/theme/screenshots"
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
human = {"id": "human-1", "kind": "human", "display_name": "Theme User", "status": "active", "description": "", "created_by": None, "created_at": "2026-11-01T00:00:00.000Z", "email": "theme@example.test"}
agent = {"id": "agent-1", "kind": "agent", "display_name": "Theme Agent", "status": "active", "description": "", "created_by": "human-1", "created_at": "2026-11-01T00:00:00.000Z", "email": None}
agent_child = {"id": "agent-2", "kind": "agent", "display_name": "Theme Builder", "status": "active", "description": "", "created_by": "agent-1", "created_at": "2026-11-01T00:00:00.000Z", "email": None}
missing_creator = {"id": "agent-3", "kind": "agent", "display_name": "Theme Former", "status": "active", "description": "", "created_by": "deleted-principal", "created_at": "2026-11-01T00:00:00.000Z", "email": None}
independent = {"id": "human-2", "kind": "human", "display_name": "Theme Independent", "status": "active", "description": "", "created_by": None, "created_at": "2026-11-01T00:00:00.000Z", "email": "independent@example.test"}


def task():
    work = {"readiness": "ready", "claimable": True, "can_claim": True, "claim_permission": "claim", "has_claim_permission": True, "reasons": [], "occupant": None, "activity": None, "is_working": False, "acquired_at": None, "claim_id": None, "hold_reason": None, "blockers": [], "hidden_blockers": 0}
    return {"id": "task-1", "key": "THEME-1", "project_id": project["id"], "title": "配色を確認する", "description": "本文を読みやすく表示する。", "purpose": "両テーマを確認する。", "acceptance_criteria": "本文のコントラストを確認する。", "stage": stage, "responsible": None, "assignee": None, "priority": "none", "due_at": None, "parent_id": None, "version": 1, "created_at": "2026-11-01T00:00:00.000Z", "updated_at": "2026-11-01T00:00:00.000Z", "work": work}


def api_route(state):
    def route(response, request):
        path = request.url.split("/api/v1", 1)[1]
        if path == "/auth/logout" and request.method == "POST":
            state["authenticated"] = False
            response.fulfill(status=204)
            return
        if path == "/auth/login" and request.method == "POST":
            state["authenticated"] = True
            response.fulfill(status=200, content_type="application/json", body="{}")
            return
        if path == "/me":
            if not state["authenticated"]:
                response.fulfill(status=401, content_type="application/json", body=json.dumps({"error": {"code": "unauthenticated", "message": "not signed in"}}))
                return
            body = me
        elif path == "/projects": body = {"projects": [project]}
        elif path == "/me/inbox": body = {"requests": [{"id": "inbox-1", "kind": "question", "body": "テーマの確認をお願いします。", "created_at": "2026-11-01T00:00:00.000Z", "task": {"id": "task-1", "key": "THEME-1", "title": "配色を確認する"}, "author": {"id": "agent-1", "display_name": "Theme Agent", "kind": "agent"}}]}
        elif path.startswith("/tasks?"):
            if state["tasks_error"]:
                response.fulfill(status=500, content_type="application/json", body=json.dumps({"error": {"code": "internal_error", "message": "テーマ確認用のエラー表示"}}))
                return
            body = {"tasks": [task()], "next_cursor": None}
        elif path == "/tasks/THEME-1/context": body = {"task": task(), "parent": None, "children": [], "dependencies": [], "dependents": [], "links": [], "comments": [], "artifacts": [], "history": []}
        elif path == "/principals": body = {"principals": [human, agent, agent_child, missing_creator, independent]}
        elif path == "/principals/work":
            work = {"key": "THEME-1", "title": "配色を確認する", "project": {"id": project["id"], "key": project["key"], "name": project["name"]}, "activity": "working", "acquired_at": "2026-11-01T00:00:00.000Z"}
            body = {"principals": [{**agent_child, "work": []}, {**independent, "work": []}, {**agent, "work": [work, {**work, "key": "THEME-2", "title": "作成関係を確認する"}]}, {**missing_creator, "work": []}, {**human, "work": []}]}
        elif path == "/resources":
            hold = {"id": "hold-1", "reason": "受け入れテストを実行する", "acquired_at": "2026-11-01T00:00:00.000Z", "holder": {"id": agent["id"], "kind": agent["kind"], "display_name": agent["display_name"], "status": agent["status"]}}
            body = {"resources": [{"id": "resource-1", "org_id": "org-1", "project_id": None, "name": "テスト環境", "description": "統合テスト用の環境", "capacity": 1, "created_at": "2026-11-01T00:00:00.000Z", "updated_at": "2026-11-01T00:00:00.000Z", "holds": [hold]}]}
        elif path == "/signup-links": body = {"signup_links": []}
        elif path.startswith("/events/stream?"):
            response.fulfill(status=200, content_type="text/event-stream", body="")
            return
        elif path == "/signup/test-token": body = {"organization": "Theme Org", "note": "theme", "expires_at": "2099-01-01T00:00:00.000Z"}
        else: raise AssertionError(f"unexpected API request: {path}")
        response.fulfill(status=200, content_type="application/json", body=json.dumps(body))
    return route


def rgb(value): return [int(part) for part in value.removeprefix("rgb(").removesuffix(")").split(", ")]


def luminance(channels):
    values = [channel / 255 for channel in channels]
    linear = [value / 12.92 if value <= 0.04045 else ((value + 0.055) / 1.055) ** 2.4 for value in values]
    return 0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2]


def contrast(foreground, background):
    first, second = luminance(rgb(foreground)), luminance(rgb(background))
    return (max(first, second) + 0.05) / (min(first, second) + 0.05)


CONTENT = "main h1, main h2, main h3, main p, main li, main th, main td, main label, main .card-title, main .task-link, main [role=alert], main input, main select, main textarea, main button:not(.primary):not(.link):not(:disabled), .center-card h1, .center-card p, .center-card label, .center-card input, .center-card [role=alert], .modal h2, .modal p, .modal li, .modal label, .modal input, .modal select, .modal textarea, .modal button:not(.primary):not(.link):not(:disabled), .sidebar .org, .sidebar .nav-link, .sidebar .me span, .theme-toggle button"


def assert_readable(page, screen):
    samples = page.locator(CONTENT).evaluate_all("""elements => elements.filter(element => { const box = element.getBoundingClientRect(); return box.width > 0 && box.height > 0 && element.innerText.trim(); }).map(element => { let background = element; while (background) { const color = getComputedStyle(background).backgroundColor; if (color !== 'rgba(0, 0, 0, 0)' && color !== 'transparent') break; background = background.parentElement; } return {name: `${element.tagName}.${element.className}`, foreground: getComputedStyle(element).color, background: getComputedStyle(background || document.body).backgroundColor}; })""")
    assert samples, f"{screen}: readable-content samples were not found"
    failures = [sample for sample in samples if contrast(sample["foreground"], sample["background"]) < 4.5]
    assert not failures, f"{screen}: insufficient content contrast: {failures}"


def verify_page(page, path, ready, theme, screenshot):
    page.goto(f"{base}{path}")
    page.locator(ready).wait_for()
    assert page.locator("html").get_attribute("data-theme") == theme
    assert page.evaluate("getComputedStyle(document.documentElement).colorScheme") == theme
    assert_readable(page, screenshot)
    if path == "/admin":
        rows = page.locator("table.settings-table").nth(1).locator("tbody tr")
        assert rows.count() == 5
        assert rows.evaluate_all("rows => rows.map(row => row.dataset.principal)") == ["human-2", "agent-3", "human-1", "agent-1", "agent-2"]
        indents = rows.evaluate_all("rows => Object.fromEntries(rows.map(row => [row.dataset.principal, getComputedStyle(row.querySelector('.principal-tree')).paddingInlineStart]))")
        assert indents == {"human-2": "0px", "agent-3": "0px", "human-1": "0px", "agent-1": "18px", "agent-2": "36px"}
        assert page.get_by_label("作成元 Theme User").count() == 1
        assert page.get_by_label("作成元 Theme Agent").count() == 1
        assert "委譲" not in "".join(rows.all_inner_texts())
        alignment = rows.nth(3).evaluate("row => { const tree = row.querySelector('.principal-tree').getBoundingClientRect(); const cells = [...row.querySelectorAll('td')].slice(1).map(cell => cell.getBoundingClientRect()); return { treeCenter: tree.y + tree.height / 2, rowHeight: row.getBoundingClientRect().height, cellCenters: cells.map(cell => cell.y + cell.height / 2) }; }")
        assert alignment["rowHeight"] > 40
        assert all(abs(center - alignment["treeCenter"]) < .5 for center in alignment["cellCenters"])
    page.screenshot(path=str(SCREENSHOTS / screenshot), full_page=True)


server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
base = f"http://127.0.0.1:{server.server_port}"
try:
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(headless=True)
        for saved_theme, os_theme in (("light", "dark"), ("dark", "light")):
            state = {"authenticated": True, "tasks_error": False}
            context = browser.new_context(color_scheme=os_theme)
            context.route("**/api/v1/**", api_route(state))
            context.add_init_script("""(() => { if (localStorage.getItem('agent-native-kanban.theme') === null) localStorage.setItem('agent-native-kanban.theme', '""" + saved_theme + """'); window.EventSource = class { constructor() {} close() {} }; })();""")
            page = context.new_page(); prefix = f"{saved_theme}-"
            for path, ready, name in [("/projects/THEME/board", "[data-task=THEME-1]", "board"), ("/resources", ".resource-card", "resources"), ("/projects/THEME/list", "tbody tr", "list"), ("/tasks/THEME-1", "[aria-label=タイトル]", "task-detail"), ("/inbox", "h1", "inbox"), ("/projects/THEME/settings", ".settings-table", "project-settings"), ("/admin", "h1", "admin"), ("/settings", "h1", "account-settings"), ("/signup/test-token", "h1", "signup")]:
                verify_page(page, path, ready, saved_theme, f"{prefix}{name}.png")
            verify_page(page, "/projects", "h1", saved_theme, f"{prefix}projects.png")
            page.get_by_role("button", name="プロジェクトを作成").click(); page.get_by_role("dialog", name="プロジェクトを作成").wait_for()
            assert_readable(page, f"{prefix}modal"); page.screenshot(path=str(SCREENSHOTS / f"{prefix}modal.png"), full_page=True)
            state["tasks_error"] = True
            verify_page(page, "/projects/THEME/board", "[role=alert]", saved_theme, f"{prefix}error.png")
            state["tasks_error"] = False
            verify_page(page, "/projects/THEME/board", "[data-task=THEME-1]", saved_theme, f"{prefix}board-before-login.png")
            page.get_by_role("button", name="ログアウト").click(); page.get_by_role("heading", name="ログイン").wait_for()
            assert page.locator("html").get_attribute("data-theme") == saved_theme
            page.get_by_label("メールアドレス").fill("theme@example.test"); page.get_by_label("パスワード").fill("theme password"); page.get_by_role("button", name="ログイン").click()
            page.get_by_role("button", name="ログアウト").wait_for(); assert page.locator("html").get_attribute("data-theme") == saved_theme
            context.close()
        for os_theme in ("light", "dark"):
            state = {"authenticated": True, "tasks_error": False}
            context = browser.new_context(color_scheme=os_theme); context.route("**/api/v1/**", api_route(state)); context.add_init_script("(() => { window.EventSource = class { constructor() {} close() {} }; })();")
            page = context.new_page(); verify_page(page, "/projects/THEME/board", "[data-task=THEME-1]", os_theme, f"os-{os_theme}-board.png")
            assert page.evaluate("localStorage.getItem('agent-native-kanban.theme')") is None; context.close()
        browser.close()
    print(f"theme checks passed; screenshots: {SCREENSHOTS}")
finally:
    server.shutdown(); server.server_close()
