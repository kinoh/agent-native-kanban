"""Built UI regression checks with controlled API responses and change notifications.

Run after npm run build: python3 web/tests/behavior.py
Requires the Python Playwright package and its Chromium browser.
"""
import json, os, tempfile, threading
from urllib.parse import parse_qs, urlsplit
from http.server import ThreadingHTTPServer, SimpleHTTPRequestHandler
from pathlib import Path
from playwright.sync_api import sync_playwright

WEB=Path(__file__).resolve().parents[1]
STATE=Path.home()/'.local/state/agent-native-kanban-ui-tests'
STATE.mkdir(parents=True, exist_ok=True)
os.environ['TMPDIR']=str(STATE)
tempfile.tempdir=str(STATE)
class Handler(SimpleHTTPRequestHandler):
    def __init__(self,*args,**kwargs):
        super().__init__(*args,directory=str(WEB/'dist'),**kwargs)
    def do_GET(self):
        if not self.path.startswith('/assets/'):
            self.path='/index.html'
        super().do_GET()
    def log_message(self,*args): pass
server=ThreadingHTTPServer(('127.0.0.1',18813),Handler)
threading.Thread(target=server.serve_forever,daemon=True).start()
base=f'http://127.0.0.1:{server.server_port}'
stage={'id':'stage-1','name':'Implement','position':0,'requires_work':True,'claim_permission':'claim','is_final':False}
new_stage={**stage,'id':'stage-2','name':'Review','position':1}
project={'id':'project-1','key':'DEV','name':'Audit Project','description':'','stages':[stage],'transition_rules':[]}
work={'readiness':'ready','claimable':True,'can_claim':True,'claim_permission':'claim','has_claim_permission':True,'reasons':[],'occupant':None,'activity':None,'is_working':False,'acquired_at':None,'claim_id':None,'hold_reason':None,'blockers':[],'hidden_blockers':0}
def task(i,s=stage):
    return {'id':f'task-{i}','key':f'DEV-{i}','project_id':project['id'],'title':f'Task {i}','description':'','purpose':'','acceptance_criteria':'','stage':s,'responsible':None,'assignee':None,'priority':'none','due_at':None,'parent_id':None,'version':1,'created_at':'2026-11-01T00:00:00.000Z','updated_at':'2026-11-01T00:00:00.000Z','work':work}
state={'mode':'many','moved':False,'display_name':'Auditor','organization_name':'Audit','is_org_admin':True,'reject_profile_patch':False,'refresh_failures':0,'reject_project_patch':False,'principal_work_changed':False}
agent={'id':'agent-1','kind':'agent','display_name':'Orchestrator','status':'active','description':'','created_at':'2026-11-01T00:00:00.000Z','email':None}
human={'id':'human-1','kind':'human','display_name':'Auditor','status':'active','description':'','created_at':'2026-11-01T00:00:00.000Z','email':'auditor@example.test'}
def principal_work():
    agent_work=[
        {'key':'DEV-1','title':'Task 1','project':{'id':project['id'],'key':project['key'],'name':project['name']},'activity':'claimed','acquired_at':'2026-11-01T00:00:00.000Z'},
        {'key':'DEV-2','title':'Task 2','project':{'id':project['id'],'key':project['key'],'name':project['name']},'activity':'working','acquired_at':'2026-11-01T01:00:00.000Z'},
    ]
    if state['principal_work_changed']:
        agent_work=[
            {'key':'DEV-1','title':'Task 1','project':{'id':project['id'],'key':project['key'],'name':project['name']},'activity':'paused','acquired_at':'2026-11-01T00:00:00.000Z'},
        ]
    return {'principals':[
        {**human,'work':[]},
        {**agent,'work':agent_work},
    ]}
FUTURE='2099-01-01T00:00:00.000Z'
def token(id,name,**fields):
    return {'id':id,'name':name,'created_at':'2026-11-01T00:00:00.000Z','expires_at':None,'revoked_at':None,'parent_token_id':None,**fields}
