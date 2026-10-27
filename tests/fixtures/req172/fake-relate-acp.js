// REQ-172 · **假的 ACP agent**（测试夹具）—— 同一个进程同时扮演：
//   ① 文档抽取（`doc ingest` 的每块）② 关系判定 A 轮 ③ 关系补漏 B 轮
//
// 为什么一个假 agent 同时管三段：`build run` 只收**一条** `--agent-command`，
// 它先跑 doc ingest、再跑 relation judge —— 若夹具只认关系轮，第 ① 段就会失败，
// 端到端永远走不到本需求新增的段。
//
// 环境变量：
//   FAKE_ACP_MODE  valid（默认）| garbage | nobasis
//     · garbage：每轮都回散文 ⇒ judge 必须**非 0 且不产产物**
//     · nobasis：A 轮全部 holds 但不给 basis、B 轮不给关系 ⇒ 判定层必须全丢 ⇒ 非 0
//   FAKE_ACP_LOG  每收到一条 prompt 追加一行（数调用次数、验缓存 0 调用）
//
// ⚠️ 它**不测协议**（协议由 `tests/test_acp_protocol.rs` 用真字节测）；
// 这里只测接线：两轮都走通、缓存命中 0 token、丢弃码各有数。
'use strict';

const fs = require('fs');

const MODE = process.env.FAKE_ACP_MODE || 'valid';
const LOG = process.env.FAKE_ACP_LOG || '';
const SESSION = 'fake-relate-req172';

let buf = '';
let prompts = 0;
let usage = 1000;

function send(obj) {
  process.stdout.write(JSON.stringify(obj) + '\n');
}
function note(update) {
  send({ jsonrpc: '2.0', method: 'session/update', params: { sessionId: SESSION, update } });
}
function chunk(text) {
  note({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text } });
}

/** 用户消息 = flatten 之后 `---` 分隔的最后一段（纯 JSON）。 */
function userOf(prompt) {
  const parts = prompt.split('\n\n---\n\n');
  return JSON.parse(parts[parts.length - 1]);
}

/** 文档块里的一个逐字片段（只取开头 12 字符，去空白）。 */
function firstQuote(text) {
  const s = Array.from(text || '').slice(0, 12).join('').trim();
  return s || '测试';
}

function answerA(u) {
  const cs = u.candidates || [];
  if (MODE === 'nobasis') {
    return JSON.stringify({
      verdicts: cs.map((c) => ({ candidate_id: c.id, verdict: 'holds', predicate: 'belongs_to' })),
    });
  }
  const verdicts = cs
    .map((c, i) => {
      const base = { candidate_id: c.id };
      if (i === 0) {
        return {
          ...base,
          verdict: 'holds',
          predicate: 'belongs_to',
          basis: {
            kind: 'domain',
            // 真 pi-acp 实测会写限定名（`表.列`）—— 校验层必须认这种形态
            columns: [c.child_table + '.' + c.child_column, c.parent_table + '.' + c.parent_column],
            detail: '样本不同值全部落在目标键里',
          },
          confidence: 0.9,
        };
      }
      if (i === 1) {
        return {
          ...base,
          verdict: 'rejects',
          basis: { kind: 'naming', columns: [c.child_column], detail: '值域与目标键不匹配' },
        };
      }
      // ③ 无依据：校验层必须丢（判据 3）
      if (i === 2) return { ...base, verdict: 'holds', predicate: 'belongs_to' };
      // ④ 谓词不在 16 主键里
      if (i === 3) {
        return {
          ...base,
          verdict: 'holds',
          predicate: 'uses',
          basis: { kind: 'naming', columns: [c.child_column], detail: 'x' },
        };
      }
      // ⑤ 依据点了空气列
      if (i === 4) {
        return {
          ...base,
          verdict: 'holds',
          predicate: 'belongs_to',
          basis: { kind: 'naming', columns: ['ghost_column'], detail: 'x' },
        };
      }
      return {
        ...base,
        verdict: 'holds',
        predicate: 'belongs_to',
        basis: {
          kind: 'naming',
          columns: [c.child_table + '.' + c.child_column, c.parent_table + '.' + c.parent_column],
          detail: '命名指向该表',
        },
        confidence: 0.6,
      };
    })
    .filter(Boolean);
  return JSON.stringify({ verdicts });
}

