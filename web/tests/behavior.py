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
server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
threading.Thread(target=server.serve_forever,daemon=True).start()
base=f'http://127.0.0.1:{server.server_port}'
stage={'id':'stage-1','name':'Implement','position':0,'requires_work':True,'claim_permission':'claim','is_final':False}
new_stage={**stage,'id':'stage-2','name':'Review','position':1}
project={'id':'project-1','key':'DEV','name':'Audit Project','description':'','stages':[stage],'transition_rules':[]}
work={'readiness':'ready','claimable':True,'can_claim':True,'claim_permission':'claim','has_claim_permission':True,'reasons':[],'occupant':None,'activity':None,'is_working':False,'acquired_at':None,'claim_id':None,'hold_reason':None,'blockers':[],'hidden_blockers':0}
def task(i,s=stage):
    return {'id':f'task-{i}','key':f'DEV-{i}','project_id':project['id'],'title':f'Task {i}','description':'','purpose':'','acceptance_criteria':'','stage':s,'responsible':None,'assignee':None,'priority':'none','due_at':None,'parent_id':None,'version':1,'created_at':'2026-11-01T00:00:00.000Z','updated_at':'2026-11-01T00:00:00.000Z','work':work}
state={'mode':'many','moved':False}
pending=[]
requests=[]
def route(r):
    path=r.request.url.split('/api/v1',1)[1]
    requests.append(path)
    if path=='/me':
        body={'id':'human-1','kind':'human','display_name':'Auditor','email':None,'organization':{'id':'org-1','name':'Audit'},'is_org_admin':True,'grants':[{'project_id':None,'permission':'admin'}]}
    elif path=='/projects':
        body={'projects':[{**project,'stages':[stage,new_stage] if state['moved'] else [stage]}]}
    elif path=='/me/inbox': body={'requests':[]}
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
        assert errors==[], errors
        results['browser_errors']=errors
        browser.close()
finally:
    server.shutdown()
(STATE/'results.json').write_text(json.dumps(results,ensure_ascii=False,indent=2))
print(json.dumps(results,ensure_ascii=False,indent=2))
