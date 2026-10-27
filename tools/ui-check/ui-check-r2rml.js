// REQ-161 追加修复验证：POST /api/r2rml 回 text/turtle ⇒ 页面应显示 Turtle 正文，不报 BAD_JSON
const http = require('http');
const fs = require('fs');
const path = require('path');
const { chromium } = require('playwright');

const HTML = path.join(__dirname, 'console.html');
const EXE = process.env.EXE;
const out = [];
const check = (n, ok, d) => { out.push(ok); console.log(`${ok ? 'PASS' : 'FAIL'}  ${n}${ok ? '' : '  ← ' + (d || '')}`); };
const TTL = '@prefix rr: <http://www.w3.org/ns/r2rml#> .\n@prefix ex: <http://example.com/> .\n\nex:triples a rr:TriplesMap .\n';

(async () => {
  const server = http.createServer((req, res) => { res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' }); res.end(fs.readFileSync(HTML)); });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const base = `http://127.0.0.1:${server.address().port}`;

  const browser = await chromium.launch({ executablePath: EXE });
  const page = await browser.newPage();
  const errs = [];
  page.on('pageerror', (e) => errs.push(String(e)));
  let r2rmlHits = 0;

  await page.route('**/api/**', async (route) => {
    const req = route.request();
    const u = new URL(req.url());
    const j = (o, s = 200) => route.fulfill({ status: s, contentType: 'application/json', body: JSON.stringify(o) });
    if (u.pathname === '/api/projects') return j({ project: 'demo', versions: [{ version: 2, frozen: false, has_snapshot: true, objects: 83, relations: 52 }] });
    if (u.pathname === '/api/r2rml' && req.method() === 'POST') {
      r2rmlHits++;
      return route.fulfill({ status: 200, contentType: 'text/turtle; charset=utf-8', body: TTL });
    }
    if (u.pathname === '/api/export/ttl') return route.fulfill({ status: 200, contentType: 'text/turtle', body: '@prefix ontok: <http://ontok.local/> .' });
    return j({ error: { code: 'NOT_FOUND', message: 'stub', hint: '' } }, 404);
  });

  await page.goto(base + '/', { waitUntil: 'load' });
  await page.fill('#token', 'x'.repeat(32));
  await page.click('#go');
  await page.waitForTimeout(400);
  await page.click('#tab-export');
  await page.waitForTimeout(300);

  await page.fill('#e2-schema', 'code/out/schema.json');
  await page.click('#e2-go');
  await page.waitForTimeout(900);

  const bodyText = (await page.textContent('#e2-out')) || '';
  check('确实调用了 POST /api/r2rml 一次', r2rmlHits === 1, `hits=${r2rmlHits}`);
  check('结果区出现「R2RML 已生成」', /R2RML 已生成/.test(bodyText), bodyText.slice(0, 160));
  check('结果区显示 Turtle 正文（@prefix rr:）', /@prefix rr:/.test(bodyText), bodyText.slice(0, 200));
  check('结果区没有 BAD_JSON', !/BAD_JSON/.test(bodyText), bodyText.slice(0, 200));
  check('结果区没有「服务端返回的不是 JSON」', !/不是 JSON/.test(bodyText), bodyText.slice(0, 200));
  check('全程无 JS 异常', errs.length === 0, errs.join(' | ').slice(0, 200));

  await browser.close();
  server.close();
  const failed = out.filter((x) => !x).length;
  console.log(`\n结果：${out.length - failed}/${out.length} 通过`);
  process.exit(failed ? 1 : 0);
})().catch((e) => { console.error('RUNNER ERROR:', e.message); process.exit(2); });
