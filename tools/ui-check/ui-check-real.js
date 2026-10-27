// REQ-160 真服务验证：打**真的** review open（不 stub 任何 API）
// 用法：node ui-check-real.js <URL> <TOKEN>
const { chromium } = require('playwright');
const http = require('http');

const BASE = process.argv[2];
const TOKEN = process.argv[3];
const EXE = process.env.EXE;
const results = [];
const check = (n, ok, d) => { results.push(ok); console.log(`${ok ? 'PASS' : 'FAIL'}  ${n}${ok ? '' : '  ← ' + (d || '')}`); };

function req(path, token) {
  return new Promise((resolve) => {
    const u = new URL(path, BASE);
    const r = http.request({ hostname: u.hostname, port: u.port, path: u.pathname + u.search,
      headers: token ? { Authorization: 'Bearer ' + token } : {} }, (res) => {
      let b = ''; res.on('data', (c) => (b += c)); res.on('end', () => resolve({ status: res.statusCode, body: b }));
    });
    r.on('error', (e) => resolve({ status: 0, body: String(e) }));
    r.end();
  });
}

(async () => {
  // ① 免口令 / 口令 两种访问（真服务端，不是 stub）
  const anon = await req('/api/projects');
  check('GET /api/projects 无口令 ⇒ 401', anon.status === 401, String(anon.status));
  const bad = await req('/api/projects', 'x'.repeat(32));
  check('GET /api/projects 陌生口令 ⇒ 401', bad.status === 401, String(bad.status));
  const good = await req('/api/projects', TOKEN);
  check('GET /api/projects 一次性口令 ⇒ 200', good.status === 200, String(good.status));
  check('回执里的项目名是 anspire-real', /anspire-real/.test(good.body), good.body.slice(0, 120));

  // ② 真页面
  const browser = await chromium.launch({ executablePath: EXE });
  const page = await browser.newPage();
  const errs = [];
  page.on('pageerror', (e) => errs.push(String(e)));
  await page.goto(BASE + '/', { waitUntil: 'load' });
  check('页面 URL 不含口令', !page.url().includes(TOKEN) && !/[?&](token|key)=/.test(page.url()), page.url());

  await page.fill('#token', TOKEN);
  await page.click('#go');
  await page.waitForTimeout(600);
  const who = (await page.textContent('#who').catch(() => '')) || '';
  check('登录后进入工作台（#who 有内容）', who.length > 0, who.slice(0, 80));

  // ③ 项目与版本 tab：真回执
  await page.click('#tab-projects').catch(() => {});
  await page.waitForTimeout(700);
  const summary = (await page.textContent('#p-summary').catch(() => '')) || '';
  const table = (await page.textContent('#p-table').catch(() => '')) || '';
  check('项目与版本页显示 anspire-real', /anspire-real/.test(summary + table), (summary + table).slice(0, 160));
  check('版本表里能看到第 2 版', /\b2\b/.test(table), table.slice(0, 200));

  // ④ 评审 tab：真待审清单（提案 p1 已导入并已裁决，但条目仍应可见）
  await page.click('#tab-review');
  await page.waitForTimeout(500);
  await page.fill('#r-proposal', 'p1');
  await page.click('#r-load');
  await page.waitForTimeout(1200);
  const rSummary = (await page.textContent('#r-summary').catch(() => '')) || '';
  const rList = (await page.textContent('#r-list').catch(() => '')) || '';
  check('评审页载入到真清单（有汇总或条目）', /条|进度|已签|待签/.test(rSummary) || rList.length > 50, (rSummary || rList).slice(0, 200));

  // ⑤ 构建包 tab：真 /api/projects 回执 ⇒ 当前项目
  await page.click('#tab-bundles');
  await page.waitForTimeout(800);
  const bProject = (await page.textContent('#b-project').catch(() => '')) || '';
  check('构建包 tab 显示真项目 anspire-real', /anspire-real/.test(bProject), bProject);

  check('全程无 JS 异常', errs.length === 0, errs.join(' | ').slice(0, 300));

  await browser.close();
  const failed = results.filter((r) => !r).length;
  console.log(`\n结果：${results.length - failed}/${results.length} 通过`);
  process.exit(failed ? 1 : 0);
})().catch((e) => { console.error('RUNNER ERROR:', e.message); process.exit(2); });
