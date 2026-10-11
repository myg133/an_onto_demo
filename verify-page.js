// 客户视图页面的交互验证（BA 复核用）
//
// ## 为什么需要这个
//
// Dev 环境无 GUI，页面**没在真浏览器里打开过** —— 只验了标签配平 / DOCTYPE /
// 零外部依赖。搜索框的 JS 是**唯一没被执行过**的部分。
//
// 本脚本用 jsdom **实际加载并执行**页面里的脚本，覆盖：
//   1. 结构：对象/分组/高亮计数、details 折叠、必需元素
//   2. 交互：输入搜索词 → 卡片隐藏/显示、分组空了就整体隐藏
//   3. 安全：零外部依赖（双击能离线打开）、零建模术语
//
// ## 怎么用（判读前必跑）
//
//   mkdir -p .review-workspace/pagecheck
//   cd .review-workspace/pagecheck && npm install jsdom
//   node BA/verify-page.js <客户视图.html>
//
// ## 它替代了什么
//
// Dev 环境无 GUI，页面只验了「标签配平 / DOCTYPE / 零外部依赖」，
// **搜索框的 JS 从未被执行过** —— 那是页面里唯一可能坏而不被发现的代码。
// 本脚本用 jsdom **实际加载并执行**它。
//
// ## 改页面后必须重跑
//
// 判读材料是给 3 名外部评审人的，页面坏了就是白跑一小时。
// 这脚本是页面唯一的行为级门禁。

const fs = require('fs');
const path = require('path');
// jsdom 可能装在仓库内的工作区里（不跟踪），也可能装在全局 —— 两处都找
function loadJsdom() {
  for (const base of [__dirname,
                      path.join(__dirname, '..', '.review-workspace', 'pagecheck')]) {
    try { return require(require.resolve('jsdom', { paths: [base] })); }
    catch (e) { /* 试下一处 */ }
  }
  console.error('找不到 jsdom。请先安装：');
  console.error('  mkdir -p .review-workspace/pagecheck && cd .review-workspace/pagecheck && npm install jsdom');
  process.exit(2);
}
const { JSDOM } = loadJsdom();

const pageArg = process.argv[2];
if (!pageArg) { console.error('用法: node verify-page.js <页面.html>'); process.exit(2); }
const html = fs.readFileSync(pageArg, 'utf8');

let fail = 0;
const ok  = m => console.log(`  \x1b[32m✓\x1b[0m ${m}`);
const bad = m => { console.log(`  \x1b[31m✗\x1b[0m ${m}`); fail = 1; };
const chk = (cond, m) => cond ? ok(m) : bad(m);

// ── 1. 结构 ──
console.log('\n【1】结构');
const dom = new JSDOM(html, { runScripts: 'outside-only' });
const d = dom.window.document;
const cards = [...d.querySelectorAll('.对象')];
const groups = [...d.querySelectorAll('.分组')];
const box = d.querySelector('.查找框');

chk(cards.length > 0, `对象卡片 ${cards.length} 个`);
chk(groups.length > 0, `分组 ${groups.length} 个`);
chk(!!box, '搜索框存在');
const marks = d.body.textContent.match(/★/g) || [];
chk(marks.length >= 13, `★ 重点标记 ${marks.length} 个（清单 13 个，页头说明里还有 1 个）`);
const details = d.querySelectorAll('details').length;
chk(details >= groups.length, `<details> 折叠块 ${details} 个（≥ 分组数，展开/收起靠它）`);

// ── 2. 交互：真的执行页面里的脚本 ──
console.log('\n【2】交互（实际执行页面脚本）');
const scripts = [...d.querySelectorAll('script')].map(s => s.textContent);
chk(scripts.length > 0, `页面内联脚本 ${scripts.length} 段`);

for (const src of scripts) {
  try {
    const fn = new Function('document', src);
    fn(dom.window.document);
    global.__ran = true;
    ok('脚本执行无异常（已用真实 document 跑过）');
  } catch (e) {
    bad(`脚本执行抛错: ${e.message}`);
  }
}
chk(global.__ran, '脚本真的执行了（不是静默跳过）');
// 事件监听挂在脚本执行时绑的 box 上，脚本已跑过，下面 dispatch 才有效
if (box && !global.__ran) {
  // 兜底：若脚本抛错导致没绑上，交互测试结果不可信，明确标记
  bad('搜索框事件未绑定 —— 交互测试结果不可信');
}

