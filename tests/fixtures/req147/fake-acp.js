// REQ-147 · **假的 ACP agent**（测试夹具）—— 讲 JSON-RPC over stdio，不起真 pi。
//
// 为什么用 Node 写：本 crate 的 ACP 对端（`pi-acp`）本身就是 Node 实现，
// 而仓里没有别的「能讲 stdio JSON-RPC 的小脚本」运行时。Rust 里再加一个
// `[[bin]]` 会改动构建产物的集合（`tests/test_binary_output_guard.rs` 盯着那些），
// 那是拿一个测试夹具去动生产面。
//
// ⚠️ 不用它测**协议**（协议由 `tests/test_acp_protocol.rs` 用录好的真实字节测）。
// 这里只测**接线**：一次问答端到端走通、缓存命中 0 次调用、编造的 quote 被丢。
//
// 环境变量：
//   FAKE_ACP_MODE  valid（默认）| mixed | empty | hang | flaky | halfgarbage | garbagetwo
//                  | maps（REQ-170：关系带 maps_to，且两端表/列在 schema 里）
//                  | maps-bad（REQ-170：maps_to 指向不存在的列 ⇒ 程序必须丢掉并计数）
//                  | cmap（REQ-171：概念带 maps_to，一个模式覆盖落地 + 四类反例；关系含
//                    belongs_to / references 与一个真结构词 related_to）
//                  | cmap-good（REQ-171：概念 maps_to 全部能落地，给 doc merge 用）
//   FAKE_ACP_LOG   每收到一条 `session/prompt` 就往这个文件追加一行（数调用次数用）
'use strict';

const fs = require('fs');

const MODE = process.env.FAKE_ACP_MODE || 'valid';
const LOG = process.env.FAKE_ACP_LOG || '';
const SESSION = 'fake-session-req147';

let buf = '';
/** 会话累计用量（每轮 +1000）—— 与真实 ACP 一样报**累计**，不是增量。 */
let usage = 1000;
let prompts = 0;

function send(obj) {
  process.stdout.write(JSON.stringify(obj) + '\n');
}
function note(update) {
  send({ jsonrpc: '2.0', method: 'session/update', params: { sessionId: SESSION, update } });
}
function chunk(text) {
  note({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text } });
}

/** 从 prompt 里取出这一块的原文（user 消息是最后一段 `---` 之后那个 JSON）。 */
function chunkTextOf(prompt) {
  const parts = prompt.split('\n\n---\n\n');
  const last = parts[parts.length - 1];
  try {
    return JSON.parse(last).text || '';
  } catch (e) {
    return '';
  }
}

/** 这一段文档里的一个**逐字**片段（前 20 个字符，去首尾空白）。 */
function verbatim(text, n) {
  return Array.from(text).slice(0, n).join('').trim();
}