tokens=[
    token('root-token-aaaaaaaa','main'),
    token('live-token-bbbbbbbb','implementer',parent_token_id='root-token-aaaaaaaa',expires_at=FUTURE,scopes=[{'project_id':project['id'],'permission':'claim'},{'project_id':project['id'],'permission':'review'}],tasks=['task-1','task-2'],task_keys=['DEV-1']),
    token('dead-token-cccccccc','revoked',parent_token_id='root-token-aaaaaaaa',expires_at=FUTURE,revoked_at='2026-11-01T00:00:00.000Z',scopes=[{'project_id':project['id'],'permission':'claim'}],tasks=[],task_keys=[]),
    token('kids-token-dddddddd','grandchild',parent_token_id='dead-token-cccccccc',expires_at=FUTURE,scopes=[{'project_id':project['id'],'permission':'claim'}],tasks=[],task_keys=[]),
]
resources=[]
def resource(id,name,capacity,holds=[]):
    return {'id':id,'org_id':'org-1','project_id':None,'name':name,'description':'shared test resource','capacity':capacity,'created_at':'2026-11-01T00:00:00.000Z','updated_at':'2026-11-01T00:00:00.000Z','holds':holds}
pending=[]
requests=[]
def route(r):
    path=r.request.url.split('/api/v1',1)[1]
    requests.append(path)
    if r.request.method=='PATCH' and path=='/me':
        body=r.request.post_data_json
        assert set(body)=={'display_name'}
        if state['reject_profile_patch']:
            state['reject_profile_patch']=False
            r.fulfill(status=422,content_type='application/json',body=json.dumps({'error':{'code':'invalid_request','message':'display name is invalid'}}))
            return
        state['display_name']=body['display_name'].strip()
        human['display_name']=state['display_name']
        r.fulfill(status=200,content_type='application/json',body=json.dumps({}))
        return
    if r.request.method=='PUT' and path=='/me/password':
        body=r.request.post_data_json
        if body['current_password']=='wrong password':
            r.fulfill(status=401,content_type='application/json',body=json.dumps({'error':{'code':'unauthenticated','message':'current password is incorrect'}}))
        else:
            r.fulfill(status=204)
        return
    if r.request.method=='PATCH' and path=='/organization':
        body=r.request.post_data_json
        assert set(body)=={'name'}
        state['organization_name']=body['name'].strip()
        r.fulfill(status=200,content_type='application/json',body=json.dumps({'id':'org-1','name':state['organization_name']}))
        return
    if r.request.method=='PATCH' and path.startswith('/principals/'):
        principal_id=path.rsplit('/',1)[1]
        body=r.request.post_data_json
        if 'display_name' in body:
            target=human if principal_id=='human-1' else agent
            target['display_name']=body['display_name'].strip()
            if principal_id=='human-1': state['display_name']=target['display_name']
        r.fulfill(status=200,content_type='application/json',body=json.dumps(human if principal_id=='human-1' else agent))
        return
    if r.request.method=='PATCH' and path=='/projects/project-1':
        body=r.request.post_data_json
        assert set(body)=={'description'}
        if state['reject_project_patch']:
            state['reject_project_patch']=False
            r.fulfill(status=422,content_type='application/json',body=json.dumps({'error':{'code':'invalid_request','message':'description is too long'}}))
            return
        project['description']=body['description'].strip()
        r.fulfill(status=200,content_type='application/json',body=json.dumps(project))
        return
    if r.request.method=='DELETE' and path.startswith('/tokens/'):
        match=next(t for t in tokens if t['id']==path.rsplit('/',1)[1])
        match['revoked_at']='2026-11-01T00:00:00.000Z'
        r.fulfill(status=204)
        return
    if path=='/resources' and r.request.method=='POST':
        body=r.request.post_data_json
        created=resource('resource-1',body['name'],body['capacity'])
        created['description']=body.get('description','')
        resources[:] = [created]
        r.fulfill(status=201,content_type='application/json',body=json.dumps(created))
        return
    if path=='/resources': body={'resources':resources}
    elif path.startswith('/resources/resource-1/acquire'):
        body=r.request.post_data_json
        resources[0]['holds']=[{'id':'hold-1','reason':body['reason'],'acquired_at':'2026-11-01T00:00:00.000Z','holder':{'id':'human-1','kind':'human','display_name':state['display_name'],'status':'active'}}]
        r.fulfill(status=200,content_type='application/json',body=json.dumps({'hold_id':'hold-1','resource':resources[0]}))
        return
    elif path.startswith('/resources/resource-1/release'):
        resources[0]['holds']=[]
        r.fulfill(status=200,content_type='application/json',body=json.dumps(resources[0]))
        return
    elif path.startswith('/resources/resource-1/revoke'):
        resources[0]['holds']=[]
        r.fulfill(status=200,content_type='application/json',body=json.dumps(resources[0]))
        return
    elif path=='/resources/resource-1' and r.request.method=='PATCH':
        resources[0].update(r.request.post_data_json)
        r.fulfill(status=200,content_type='application/json',body=json.dumps(resources[0]))
        return
    elif path=='/resources/resource-1' and r.request.method=='DELETE':
        resources.clear()
        r.fulfill(status=204)
        return
    elif path=='/me':
        if state['refresh_failures']:
            state['refresh_failures']-=1
            r.fulfill(status=500,content_type='application/json',body=json.dumps({'error':{'code':'internal_error','message':'internal server error'}}))
            return
        body={'id':'human-1','kind':'human','display_name':state['display_name'],'email':'auditor@example.test','organization':{'id':'org-1','name':state['organization_name']},'is_org_admin':state['is_org_admin'],'grants':[{'project_id':None,'permission':'admin'}] if state['is_org_admin'] else []}
    elif path=='/projects':
        body={'projects':[{**project,'stages':[stage,new_stage] if state['moved'] else [stage]}]}
    elif path=='/me/inbox': body={'requests':[]}
    elif path=='/principals/work': body=principal_work()
    elif path=='/principals': body={'principals':[human,agent]}
    elif path=='/signup-links': body={'signup_links':[]}
    elif path=='/principals/agent-1/grants': body={'grants':[]}
    elif path=='/principals/agent-1/tokens': body={'tokens':tokens}
    elif path=='/tasks/DEV-1/context':
        history=[{'seq':1,'type':'work.claimed','category':'work','actor':agent,'delegated_token_id':'live-token-bbbbbbbb','data':{},'created_at':'2026-11-01T00:00:00.000Z'},
                 {'seq':2,'type':'work.released','category':'work','actor':agent,'delegated_token_id':None,'data':{},'created_at':'2026-11-01T00:00:00.000Z'}]
        body={'task':task(1),'parent':None,'children':[],'dependencies':[],'dependents':[],'links':[],'comments':[],'artifacts':[],'history':history}
    elif path.startswith('/tasks?'):
        if state['mode']=='many':
            query=parse_qs(urlsplit(path).query)
            cursor=int(query.get('cursor',['0'])[0])
            body={'tasks':[task(i) for i in range(cursor+1,min(cursor+500,501)+1)],'total':501,'next_cursor':500 if cursor==0 else None}
        elif state['mode']=='race':
            query=parse_qs(urlsplit(path).query)
            if query.get('q')==['new']:
                body={'tasks':[task(999)],'total':1,'next_cursor':None}
            elif query.get('cursor')==['500']:
                pending.append(r)
                return
            else:
                body={'tasks':[task(i) for i in range(1,501)],'total':501,'next_cursor':500}
        elif state['mode']=='error':
            query=parse_qs(urlsplit(path).query)
            if query.get('cursor')==['500']:
                r.fulfill(status=500,content_type='application/json',body=json.dumps({'error':{'code':'internal_error','message':'page unavailable'}}))
                return
            body={'tasks':[task(i) for i in range(1,501)],'total':501,'next_cursor':500}
        else: body={'tasks':[task(1,new_stage if state['moved'] else stage)],'total':1,'next_cursor':None}
    else: raise AssertionError(path)
    r.fulfill(status=200,content_type='application/json',body=json.dumps(body))
