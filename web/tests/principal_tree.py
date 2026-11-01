"""Shared principal relationships and browser checks for the admin tree."""

def extra_principals(agent, human):
    return [
        {**agent, 'id':'agent-grandchild', 'display_name':'Grandchild', 'created_by':'agent-2'},
        {**agent, 'id':'agent-sibling', 'display_name':'Sibling', 'created_by':'agent-1'},
        {**human, 'id':'human-agent', 'display_name':'Human from agent', 'created_by':'agent-1'},
        {**agent, 'id':'cycle-a', 'display_name':'Cycle A', 'created_by':'cycle-b'},
        {**agent, 'id':'cycle-b', 'display_name':'Cycle B', 'created_by':'cycle-a'},
        {**agent, 'id':'agent-self', 'display_name':'Self cycle', 'created_by':'agent-self'},
        {**agent, 'id':'agent-root', 'display_name':'No creator', 'created_by':None},
    ]


def assert_principal_tree(page):
    rows = page.locator('table.settings-table').nth(1).locator('tbody tr')
    expected = ['human-2','agent-1','agent-2','agent-grandchild','agent-sibling','agent-3','human-1','human-agent','agent-self','agent-root','cycle-a','cycle-b']
    assert rows.evaluate_all('rows => rows.map(row => row.dataset.principal)') == expected
    assert page.locator('.principal-creator').count() == 0
    assert '作成元' not in ''.join(rows.all_inner_texts())
    depths = {'agent-2':1, 'agent-grandchild':2, 'agent-sibling':1, 'cycle-b':1}
    for key in expected:
        row = page.locator(f'[data-principal="{key}"]')
        assert row.locator('.principal-tree').evaluate('e => getComputedStyle(e).paddingInlineStart') == f'{depths.get(key, 0)*24}px'
        assert row.locator('td').count() == 6
        assert row.get_by_role('button', name='権限', exact=True).count() == 1
    geometry = rows.evaluate_all('''rows => Object.fromEntries(rows.map(row => {
        const box = row.getBoundingClientRect();
        const tree = row.querySelector('.principal-tree').getBoundingClientRect();
        const lines = [...row.querySelectorAll('.principal-branch')].map(line => {
            const rect = line.getBoundingClientRect();
            const pseudo = getComputedStyle(line, '::before');
            return {x:rect.x, top:rect.top, bottom:rect.bottom, through:line.classList.contains('principal-branch-through'),
                down:line.classList.contains('principal-branch-down'), end:line.classList.contains('principal-branch-end'),
                join:rect.top + parseFloat(pseudo.height)};
        });
        return [row.dataset.principal, {top:box.top, bottom:box.bottom, height:box.height, center:tree.y+tree.height/2,
            cells:[...row.querySelectorAll('td')].map(cell => { const b=cell.getBoundingClientRect(); return b.y+b.height/2; }), lines}];
    }))''')
    for key, row in geometry.items():
        assert all(abs(center-row['center']) < .5 for center in row['cells']), key
        for line in row['lines']:
            if line['through'] or line['down']:
                assert line['bottom'] >= row['bottom']-.5, (key,line,row)
            if line['through'] or line['end']:
                assert line['top'] <= row['top']+.5, (key,line,row)
            if line['end']:
                assert abs(line['join']-row['center']) <= 1, (key,line,row)
    # Both parent and child have several work entries, so lines must span variable-height cells.
    assert geometry['agent-1']['height'] > 40
    assert geometry['agent-2']['height'] > 40
    for parent, child in [('agent-1','agent-2'), ('agent-2','agent-grandchild')]:
        down = next(line for line in geometry[parent]['lines'] if line['down'])
        branch = next(line for line in geometry[child]['lines'] if line['end'])
        assert abs(down['x']-branch['x']) <= .5
        assert abs(down['bottom']-branch['top']) <= 2
    # The elder child's and grandchild's trunk reaches the younger sibling without a gap.
    for previous, following in [('agent-2','agent-grandchild'), ('agent-grandchild','agent-sibling')]:
        trunk = next(line for line in geometry[previous]['lines'] if line['through'])
        branch = geometry[following]['lines'][0]
        assert abs(trunk['x']-branch['x']) <= .5
        assert abs(trunk['bottom']-branch['top']) <= 2
    return {'principals':len(expected), 'agent_only_relations':True, 'cycles_retained':True, 'continuous_tall_rows':True}