if (box) {
  const visible = () => cards.filter(c => c.style.display !== 'none').length;
  const visibleGroups = () => groups.filter(g => g.style.display !== 'none').length;

  // 空搜索 → 全部可见
  box.value = '';
  box.dispatchEvent(new dom.window.Event('input'));
  chk(visible() === cards.length, `空搜索 → 全部可见（${visible()}/${cards.length}）`);

  // 搜一个确实存在的对象名
  const target = cards[0].querySelector('.名');
  const name = (target?.textContent || '').trim();
  if (name) {
    box.value = name;
    box.dispatchEvent(new dom.window.Event('input'));
    const v = visible(), gv = visibleGroups();
    chk(v > 0 && v < cards.length, `搜「${name}」→ 命中 ${v}/${cards.length}（确实是筛选不是全留）`);
    chk(gv > 0 && gv <= groups.length, `搜「${name}」→ 可见分组 ${gv}/${groups.length}`);
  } else {
    bad('找不到对象名元素，无法测搜索');
  }

  // 搜不存在的东西 → 全部隐藏、分组全隐藏
  box.value = 'zzz绝对不存在的词zzz';
  box.dispatchEvent(new dom.window.Event('input'));
  chk(visible() === 0, `搜不存在的词 → 卡片全隐藏（${visible()}）`);
  chk(visibleGroups() === 0, `搜不存在的词 → 分组也全隐藏（${visibleGroups()}）`);

  // 恢复
  box.value = '';
  box.dispatchEvent(new dom.window.Event('input'));
  chk(visible() === cards.length, '清空后恢复全部可见');
} else {
  bad('没有搜索框，跳过交互测试');
}

// ── 3. 离线可开（双击就行）──
console.log('\n【3】离线可开');
const raw = html;
for (const [pat, name] of [[/<link\b/i,'<link> 外部样式'], [/<script[^>]+src=/i,'<script src=> 外部脚本'],
                            [/@import/i,'CSS @import'], [/(src|href)\s*=\s*["']https?:/i,'外部 http(s) 资源'],
                            [/<img\b/i,'<img> 外链图片']]) {
  chk(!pat.test(raw), `零${name}`);
}
chk(/<!DOCTYPE html>/i.test(raw), '有 DOCTYPE');
chk(/charset=["']?utf-8/i.test(raw), '声明 UTF-8');

// ── 4. 建模术语（给人看的展示文本，剥掉 style/script/注释）──
console.log('\n【4】展示文本零建模术语');
const text = raw
  .replace(/<style[\s\S]*?<\/style>/gi, ' ')
  .replace(/<script[\s\S]*?<\/script>/gi, ' ')
  .replace(/<!--[\s\S]*?-->/g, ' ')
  .replace(/<[^>]+>/g, ' ')
  .replace(/\s+/g, ' ');
const CJK = ['外键','主键','基数','证据','实体','建模','字段','属性表'];
const EN  = ['key','entity','property','link','snapshot','ordinal','foreign'];
for (const w of CJK) { const n = text.split(w).length - 1; chk(n === 0, `中文「${w}」 0 命中`); }
for (const w of EN) {
  // 独立词元匹配：CJK 也算词字符，所以不能用 \b（Dev 踩过，漏判 50 条）
  const n = (text.match(new RegExp(`(?<![A-Za-z])${w}(?![A-Za-z])`, 'gi')) || []).length;
  chk(n === 0, `英文「${w}」 0 命中（独立词元）`);
}
for (const w of ['N:1','1:N','N:M','1:1']) chk(!text.includes(w), `基数记号「${w}」不出现`);

console.log(fail ? '\n\x1b[31m页面验证未通过\x1b[0m' : '\n\x1b[32m页面验证全过\x1b[0m');
process.exit(fail);