function answerB(u) {
  if (MODE === 'nobasis') return JSON.stringify({ relations: [] });
  const tables = u.tables || [];
  const focus = u.focus_table;
  const card = u.focus_card || {};
  const cols = (card.columns || []).map((c) => c.name);
  const child = cols.find((c) => c !== card.primary_key) || card.primary_key;
  const parent = tables.find((t) => t.name !== focus && t.primary_key);
  if (!parent || !child) return JSON.stringify({ relations: [] });
  const mk = (over) => ({
    table: focus,
    child_table: focus,
    child_column: child,
    parent_table: parent.name,
    parent_column: parent.primary_key,
    predicate: 'references',
    basis: { kind: 'comment', columns: [child], detail: '同名列/注释指向该表' },
    ...(over || {}),
  });
  // 5 条合法（校验层按「每表 ≤3」截断）+ 3 条必须各自被丢：
  // 谓词禁用词 / 表不存在 / 列不存在
  const rels = [
    mk(),
    mk(),
    mk(),
    mk(),
    mk(),
    mk({ predicate: 'related_to' }),
    mk({ parent_table: 'no_such_table_req172' }),
    mk({ child_column: 'no_such_column_req172' }),
  ];
  return JSON.stringify({ relations: rels });
}

function answerFor(prompt) {
  let u;
  try {
    u = userOf(prompt);
  } catch (e) {
    return '就绪'; // 预热 / 不是协议里那几种 JSON
  }
  if (u && u.chunk_id) {
    // ① 文档抽取：一条概念 + 一条逐字 quote（证据校验必须过得去）
    return JSON.stringify({
      concepts: [{ name: '测试概念', quote: firstQuote(u.text), modality: 'fact' }],
      relations: [],
    });
  }
  if (u && u.round === 'A') return answerA(u);
  if (u && u.round === 'B') return answerB(u);
  return '就绪';
}

process.stdin.on('data', (d) => {
  buf += d.toString();
  let i;
  while ((i = buf.indexOf('\n')) >= 0) {
    const line = buf.slice(0, i).trim();
    buf = buf.slice(i + 1);
    if (!line) continue;
    let m;
    try {
      m = JSON.parse(line);
    } catch (e) {
      continue;
    }
    if (m.method === 'initialize') {
      send({
        jsonrpc: '2.0',
        id: m.id,
        result: {
          protocolVersion: 1,
          agentInfo: { name: 'fake-relate-acp', title: 'fake ACP for REQ-172', version: '0.1.0' },
          agentCapabilities: {},
        },
      });
    } else if (m.method === 'session/new') {
      send({ jsonrpc: '2.0', id: m.id, result: { sessionId: SESSION, configOptions: null } });
    } else if (m.method === 'session/prompt') {
      prompts += 1;
      if (LOG) fs.appendFileSync(LOG, `${prompts}\n`);
      const prompt = (m.params.prompt || []).map((p) => p.text || '').join('');
      // 会话首轮先吐一段前言（真 pi-acp 就是这样；AcpChatTransport 会预热掉它）
      if (prompts === 1) chunk('## Skills\n- 假技能\n');
      let answer;
      if (MODE === 'garbage') {
        answer = '好的，我来判断：这两张表看起来有关系。';
      } else {
        answer = answerFor(prompt);
      }
      const mid = Math.floor(answer.length / 2);
      chunk(answer.slice(0, mid));
      chunk(answer.slice(mid));
      usage += 1000;
      note({ sessionUpdate: 'usage_update', used: usage, size: 200000 });
      send({ jsonrpc: '2.0', id: m.id, result: { stopReason: 'end_turn' } });
    } else {
      send({ jsonrpc: '2.0', id: m.id, error: { code: -32601, message: 'method not found' } });
    }
  }
});