results={}
try:
    with sync_playwright() as p:
        browser=p.chromium.launch(headless=True)
        context=browser.new_context()
        context.route('**/api/v1/**',route)
        context.add_init_script('''window.auditStreams=[]; window.EventSource=class {
          constructor(url){this.url=url;window.auditStreams.push(this)}
          close(){this.closed=true}
        };''')
        page=context.new_page()
        errors=[]
        page.on('pageerror',lambda e:errors.append(str(e)))
        page.goto(base+'/projects/DEV/board')
        page.wait_for_selector('[data-task="DEV-501"]')
        results['pagination_board']={'total':501,'visible_cards':page.locator('.card').count(),'next_page_requested':any('cursor=500' in r for r in requests),'last_task_visible':page.locator('[data-task="DEV-501"]').count()}
        assert results['pagination_board']['visible_cards']==501
        assert results['pagination_board']['next_page_requested']
        page.get_by_role('link',name='リスト',exact=True).click()
        page.get_by_role('cell',name='Task 501',exact=True).wait_for()
        results['pagination_list']={'total':501,'visible_rows':page.locator('tbody tr').count(),'next_page_requested':any('cursor=500' in r for r in requests)}
        assert results['pagination_list']['visible_rows']==501
        with page.expect_response(lambda r:'depends_on=DEV-1' in r.url):
            page.get_by_role('textbox',name='依存先',exact=True).fill('DEV-1')
        state['mode']='stage' 
        page.goto(base+'/projects/DEV/board')
        page.wait_for_selector('[data-task="DEV-1"]')
        requests.clear()
        state['moved']=True
        with page.expect_response(lambda r:'/api/v1/tasks?' in r.url):
            page.evaluate('window.auditStreams.filter(s=>!s.closed).forEach(s=>s.onmessage?.({data:"{}"}))')
        page.wait_for_selector('[data-stage="Review"] [data-task="DEV-1"]')
        results['external_stage_change']={'api_task_stage':'Review','visible_cards':page.locator('.card').count(),'review_column_visible':page.locator('[data-stage="Review"]').count(),'projects_reloaded':'/projects' in requests,'requests':requests.copy()}
        assert results['external_stage_change']['visible_cards']==1
        assert results['external_stage_change']['review_column_visible']==1
        assert results['external_stage_change']['projects_reloaded']
        page.reload()
        page.wait_for_selector('[data-stage="Review"] [data-task="DEV-1"]')
        results['after_browser_reload']={'visible_cards':page.locator('.card').count(),'review_column_visible':page.locator('[data-stage="Review"]').count()}
        state['mode']='race'
        with page.expect_request(lambda r:'cursor=500' in r.url):
            page.goto(base+'/projects/DEV/board')
        with page.expect_response(lambda r:'q=new' in r.url):
            page.get_by_placeholder('タスクを絞り込む').fill('new')
        page.wait_for_selector('[data-task="DEV-999"]')
        assert len(pending)==1
        with page.expect_response(lambda r:'cursor=500' in r.url):
            pending.pop().fulfill(status=200,content_type='application/json',body=json.dumps({'tasks':[task(501)],'total':501,'next_cursor':None}))
        # Let React apply any queued state updates from the delayed response.
        page.evaluate('() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))')
        assert page.locator('.card').count()==1
        assert page.locator('[data-task="DEV-999"]').count()==1
        results['latest_filter_wins']=True
        state['mode']='error'
        page.goto(base+'/projects/DEV/board')
        page.get_by_text('page unavailable',exact=True).wait_for()
        assert page.locator('.card').count()==0
        results['partial_list_not_presented_as_complete']=True
        state['mode']='stage'
        page.goto(base+'/tasks/DEV-1')
        page.get_by_text('委譲 bbbbbbbb',exact=True).wait_for()
        history=page.locator('.history li')
        assert history.count()==2
        assert history.nth(1).locator('.badge').count()==0
        assert page.locator('.history .badge').get_attribute('title')=='委譲トークン live-token-bbbbbbbb'
        results['delegated_history']=True
        page.goto(base+'/admin')
        page.get_by_text('作業は主体から報告された状況であり、外部プロセスの稼働を保証するものではありません。',exact=True).wait_for()
        agent_row=page.locator('tr').filter(has_text='Orchestrator')
        assert agent_row.get_by_role('link',name='DEV-1 Task 1',exact=True).count()==1
        assert agent_row.get_by_role('link',name='DEV-2 Task 2',exact=True).count()==1
        assert '取得済み' in agent_row.inner_text()
        assert '作業中' in agent_row.inner_text()
        assert page.locator('tr').filter(has_text='Auditor').get_by_text('作業なし',exact=True).count()==1
        requests.clear()
        state['principal_work_changed']=True
        with page.expect_response(lambda r:r.url.endswith('/api/v1/principals/work')):
            page.evaluate('window.auditStreams.filter(s=>!s.closed).forEach(s=>s.onmessage?.({data:"{}"}))')
        agent_row.get_by_text('一時停止',exact=True).wait_for()
        assert agent_row.get_by_role('link',name='DEV-2 Task 2',exact=True).count()==0
        results['principal_work']={'multiple_claims':2,'activities':['claimed','working','paused'],'live_reload':'/principals/work' in requests,'stale_work_removed':True}
        assert results['principal_work']['live_reload']
        page.get_by_role('button',name='権限',exact=True).last.click()
        rows=page.get_by_role('list',name='発行済みのトークン').get_by_role('listitem')
        rows.first.wait_for()
        def row(name): return rows.filter(has=page.get_by_text(name,exact=True))
        assert row('main').locator('.token-state').inner_text()=='有効'
        assert row('main').locator('.badge').count()==0
        live=row('implementer')
        assert live.locator('.token-state').inner_text()=='有効'
        assert '委譲 bbbbbbbb' in live.inner_text()
        assert '委譲元: main' in live.inner_text()
        assert 'Audit Project: claim, review（タスク DEV-1, ほか1件 と子孫）' in live.inner_text(), live.inner_text()
        assert row('revoked').locator('.token-state').inner_text()=='失効済み'
        assert row('revoked').get_by_role('button',name='失効').count()==0
        assert 'プロジェクト全体' in row('revoked').inner_text()
        assert row('grandchild').locator('.token-state').inner_text()=='親トークンが無効'
        assert row('grandchild').get_by_role('button',name='失効').count()==0
        page.screenshot(path=str(STATE/'delegated-tokens.png'))
        page.once('dialog',lambda dialog:dialog.accept())
        with page.expect_response(lambda r:r.request.method=='DELETE' and '/tokens/root-token' in r.url):
            row('main').get_by_role('button',name='失効').click()
        live.get_by_text('親トークンが無効',exact=True).wait_for()
        results['delegated_tokens']=True
        page.goto(base+'/settings')
        page.get_by_label('表示名',exact=True).fill('Renamed Auditor')
        with page.expect_response(lambda r:r.request.method=='PATCH' and r.url.endswith('/api/v1/me')):
            page.get_by_role('button',name='表示名を保存',exact=True).click()
        page.get_by_text('保存しました',exact=True).wait_for()
        page.locator('.sidebar').get_by_role('link',name='Renamed Auditor',exact=True).wait_for()
        assert page.locator('.sidebar').get_by_role('link',name='Renamed Auditor',exact=True).count()==1
        page.get_by_label('現在のパスワード').fill('retained current password')
        page.get_by_label('新しいパスワード',exact=True).fill('retained new password')
        page.get_by_label('新しいパスワードを確認').fill('retained new password')
        state['reject_profile_patch']=True
        page.get_by_label('表示名',exact=True).fill('Rejected Name')
        with page.expect_response(lambda r:r.request.method=='PATCH' and r.url.endswith('/api/v1/me')):
            page.get_by_role('button',name='表示名を保存',exact=True).click()
        page.get_by_text('display name is invalid',exact=True).wait_for()
        assert page.get_by_label('表示名',exact=True).input_value()=='Rejected Name'
        assert page.get_by_label('現在のパスワード').input_value()=='retained current password'
        state['refresh_failures']=1
        page.get_by_label('表示名',exact=True).fill('Refresh Retry')
        with page.expect_response(lambda r:r.request.method=='PATCH' and r.url.endswith('/api/v1/me')):
            page.get_by_role('button',name='表示名を保存',exact=True).click()
        page.get_by_text('internal server error',exact=True).wait_for()
        assert page.get_by_role('heading',name='アカウント設定',exact=True).count()==1
        assert page.locator('.center-card').count()==0
        assert page.get_by_label('表示名',exact=True).input_value()=='Refresh Retry'
        assert page.get_by_label('現在のパスワード').input_value()=='retained current password'
        with page.expect_response(lambda r:r.request.method=='PATCH' and r.url.endswith('/api/v1/me')):
            page.get_by_role('button',name='表示名を保存',exact=True).click()
        page.locator('.sidebar').get_by_role('link',name='Refresh Retry',exact=True).wait_for()
        results['profile_refresh_recovery']={'patch_failure_preserves_inputs':True,'refresh_failure_preserves_inputs':True,'retry_refreshes_identity':True}
        page.get_by_label('表示名',exact=True).fill('Renamed Auditor')
        with page.expect_response(lambda r:r.request.method=='PATCH' and r.url.endswith('/api/v1/me')):
            page.get_by_role('button',name='表示名を保存',exact=True).click()
        page.locator('.sidebar').get_by_role('link',name='Renamed Auditor',exact=True).wait_for()
        page.get_by_label('現在のパスワード').fill('wrong password')
        page.get_by_label('新しいパスワード',exact=True).fill('a valid password')
        page.get_by_label('新しいパスワードを確認').fill('different password')
        page.get_by_role('button',name='パスワードを変更',exact=True).click()
        page.get_by_text('新しいパスワードが一致しません',exact=True).wait_for()
        page.get_by_label('新しいパスワードを確認').fill('a valid password')
        with page.expect_response(lambda r:r.request.method=='PUT' and r.url.endswith('/api/v1/me/password')):
            page.get_by_role('button',name='パスワードを変更',exact=True).click()
        page.get_by_text('現在のパスワードが違います',exact=True).wait_for()
        assert page.get_by_label('現在のパスワード').input_value()=='wrong password'
        page.get_by_label('現在のパスワード').fill('correct password')
        with page.expect_response(lambda r:r.request.method=='PUT' and r.url.endswith('/api/v1/me/password')):
            page.get_by_role('button',name='パスワードを変更',exact=True).click()
        page.get_by_text('変更しました',exact=True).wait_for()
        assert page.get_by_label('現在のパスワード').input_value()==''
        assert page.get_by_label('新しいパスワード',exact=True).input_value()==''
        results['account_settings']={'profile_refresh':state['display_name'],'password_inputs_cleared':True}
        page.goto(base+'/admin')
        page.get_by_label('組織名').fill('Renamed Organization')
        with page.expect_response(lambda r:r.request.method=='PATCH' and r.url.endswith('/api/v1/organization')):
            page.get_by_role('button',name='組織名を保存',exact=True).click()
        page.get_by_text('保存しました',exact=True).first.wait_for()
        assert page.locator('.org').inner_text()=='Renamed Organization'
        agent_rename=page.get_by_role('button',name='表示名を変更',exact=True).last
        agent_rename.click()
        page.get_by_label('表示名',exact=True).fill('Canceled Agent')
        page.get_by_role('button',name='キャンセル',exact=True).click()
        assert page.get_by_text('Canceled Agent',exact=True).count()==0
        agent_rename.click()
        page.get_by_label('表示名',exact=True).fill('Renamed Agent')
        with page.expect_response(lambda r:r.request.method=='PATCH' and r.url.endswith('/api/v1/principals/agent-1')):
            page.get_by_role('button',name='表示名を保存',exact=True).click()
        page.get_by_text('Renamed Agent').wait_for()
        page.get_by_role('button',name='表示名を変更',exact=True).first.click()
        page.get_by_label('表示名',exact=True).fill('Lead Auditor')
        with page.expect_response(lambda r:r.request.method=='PATCH' and r.url.endswith('/api/v1/principals/human-1')):
            page.get_by_role('button',name='表示名を保存',exact=True).click()
        page.locator('.sidebar').get_by_role('link',name='Lead Auditor',exact=True).wait_for()
        assert page.locator('.sidebar').get_by_role('link',name='Lead Auditor',exact=True).count()==1
        results['admin_renames']={'organization_refresh':state['organization_name'],'agent_refresh':agent['display_name'],'self_refresh':state['display_name']}
        page.goto(base+'/resources')
        page.get_by_role('heading',name='リソース',exact=True).wait_for()
        page.get_by_role('button',name='リソースを追加',exact=True).click()
        dialog=page.get_by_role('dialog',name='リソースを追加')
        dialog.get_by_label('名前').fill('test environment')
        dialog.get_by_label('説明').fill('browser check')
        dialog.get_by_label('上限').fill('1')
        with page.expect_response(lambda r:r.request.method=='POST' and r.url.endswith('/api/v1/resources')):
            dialog.get_by_role('button',name='リソースを追加',exact=True).click()
        page.get_by_role('heading',name='test environment',exact=True).wait_for()
        reason=page.get_by_role('textbox',name='test environmentを確保する理由')
        reason.fill('browser run')
        with page.expect_response(lambda r:r.request.method=='POST' and r.url.endswith('/acquire')):
            page.get_by_role('button',name='確保',exact=True).click()
        page.get_by_text('browser run',exact=True).wait_for()
        with page.expect_response(lambda r:r.request.method=='POST' and r.url.endswith('/release')):
            page.get_by_role('button',name='解放',exact=True).click()
        page.locator('.resource-hold').wait_for(state='detached')
        resources[0]['holds']=[{'id':'hold-external','reason':'external run','acquired_at':'2026-11-01T00:00:00.000Z','holder':agent}]
        with page.expect_response(lambda r:r.url.endswith('/api/v1/resources')):
            page.evaluate('window.auditStreams.filter(s=>!s.closed).forEach(s=>s.onmessage?.({data:"{}"}))')
        page.get_by_text('external run',exact=True).wait_for()
        page.get_by_role('button',name='解除',exact=True).click()
        page.get_by_label('解除理由').fill('browser cleanup')
        with page.expect_response(lambda r:r.request.method=='POST' and r.url.endswith('/revoke')):
            page.get_by_role('button',name='解除する',exact=True).click()
        page.locator('.resource-hold').wait_for(state='detached')
        page.get_by_role('button',name='編集',exact=True).click()
        dialog=page.get_by_role('dialog',name='リソースを編集')
        dialog.get_by_label('名前').fill('renamed environment')
        with page.expect_response(lambda r:r.request.method=='PATCH' and r.url.endswith('/resources/resource-1')):
            dialog.get_by_role('button',name='変更を保存',exact=True).click()
        page.get_by_role('heading',name='renamed environment',exact=True).wait_for()
        page.once('dialog',lambda dialog:dialog.accept())
        with page.expect_response(lambda r:r.request.method=='DELETE' and r.url.endswith('/resources/resource-1')):
            page.get_by_role('button',name='削除',exact=True).click()
        page.locator('.resource-card').wait_for(state='detached')
        page.screenshot(path=str(STATE/'resources.png'))
        results['resources']={'created':True,'acquired_and_released':True,'external_hold_reloaded':True,'revoked':True,'edited':True,'deleted':page.locator('.resource-card').count()==0}
        assert results['resources']['deleted']
        state['mode']='stage'
        page.goto(base+'/projects/DEV/board')
        page.wait_for_selector('[data-task="DEV-1"]')
        page.get_by_role('link',name='プロジェクトの概要を設定',exact=True).click()
        overview_box=page.get_by_role('textbox',name='プロジェクトの概要',exact=True)
        overview_box.wait_for()
        state['reject_project_patch']=True
        overview_box.fill('rejected text')
        page.get_by_role('button',name='概要を保存',exact=True).click()
        page.get_by_text('description is too long',exact=True).wait_for()
        assert project['description']=='' and overview_box.input_value()=='rejected text'
        assert page.get_by_text('保存しました').count()==0
        overview_box.fill('  目的: 監査\nrepo: org/audit\n3行目の詳細  ')
        page.get_by_role('button',name='概要を保存',exact=True).click()
        page.get_by_text('保存しました').wait_for()
        assert project['description']=='目的: 監査\nrepo: org/audit\n3行目の詳細'
        assert overview_box.input_value()==project['description']
        page.reload()
        overview_box.wait_for()
        assert overview_box.input_value()==project['description']
        for url in ('/projects/DEV/board','/projects/DEV/list','/tasks/DEV-1'):
            page.goto(base+url)
            shown=page.get_by_role('region',name='プロジェクトの概要')
            shown.wait_for()
            assert shown.locator('p').evaluate('p=>getComputedStyle(p).whiteSpace')=='pre-wrap'
            assert shown.locator('p').inner_text().split('\n')==project['description'].split('\n')
            toggle=shown.get_by_role('button',name='すべて表示',exact=True)
            collapsed=shown.locator('p').evaluate('p=>p.getBoundingClientRect().height')
            toggle.click()
            expanded=shown.locator('p').evaluate('p=>p.getBoundingClientRect().height')
            assert expanded>collapsed, (url,collapsed,expanded)
            shown.get_by_role('button',name='折りたたむ',exact=True).click()
            assert shown.locator('p').evaluate('p=>p.getBoundingClientRect().height')==collapsed
        results['project_overview_saved_and_shown']=True
        state['is_org_admin']=False
        page.goto(base+'/projects/DEV/settings')
        page.get_by_text('repo: org/audit').wait_for()
        assert page.get_by_role('textbox',name='プロジェクトの概要',exact=True).count()==0
        assert page.get_by_role('button',name='概要を保存',exact=True).count()==0
        project['description']=''
        page.goto(base+'/projects/DEV/board')
        page.wait_for_selector('[data-task="DEV-1"]')
        assert page.get_by_role('region',name='プロジェクトの概要').count()==0
        assert page.get_by_role('link',name='プロジェクトの概要を設定',exact=True).count()==0
        page.goto(base+'/tasks/DEV-1')
        page.wait_for_selector('.breadcrumbs')
        assert page.get_by_role('region',name='プロジェクトの概要').count()==0
        results['project_overview_read_only_and_empty']=True
        page.goto(base+'/projects/DEV/board')
        page.locator('.sidebar').get_by_role('link',name='Lead Auditor',exact=True).wait_for()
        assert page.get_by_role('link',name='組織の管理',exact=True).count()==0
        page.goto(base+'/admin')
        assert page.get_by_label('組織名').count()==0
        results['general_user_settings']=True
        assert errors==[], errors
        results['browser_errors']=errors
        browser.close()
finally:
    server.shutdown()
(STATE/'results.json').write_text(json.dumps(results,ensure_ascii=False,indent=2))
print(json.dumps(results,ensure_ascii=False,indent=2))
