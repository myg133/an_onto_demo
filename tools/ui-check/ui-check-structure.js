// REQ-161 结构性验证（真浏览器）：左导航 / 抽屉 / sticky 表头 / key 等宽
const http = require('http');
const fs = require('fs');
const path = require('path');
const { chromium } = require('playwright');

const HTML = path.join(__dirname, 'console.html');
const EXE = process.env.EXE;
const out = [];
const check = (n, ok, d) => { out.push(ok); console.log(`${ok ? 'PASS' : 'FAIL'}  ${n}${ok ? '' : '  ← ' + (d || '')}`); };

const catalog = {
  project: 'demo', version: 2, total_items: 2,
  counts: { classes: 2, properties: 53, relations: 5, excluded_relations: 0, skipped_classes: 0 },
  items: [
    { name: 'cvm_instance', kind: 'class', table: 't001', primary_key: 'id', property_count: 41, relation_count: 3, source_kind: 'api' },
    { name: 'cbs_disk', kind: 'class', table: 't002', primary_key: 'id', property_count: 12, relation_count: 2, source_kind: 'db' },
  ],
};

(async () => {
  const server = http.createServer((req, res) => { res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' }); res.end(fs.readFileSync(HTML)); });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const base = `http://127.0.0.1:${server.address().port}`;

  const browser = await chromium.launch({ executablePath: EXE });
  const page = await browser.newPage();
  const errs = [];
  page.on('pageerror', (e) => errs.push(String(e)));

  await page.route('**/api/**', async (route) => {
    const u = new URL(route.request().url());
    const j = (o, s = 200) => route.fulfill({ status: s, contentType: 'application/json', body: JSON.stringify(o) });
    if (u.pathname === '/api/projects') return j({ project: 'demo', versions: [] });
    if (u.pathname === '/api/catalog') return j(catalog);
    if (u.pathname === '/api/describe') return j({ class: 'cvm_instance', label: '云服务器', properties: [], relations: [] });
    return j({ error: { code: 'NOT_FOUND', message: 'stub', hint: '' } }, 404);
  });

  await page.goto(base + '/', { waitUntil: 'load' });
  await page.fill('#token', 'x'.repeat(32));
  await page.click('#go');
  await page.waitForTimeout(300);

  // ① 左导航
  check('#nav 存在', await page.isVisible('#nav'));
  check('#tabs 已移除', (await page.locator('#tabs').count()) === 0);
  const navCount = await page.locator('#nav .nav-item').count();
  check('左导航恰好 8 个入口', navCount === 8, `实际 ${navCount}`);

  // ② 导航切换
  await page.click('#tab-catalog');
  await page.waitForTimeout(600);
  check('点「目录浏览」切到 v-catalog', await page.isVisible('#v-catalog'));
  check('其它视图已隐藏', !(await page.isVisible('#v-projects')));

  // ③ 目录行 → 抽屉
  const rows = await page.locator('#k-table tbody tr').count();
  check('目录表有数据行（stub 2 行）', rows >= 2, `实际 ${rows}`);
  await page.locator('#k-table .key').first().click();
  await page.waitForTimeout(500);
  const drawerOpen = await page.evaluate(() => {
    const d = document.getElementById('drawer');
    return d ? (d.classList.contains('is-on') || d.getAttribute('aria-hidden') === 'false') : null;
  });
  check('点行后抽屉打开', drawerOpen === true, String(drawerOpen));
  await page.keyboard.press('Escape');
  await page.waitForTimeout(400);
  const drawerClosed = await page.evaluate(() => {
    const d = document.getElementById('drawer');
    return d ? (!d.classList.contains('is-on') || d.getAttribute('aria-hidden') === 'true') : null;
  });
  check('Esc 关闭抽屉', drawerClosed === true, String(drawerClosed));

  // ④ sticky 表头 + 32px 行高
  const css = await page.evaluate(() => {
    const th = document.querySelector('#k-table thead th');
    const td = document.querySelector('#k-table tbody td');
    if (!th || !td) return null;
    const a = getComputedStyle(th), b = getComputedStyle(td);
    return { pos: a.position, top: a.top, h: b.height };
  });
  check('表头 position=sticky', css && css.pos === 'sticky', JSON.stringify(css));
  check('表格行高 32px', css && Math.abs(parseFloat(css.h) - 32) < 1.5, JSON.stringify(css));

  // ⑤ key 等宽
  const mono = await page.evaluate(() => {
    const el = document.querySelector('#k-table tbody .key') || document.querySelector('.key');
    if (!el) return null;
    const f = getComputedStyle(el).fontFamily || '';
    return { f, ok: /mono|Consolas|ui-monospace|Menlo|Courier/i.test(f) };
  });
  check('.key 使用等宽字体', mono && mono.ok, JSON.stringify(mono));

  check('全程无 JS 异常', errs.length === 0, errs.join(' | ').slice(0, 200));

  await browser.close();
  server.close();
  const failed = out.filter((x) => !x).length;
  console.log(`\n结果：${out.length - failed}/${out.length} 通过`);
  process.exit(failed ? 1 : 0);
})().catch((e) => { console.error('RUNNER ERROR:', e.message); process.exit(2); });