function answerFor(text) {
  const q1 = verbatim(text, 20);
  const q2 = Array.from(text).slice(Math.max(0, Array.from(text).length - 20)).join('').trim();
  // ⛔ 这一条**原文里没有** —— AC-005 的变异：程序必须丢它 + 计数。
  const forged = '模型编造的原文片段：客户向供应商采购';
  switch (MODE) {
    case 'empty':
      return JSON.stringify({ concepts: [], relations: [] });
    case 'mixed':
      return JSON.stringify({
        concepts: [{ name: '测试概念', quote: q1, modality: 'fact' }],
        relations: [
          {
            subject: '测试概念',
            predicate: '采购',
            object: '供应商',
            quote: forged,
            modality: 'fact',
          },
        ],
      });
    case 'garbage':
      return '好的，我来抽取：';
    // ⭐ REQ-170：**库结构进了 prompt** 之后模型该给的形状（关系带 maps_to）。
    // 这里只认列名（child_column / parent_column），表由「概念名 ↔ 表名/中文名」落 ——
    // 与简报 §2-R3 的形状一致。
    case 'maps':
      return JSON.stringify({
        concepts: [
          { name: '云硬盘', quote: q1, modality: 'fact' },
          { name: '云主机', quote: q2 || q1, modality: 'fact' },
        ],
        relations: [
          {
            subject: '云硬盘',
            predicate: '挂载到',
            object: '云主机',
            quote: q2 || q1,
            modality: 'fact',
            maps_to: { child_column: 'server_id', parent_column: 'id' },
          },
        ],
      });
    // 反例：父端列在 schema 里不存在 ⇒ 必须**整条丢弃并计数**（⛔ 不编列名）。
    case 'maps-bad':
      return JSON.stringify({
        concepts: [{ name: '云硬盘', quote: q1, modality: 'fact' }],
        relations: [
          {
            subject: '云硬盘',
            predicate: '挂载到',
            object: '云主机',
            quote: q2 || q1,
            modality: 'fact',
            maps_to: { child_column: 'server_id', parent_column: 'no_such_column' },
          },
        ],
      });
    // ⭐ REQ-171：**概念带 maps_to**（概念 → 表），一个模式同时给「落地」+ 四类反例。
    // 夹具 schema = tests/fixtures/req171/schema.json（llm_models「模型」/ model_pricing
    // 「模型价格」/ customers「客户」/ balances「账户余额」+ 2 张垃圾表）。
    case 'cmap':
      return JSON.stringify({
        concepts: [
          // ① 落地：表名 + 列都在
          {
            name: '模型',
            quote: q1,
            modality: 'fact',
            maps_to: { table: 'llm_models', column: 'model_name' },
          },
          // ② 查不到表
          { name: '客户', quote: q1, modality: 'fact', maps_to: { table: 'no_such_table' } },
          // ③ 对上多张：中文名「模型」同时命中「模型」与「模型价格」两张表
          { name: '账户余额', quote: q2 || q1, modality: 'fact', maps_to: { table: '模型' } },
          // ④ 形状不对（不是对象）
          { name: '平台', quote: q1, modality: 'fact', maps_to: 'llm_models.model_name' },
          // ⑤ 查不到列（表对了、列不在那张表里）
          {
            name: '单价',
            quote: q1,
            modality: 'fact',
            maps_to: { table: 'model_pricing', column: 'no_such_col' },
          },
        ],
        relations: [
          // R3：这两个词**不再**被谓词闸丢（契约 §5.2 的合法主键）
          { subject: '模型', predicate: 'belongs_to', object: '平台', quote: q1, modality: 'fact' },
          { subject: '客户', predicate: 'references', object: '模型', quote: q1, modality: 'fact' },
          // 真结构词：仍要被闸丢
          { subject: '客户', predicate: 'related_to', object: '平台', quote: q1, modality: 'fact' },
        ],
      });
    // 概念 `maps_to` 的**正常路径**（给 `doc merge` 的上游产物用）
    case 'cmap-good':
      return JSON.stringify({
        concepts: [
          { name: '模型', quote: q1, modality: 'fact', maps_to: { table: 'llm_models' } },
          {
            name: '客户',
            quote: q1,
            modality: 'fact',
            maps_to: { table: 'customers', column: 'email' },
          },
        ],
        relations: [
          // 抽象概念之间的关系：**没有** join 列 ⇒ 不给关系上的 maps_to（REQ-171 的正例）
          { subject: '模型', predicate: 'belongs_to', object: '客户', quote: q1, modality: 'fact' },
        ],
      });
    case 'valid':
    default:
      return JSON.stringify({
        concepts: [{ name: '测试概念', quote: q1, modality: 'fact' }],
        relations: [
          {
            subject: '测试概念',
            predicate: '前置于',
            object: '前置条件',
            quote: q2 || q1,
            modality: 'spec',
          },
        ],
      });
  }
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
      // 版本必须是**具体**的（模糊版本会被 connect 拒掉 —— 那是如实拒绝）
      send({
        jsonrpc: '2.0',
        id: m.id,
        result: {
          protocolVersion: 1,
          agentInfo: { name: 'fake-acp', title: 'fake ACP for tests', version: '0.1.0' },
          agentCapabilities: {},
        },
      });
    } else if (m.method === 'session/new') {
      send({ jsonrpc: '2.0', id: m.id, result: { sessionId: SESSION, configOptions: null } });
    } else if (m.method === 'session/prompt') {
      prompts += 1;
      if (LOG) fs.appendFileSync(LOG, `${prompts}\n`);
      // 诊断用（REQ-171）：把**每次下发的完整 prompt** 追加到文件里。
      // 用途：不花一个 token 就能量出「真 schema + 真语料」下摘要到底多大、里面有哪些表。
      if (process.env.FAKE_ACP_DUMP) {
        const body = m.params.prompt.map((p) => p.text || '').join('');
        fs.appendFileSync(
          process.env.FAKE_ACP_DUMP,
          `\n===== prompt ${prompts} (${body.length} chars) =====\n${body}\n`
        );
      }
      if (MODE === 'hang') {
        // 永不回终响应 —— 客户端必须靠自己的超时闸失败，而不是一直等
        continue;
      }
      // ⚠️ 会话**首轮**先吐一段前言（实测 pi-acp 就是这样）：
      // 不预热的话第一块会拿到「## Skills … {json}」这种不可解析的正文。
      if (prompts === 1) chunk('## Skills\n- 假的技能清单\n- 这一段不是回答\n');
      const text = chunkTextOf(m.params.prompt.map((p) => p.text || '').join(''));
      // `halfgarbage`：**只让第 2 条 prompt（= 第 1 块）**回一段不是 JSON 的话
      // ⇒ 「1 块回答不可用、其余正常」：断言还有、`rejected.parse` = 1、`failed_calls` = 0。
      // `garbagetwo`：让**前两块**都不可用（第 2、3 条 prompt）—— 用来构造
      // 「2 块 / 12 块 = 16.7%」这种**低于** 20% 的比例（阈值判据必须两侧都成立）。
      const garbageBlocks = MODE === 'halfgarbage' ? 1 : MODE === 'garbagetwo' ? 2 : 0;
      const isGarbage = prompts >= 2 && prompts < 2 + garbageBlocks;
      const answer = isGarbage ? '好的，我来抽取这一段：' : answerFor(text);
      const mid = Math.floor(answer.length / 2);
      // 分两片发：正文必须靠**累积**才完整（F-1）
      chunk(answer.slice(0, mid));
      chunk(answer.slice(mid));
      // `flaky`：**恰好让一块失败**（第 2 条 prompt = 第 1 块）。
      // 手法 = 这一轮**一条 `usage_update` 都不报** —— 客户端必须把它判成失败
      //（「不知道花了多少」不许当成 0）。于是得到「1 块失败 + 其余成功」的中间态：
      // 产物存在，但**不可重放**（失败块不进缓存，重跑会重调它）。
      if (!(MODE === 'flaky' && prompts === 2)) {
        usage += 1000;
        note({ sessionUpdate: 'usage_update', used: usage, size: 200000 });
      }
      send({ jsonrpc: '2.0', id: m.id, result: { stopReason: 'end_turn' } });
    } else {
      send({ jsonrpc: '2.0', id: m.id, error: { code: -32601, message: 'method not found' } });
    }
  }
});
