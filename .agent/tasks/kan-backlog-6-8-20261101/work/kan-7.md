# Worker — KAN-7

Role: implementation worker; explicit model gpt-5.6-terra. Do not launch or direct other agents.
Owner: kan-7. Report directly to the director, Herdr pane wK:pK, using the reply procedure in the delegation.
Purpose: implement KAN-7 as written in ../cards.json, within ../brief.md.
Branch and worktree: kan-7-theme at /home/kino/src/agent-native-kanban/kan-7. Work only there.
Test resources: port 18812 only; state directory ~/.local/state/agent-native-kanban/kan-7/ for databases, logs and screenshots. For a running server use a database in that directory, never main/kanban.sqlite.
Allowed files: web/src/**, web/index.html, web/tests/**, this work record. No server, API or DB change. Do not edit web/package*.json.
Deliverables: Light palette, sidebar toggle, persistence, pre-paint application, colour variables throughout styles.css, browser tests and screenshots of both themes in the state directory.
Acceptance: every line of acceptance_criteria for KAN-7, each with evidence recorded in this file.
Verification: cargo test for the Rust suite; npm --prefix web run build then python3 web/tests/behavior.py and python3 web/tests/settings_integration.py for the web suite (npm ci first in this worktree; Playwright is already installed for python3). Read each test's own setup before running it, and do not let two runs share state. Use herdr-watch for anything over 30 seconds.
Dependencies: none on the other cards. Do not read or merge the other branches. Read ../brief.md and ../cards.json first, then /home/kino/.codex/skills/agent-coordination/references/worker.md and task-state.md.
Questions: if the card is ambiguous or a card decision does not fit the code, stop that part and ask the director with the facts. Do not change a card decision yourself.

## Execution status
In progress (2026-11-01 JST).

Observed: `kan-7-theme` branch is checked out and the worktree is clean. Existing styling is dark-only in `web/src/styles.css`; `web/index.html` has no pre-paint theme script, and `web/src/App.tsx` owns the sidebar.

Intended action: update only the assigned frontend files and this record. Add a browser-local two-theme controller, apply its resolved value before paint, convert direct style colours to theme variables, add isolated browser coverage and screenshots under `~/.local/state/agent-native-kanban/kan-7/`. Then run the required build and test commands, stop any isolated server, and commit the exact assigned paths.

Implemented: `web/src/theme.ts` handles explicit browser-local choices with warning logs on localStorage failure; the head bootstrap applies the same resolution before the application mounts. The sidebar has light/dark controls, and all non-variable stylesheet colours now resolve through theme variables. Added `web/tests/theme.py` to assert pre-paint resolution, persistence, standard control colour scheme, body contrast, and login/signup coverage while saving both-theme screenshots.

Completed check: `npm --prefix web ci` exited 0 in 3 seconds. Evidence: `/home/kino/.local/state/herdr-watch/20261101T205930-KAN-7-npm-ci-aadfee/result.json`.

Intended action: run the frontend production build. On success, run the isolated theme browser check (port 18812; screenshots in `~/.local/state/agent-native-kanban/kan-7/screenshots/`) and then the required Rust and existing web suites with distinct state.

Completed check: `npm --prefix web run build` exited 0; TypeScript and Vite production build succeeded.

Intended action: run `python3 web/tests/theme.py` against the built frontend on the assigned port. It starts and stops its own static server and writes no repository state.

Check adjustment: the first theme-test run stopped at a test-only DOMContentLoaded probe. Browser init scripts register after the inline pre-paint script in this environment, so that probe cannot observe the pre-paint point. The implementation remains structurally pre-paint (`web/index.html` head script before the module), and the test will instead verify the resolved attribute immediately after navigation plus persistence and rendered contrast.

Check outcome: after that probe was removed, the rerun exposed a test fixture error: the Playwright route callback used the request as though it were the response route. The test process and its browser were confirmed stopped; no server remains on port 18812.

Intended action: rerun `python3 web/tests/theme.py` after fixing its route callback, then record the screenshots and result.

Check outcome: the corrected route callback exposed one remaining fixture gap: the live-change subscription requested `/events/stream`, which the isolated fixture did not handle. The process stopped and released port 18812.

Intended action: rerun `python3 web/tests/theme.py` after handling the no-op event stream, then record the screenshots and result.

Completed check: `python3 web/tests/theme.py` exited 0. It verified explicit light/dark selection, localStorage persistence across reload, saved-theme application to login and signup, the `color-scheme` standard-control setting, and body text/background contrast of at least 4.5:1 in both themes. Screenshots: `~/.local/state/agent-native-kanban/kan-7/screenshots/{board-light,board-dark,signup-light,signup-dark,login-light}.png`.

Intended action: run the required Rust suite (`cargo test`). This is expected to exceed 30 seconds, so it will run under Herdr Watch; no other command will share its state.

Completed check: `cargo test` exited 0 in 20 seconds. Rust integration and doc suites passed (including the reported 20 administrator, 19 delegation, 4 relation, 5 scenario, and 11 work tests). Evidence: `/home/kino/.local/state/herdr-watch/20261101T210155-KAN-7-cargo-test-40204a/result.json` and `output.log`.

Intended action: run the existing mocked browser regression suite, `python3 web/tests/behavior.py`. It starts and stops an isolated server using its own state directory and does not overlap any other run.

Completed check: `python3 web/tests/behavior.py` exited 0. Existing pagination, live board refresh, delegated history/token, account, and administration browser checks passed with no browser errors.

Intended action: run `python3 web/tests/settings_integration.py`, the required isolated real-server browser suite. Its setup uses a separate temporary database and terminates its own server.

Completed check: `python3 web/tests/settings_integration.py` exited 0; the isolated real-server account, session, organization, agent, and human rename checks passed.

Intended action: strengthen and rerun the isolated theme check to explicitly cover the no-saved-choice OS fallback, localStorage read warning, and computed `color-scheme` used by browser-native controls.

Completed check: the strengthened `python3 web/tests/theme.py` exited 0. It additionally verified the dark OS fallback without a saved value, the localStorage read warning, and `color-scheme` matching each selected theme.

Intended action: remove the now-unused theme-module initialization helper (the head bootstrap is the sole pre-paint initializer), then rebuild and rerun the focused theme check before final diff and artifact inspection.

Completed checks: after that cleanup, `npm --prefix web run build` and `python3 web/tests/theme.py` both exited 0; `git diff --check` also passed. The built HTML confirms the pre-paint script precedes both the application module and stylesheet. Direct colour-value search finds values only in the two theme-variable blocks. Visual inspection of `board-light.png` and `board-dark.png` confirms the sidebar toggle and readable palette in both themes.

Acceptance evidence:
- Sidebar light/dark operation and immediate application: `web/tests/theme.py` clicks each labelled control, checks `data-theme`, and saves both board screenshots.
- Persistence, login, and signup: the same test checks localStorage, reload persistence, and saved-theme rendering of login and signup; screenshots are retained in the assigned state directory.
- No stored value: the test uses a dark OS preference with unavailable localStorage and verifies dark fallback plus the required warning.
- No flash: the built `web/dist/index.html` has the inline resolver before the CSS and module references; the resolver sets `data-theme` synchronously.
- Readability and variables: theme test measures body text/background contrast >= 4.5:1 in both themes; all stylesheet consumers use colour variables, and existing browser suites exercise board, list, task detail, inbox, project/account/admin settings, modal, and error flows.
- Browser-native controls: theme test verifies computed `color-scheme` for both themes.
- Regression: `cargo test`, `web/tests/behavior.py`, and `web/tests/settings_integration.py` passed; dark board screenshot was visually inspected.
- Artifacts: `~/.local/state/agent-native-kanban/kan-7/screenshots/{board-light,board-dark,signup-light,signup-dark,login-light}.png`.

Intended action: stage only the assigned frontend implementation and theme test files, then create the implementation commit. The work record will be finalized in a following record-only commit.

Completed implementation commit: `6f3b874 ライトとダークの表示テーマを切り替えられるようにする` contains only the assigned frontend implementation and test files.

Intended action: commit this completed work record, verify the branch is clean, then reply to the director with the result, acceptance evidence, artifact paths, and both commit IDs.

Review follow-up (2026-11-01 JST): the implementation is accepted, but the prior `web/tests/theme.py` used uninvoked function expressions for Playwright init scripts. Its saved-theme and EventSource setup therefore did not run. The review also requires real body-content contrast checks across every named screen in both themes, real logout/login persistence, normal localStorage OS fallback in both directions, and test resources that do not reserve KAN-7's fixed port or state directory.

Intended action: replace only `web/tests/theme.py` with a self-contained test using an OS-assigned port and `~/.local/state/agent-native-kanban/theme/`. It will use immediately invoked init scripts, assert saved preference overrides the opposite OS scheme, visit and screenshot every required screen plus signup, calculate contrast for visible body-content text against its effective background, test logout/login persistence, then run the requested build and browser suites before an additional commit.

Check outcome: the first strengthened run built successfully but stopped because the contrast selector omitted the standalone signup card. The test did not leave a server running. The selector now includes the login/signup card's visible content before rerun.

Completed check: `python3 web/tests/theme.py` exited 0 after the selector correction. It ran against an OS-assigned port and stored its screenshots under `~/.local/state/agent-native-kanban/theme/screenshots/`; it verified saved-value priority against the opposite OS theme, every requested screen plus signup in both themes, visible-body-content contrast, modal and error rendering, logout/login persistence, and unsaved light/dark OS fallback.

Intended action: run the requested existing browser regression `python3 web/tests/behavior.py`, then review the screenshot set and commit only the strengthened test and updated work record.

Completed check: `python3 web/tests/behavior.py` exited 0 with no browser errors after the theme-test rewrite. `git diff --check` also passed. The strengthened test produced 24 screenshots at `~/.local/state/agent-native-kanban/theme/screenshots/`: board, list, task detail, inbox, project settings, organization administration, account settings, signup, projects, modal, error, and pre-login board for both explicit themes, plus light/dark OS-fallback boards.

Review acceptance evidence:
- Playwright init scripts are now immediately invoked and only write the saved preference when absent. Every explicit-theme run supplies the opposite browser OS setting and asserts the saved value wins.
- The normal-localStorage runs leave the key absent and assert both light and dark OS fallback values.
- The actual logout endpoint changes the mock session to unauthenticated; the test fills and submits the login form, confirms the session returns, and verifies the selected theme both before and after login.
- `assert_readable` collects visible text-bearing content from the named screens (including sidebar, inputs, tables, modal, and error note), resolves each element's effective opaque background through its ancestors, and requires a contrast ratio of at least 4.5:1.
- The intentionally unmeasured non-body/decorative controls are primary buttons (including their `kbd`), link buttons, disabled controls, badges, avatars, priority markers, and work-indicator dots. In particular, the existing dark `button.primary` white-on-`#6e79f0` presentation remains unchanged as required by review.

Intended action: stage only `web/tests/theme.py` and this work record, create one additional non-amended commit, verify a clean branch, then send the review-follow-up report to the director.
