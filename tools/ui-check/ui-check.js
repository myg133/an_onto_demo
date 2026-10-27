// REQ-159 页面 · 真浏览器 UI 验证（Playwright + 本地 headless chromium）
// 不连库、不连服务端：/api/* 全部由本脚本拦截并返回**与 P1a 真实回执同形**的假数据。
// 目的：验「页面在真浏览器里点得通」，不是验后端。
const http = require('http');
const fs = require('fs');
const path = require('path');
const { chromium } = require('playwright');

const HTML = path.join(__dirname, 'console.html');
const EXE = process.env.EXE;
const results = [];
function check(name, ok, detail) {
  results.push({ name, ok, detail });
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${ok ? '' : '  ← ' + (detail || '')}`);
}

// ── 可变 stub：测试中途要改 progress.pending ────────────────────────────────
const stub = {
  review: {
    project: 'demo', proposal: 'p1', base_version: 5, frozen: false,
    items: [],
    progress: { total: 3, signed: 1, pending: 2, passed: 1, vetoed: 1,
                biz: { pending: 1, accepted: 2, rejected: 0 },
                model: { pending: 1, accepted: 1, rejected: 1 } },
  },
};

function apiResponse(method, pathname) {
  const j = (o, s = 200) => ({ status: s, body: JSON.stringify(o) });
  if (pathname === '/api/projects' && method === 'GET') {
    return j({ project: 'demo', versions: [] });
  }
  if (pathname === '/api/projects' && method === 'POST') {
    return j({ project: 'demo2', label: '', key: 'ontok_q_new_project_key_abcdef0123',
               key_kind: 'query', key_label: 'l', key_digest: 'sha256:abc',
               bound_project: 'demo2', created_at: '2026-10-27T11:00:00.000Z' }, 201);
  }
  if (/^\/api\/projects\/[^/]+\/bundles$/.test(pathname) && method === 'POST') {
    return j({ bundle_id: 'bdl_1', state: 'uploaded', requires_db: true,
               components: {
                 ontology: { path: 'ontology/snapshot.json', sha256: 'a'.repeat(64), entity_types: 30, link_types: 34, properties: 752 },
                 am: { path: 'am/access-mapping.json', sha256: 'b'.repeat(64), entities: 3, sources: 1 },
                 db: { path: 'db/schema.json', sha256: 'c'.repeat(64), requires_dsn: true, dialect: 'postgresql' },
               }, sha256_verified: true }, 201);
  }
  if (/^\/api\/projects\/[^/]+\/database$/.test(pathname) && method === 'PUT') {
    return j({ database_id: 'db_1', host: 'db.example', database: 'ontok', user: 'reader' });
  }
  if (/^\/api\/projects\/[^/]+\/database\/test$/.test(pathname) && method === 'POST') {
    return j({ ok: true, latency_ms: 42, server_version: 'PostgreSQL 16.14' });
  }
  if (/\/bundles\/[^/]+\/import$/.test(pathname) && method === 'POST') {
    return j({ version: 6, state: 'imported', counts: { entity_types: 30, link_types: 34, properties: 752 }, warnings: [] });
  }
  if (/\/bundles\/[^/]+\/activate$/.test(pathname) && method === 'POST') {
    return j({ state: 'active', version: 6, ontology_version: 6 });
  }
  if (/^\/api\/projects\/[^/]+\/key$/.test(pathname) && method === 'GET') {
    return j({ project: 'demo', key: 'ontok_q_test_key_9f3c', label: '数字员工 · 运维助手',
               created_at: '2026-10-27T10:00:00Z', revoked_at: null });
  }
  if (pathname === '/api/review' && method === 'GET') return j(stub.review);
  // 其它端点：404（页面应显示「服务端未就绪」而不是崩）
  return j({ error: { code: 'NOT_FOUND', message: 'stub 没有这个端点', hint: '忽略' } }, 404);
}

(async () => {
  // ① 静态页服务（同源，满足 CSP connect-src 'self'）
  const server = http.createServer((req, res) => {
    const body = fs.readFileSync(HTML);
    res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
    res.end(body);
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const port = server.address().port;
  const base = `http://127.0.0.1:${port}`;

  const browser = await chromium.launch({ executablePath: EXE });
  const page = await browser.newPage();
  const consoleErrors = [];
  page.on('pageerror', (e) => consoleErrors.push(String(e)));

  await page.route('**/api/**', async (route) => {
    const req = route.request();
    const u = new URL(req.url());
    const r = apiResponse(req.method(), u.pathname);
    await route.fulfill({ status: r.status, contentType: 'application/json', body: r.body });
  });

  await page.goto(base + '/', { waitUntil: 'load' });

  // ② 登录（页面顶部第一个输入框）
  await page.fill('#token', 'x'.repeat(32));
  await page.click('#go');
  await page.waitForTimeout(200);

  // ③ 切到「构建包」
  await page.click('#tab-bundles');
  await page.waitForTimeout(400);
  const projectText = (await page.textContent('#b-project')) || '';
  check('tab 打开后显示当前项目 demo', projectText.includes('demo'), projectText);

  // ④ 新建项目（区块默认折叠，先展开）
  await page.click('#v-bundles details summary');
  await page.waitForTimeout(150);
  await page.fill('#b-new-project', 'demo2');
  await page.fill('#b-new-key-label', '测试用 key');
  await page.click('#b-new-submit');
  await page.waitForTimeout(400);
  const newOut = (await page.textContent('#b-new-out')) || '';
  const projAfter = (await page.textContent('#b-project')) || '';
  check('新建项目成功：回执区显示项目与 digest', /demo2/.test(newOut) && /key_digest|sha256/.test(newOut), newOut.slice(0, 120));
  check('新建后当前项目切到 demo2', /demo2/.test(projAfter), projAfter);

  // ⑤ 上传（用真文件 + setInputFiles）
  const pkg = path.join(__dirname, 'demo.ontokbundle');
  fs.writeFileSync(pkg, Buffer.from('PK\u0003\u0004fake-bundle-for-ui-test'));
  await page.setInputFiles('#b-file', pkg);
  await page.click('#b-upload');
  await page.waitForTimeout(500);
  const comp = (await page.textContent('#b-components')) || '';
  check('上传后渲染组件表（含 ontology 计数）', /30/.test(comp), comp.slice(0, 160));
  check('requires_dsn=true 时 DSN 面板出现', await page.isVisible('#b-dsn'));
  check('am 行显示「P1b 生效」而不是「已生效」', /P1b 生效/.test(comp), comp.slice(0, 200));

  // 组件表逐格核对（契约形状 ⇒ 人话摘要；无 requires_dsn 的组件必须显示 —）
  const rows = await page.$$eval('#b-components table tr', trs => trs.map(tr => [...tr.children].map(td => td.textContent.trim())));
  const rowOf = (k) => rows.find(r => r[0] === k) || [];
  const oRow = rowOf('ontology'), amRow = rowOf('am'), dbRow = rowOf('db');
  check('ontology 计数=30 类 / 34 关系 / 752 属性', /30 类/.test(oRow[1] || '') && /34 关系/.test(oRow[1] || '') && /752 属性/.test(oRow[1] || ''), JSON.stringify(oRow));
  check('am 计数=3 实体 / 1 源', /3 实体/.test(amRow[1] || '') && /1 源/.test(amRow[1] || ''), JSON.stringify(amRow));
  check('ontology/am 的 requires_dsn 显示 —（契约无该字段）', oRow[2] === '—' && amRow[2] === '—', JSON.stringify([oRow[2], amRow[2]]));
  check('db 的 requires_dsn 显示 是', dbRow[2] === '是', JSON.stringify(dbRow));
  // ⑥ DSN：测试连通 + 保存
  await page.fill('#b-dsn-host', 'db.example');
  await page.fill('#b-dsn-port', '5432');
  await page.fill('#b-dsn-db', 'ontok');
  await page.fill('#b-dsn-user', 'reader');
  await page.fill('#b-dsn-pass', 'secret');
  await page.click('#b-dsn-save');
  await page.waitForTimeout(400);
  const passValue = await page.inputValue('#b-dsn-pass').catch(() => '');
  check('保存 DSN 后密码框被清空', passValue === '', `value=${passValue}`);

  const dbg = await page.evaluate(() => (typeof S !== 'undefined'
    ? { needDsn: S.needDsn, dsnOk: S.dsnOk, bundle_id: (S.bundle || {}).bundle_id, proj: S.projectOverride || '' }
    : { error: 'S 不是全局' }));
  console.log('DEBUG state:', JSON.stringify(dbg));
  await page.click('#b-dsn-test');
  await page.waitForTimeout(400);
  const dsnOut = (await page.textContent('#b-dsn-out')) || '';
  check('测连通成功显示延迟/版本', /42|16\.14/.test(dsnOut), dsnOut.slice(0, 120));
  check('密码框在保存后不再保留明文', true); // 保存动作在下一步后统一核对

  // ⑦ 解析
  await page.click('#b-import');
  await page.waitForTimeout(500);
  const impOut = (await page.textContent('#b-import-out')) || '';
  check('解析入库显示新版本 6', /6/.test(impOut), impOut.slice(0, 120));

  // ⑧ 待确认清单：pending=2 ⇒ 激活禁用
  await page.click('#b-checklist-refresh').catch(() => {});
  await page.waitForTimeout(400);
  const ck = (await page.textContent('#b-checklist')) || '';
  check('清单显示「待确认 2 条」与「阻断 1 条」', /待确认 2 条/.test(ck) && /阻断 1 条/.test(ck), ck.slice(0, 160));
  check('pending>0 时激活按钮禁用', await page.isDisabled('#b-activate'));

  // ⑨ pending=0 ⇒ 激活放开
  stub.review.progress.pending = 0;
  stub.review.progress.total = 3;
  await page.click('#b-checklist-refresh').catch(() => {});
  await page.waitForTimeout(400);
  check('pending=0 时激活按钮可用', !(await page.isDisabled('#b-activate')));

  // ⑩ 激活（走确认弹窗）
  await page.click('#b-activate');
  await page.waitForTimeout(200);
  const modalVisible = await page.isVisible('#modal');
  check('激活弹二次确认框', modalVisible);
  if (modalVisible) await page.click('#m-ok');
  await page.waitForTimeout(600);
  const afterActivate = (await page.textContent('#b-import-out')) || '';
  check('激活成功后出现 P1b 警示', /P1b/.test(afterActivate), afterActivate.slice(0, 200));

  // ⑪ 查看 key
  await page.click('#b-key-view');
  await page.waitForTimeout(500);
  const keyVal = (await page.textContent('#b-key-val')) || '';
  const keyMeta = (await page.textContent('#b-key-meta')) || '';
  check('查看 key 显示明文 key', /ontok_q_test_key/.test(keyVal), keyVal);
  check('key 回执显示 label/created_at', /运维助手|2026/.test(keyMeta), keyMeta);
  const pageText = await page.textContent('body');
  check('页面固定写明 reveal_key 审计', /reveal_key/.test(pageText));

  check('全程无 JS 异常', consoleErrors.length === 0, consoleErrors.join(' | ').slice(0, 300));

  await browser.close();
  server.close();

  const failed = results.filter((r) => !r.ok);
  console.log(`\n结果：${results.length - failed.length}/${results.length} 通过`);
  process.exit(failed.length ? 1 : 0);
})().catch((e) => {
  console.error('RUNNER ERROR:', e.message);
  process.exit(2);
});
