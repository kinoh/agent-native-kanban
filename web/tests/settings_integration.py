"""Run account/admin browser checks against an isolated real server.

It never prints credentials, cookies, signup links, or database contents.
Run after `npm run build` and `cargo build`: python3 web/tests/settings_integration.py
"""
import os, socket, subprocess, tempfile, time, urllib.request
from pathlib import Path
from playwright.sync_api import sync_playwright

ROOT=Path(__file__).resolve().parents[2]
STATE=Path.home()/'.local/state/agent-native-kanban/settings-integration'
STATE.mkdir(parents=True,exist_ok=True)
os.environ['TMPDIR']=str(STATE); tempfile.tempdir=str(STATE)
run=Path(tempfile.mkdtemp(prefix='run-',dir=STATE)); db=run/'test.sqlite'
requested_port=os.environ.get('KANBAN_TEST_PORT')
sock=socket.socket(); sock.bind(('127.0.0.1',int(requested_port) if requested_port else 0)); port=sock.getsockname()[1]; sock.close()
base=f'http://127.0.0.1:{port}'; binary=ROOT/'target/debug/agent-native-kanban'
bootstrap=subprocess.run([str(binary),'bootstrap','--db',str(db),'--public-url',base,'--org','Integration','--expires-hours','1'],capture_output=True,text=True,check=True)
admin_signup=bootstrap.stdout.strip().replace(base,'')
server=subprocess.Popen([str(binary),'serve','--db',str(db),'--listen',f'127.0.0.1:{port}','--public-url',base,'--web-dist',str(ROOT/'web/dist')],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
try:
    for _ in range(40):
        try: urllib.request.urlopen(base,timeout=.2); break
        except OSError: time.sleep(.1)
    else: raise RuntimeError('isolated server did not start')
    with sync_playwright() as p:
        browser=p.chromium.launch(headless=True)
        admin=browser.new_context(); page=admin.new_page()
        page.goto(base+admin_signup); page.get_by_label('表示名').fill('Administrator'); page.get_by_label('メールアドレス').fill('admin@example.test'); page.get_by_label('パスワード（12文字以上）').fill('admin password 123'); page.get_by_role('button',name='登録する').click(); page.get_by_role('link',name='組織の管理').click(); page.get_by_role('heading',name='組織の管理').wait_for()
        page.get_by_placeholder('用途（例: 開発チーム）').fill('member'); page.get_by_role('button',name='リンクを発行').click(); member_signup=page.locator('.secret code').inner_text().replace(base,'')
        member=browser.new_context(); user=member.new_page(); user.goto(base+member_signup); user.get_by_label('表示名').fill('Member'); user.get_by_label('メールアドレス').fill('member@example.test'); user.get_by_label('パスワード（12文字以上）').fill('member password 123'); user.get_by_role('button',name='登録する').click(); user.get_by_role('link',name='Member').click(); user.get_by_role('heading',name='アカウント設定').wait_for()
        second=browser.new_context(); peer=second.new_page(); peer.goto(base); peer.get_by_label('メールアドレス').fill('member@example.test'); peer.get_by_label('パスワード').fill('member password 123'); peer.get_by_role('button',name='ログイン').click(); peer.get_by_role('link',name='Member').wait_for()
        user.get_by_label('表示名').fill('Member Renamed'); user.get_by_role('button',name='表示名を保存').click(); user.get_by_role('link',name='Member Renamed').wait_for(); user.reload(); user.get_by_label('表示名').wait_for(); assert user.get_by_label('表示名').input_value()=='Member Renamed'
        user.get_by_label('現在のパスワード').fill('wrong password'); user.get_by_label('新しいパスワード',exact=True).fill('member password 456'); user.get_by_label('新しいパスワードを確認').fill('member password 456'); user.get_by_role('button',name='パスワードを変更').click(); user.get_by_text('現在のパスワードが違います').wait_for()
        user.get_by_label('現在のパスワード').fill('member password 123'); user.get_by_role('button',name='パスワードを変更').click(); user.get_by_text('変更しました').wait_for(); user.reload(); user.get_by_role('heading',name='アカウント設定').wait_for()
        peer.reload(); peer.get_by_role('heading',name='ログイン').wait_for()
        fresh=browser.new_context(); login=fresh.new_page(); login.goto(base); login.get_by_label('メールアドレス').fill('member@example.test'); login.get_by_label('パスワード').fill('member password 123'); login.get_by_role('button',name='ログイン').click(); login.get_by_text('メールアドレスまたはパスワードが正しくありません').wait_for(); login.get_by_label('パスワード').fill('member password 456'); login.get_by_role('button',name='ログイン').click(); login.get_by_role('link',name='Member Renamed').wait_for()
        page.get_by_label('組織名').fill('Integration Renamed'); page.get_by_role('button',name='組織名を保存').click(); page.locator('.org').get_by_text('Integration Renamed').wait_for(); page.get_by_placeholder('エージェント名').fill('Integration Agent'); page.get_by_role('button',name='エージェントを登録').click(); page.get_by_text('Integration Agent').wait_for()
        for name,new in [('Integration Agent','Agent Renamed'),('Member Renamed','Member Admin Renamed')]:
            page.locator('tr').filter(has_text=name).get_by_role('button',name='表示名を変更').click(); dialog=page.get_by_role('dialog',name='表示名を変更'); dialog.get_by_label('表示名').fill(new); dialog.get_by_role('button',name='表示名を保存').click(); page.get_by_text(new).wait_for()
        page.reload(); page.locator('.org').get_by_text('Integration Renamed').wait_for(); page.get_by_text('Agent Renamed').wait_for(); page.get_by_text('Member Admin Renamed').wait_for()
        print('settings integration: account, session, organization, agent, and human rename checks passed')
        browser.close()
finally:
    server.terminate(); server.wait(timeout=5)
