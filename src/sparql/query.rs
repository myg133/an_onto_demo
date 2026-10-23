//! REQ-122 · **SPARQL 子集**（词法 + 语法）—— 纯函数，零 I/O、零新依赖。
//!
//! # 一句话
//!
//! 只认「一条 SELECT + 一个基本图模式（BGP）+ 基本 FILTER + 单列聚合/分组/排序/分页」，
//! 其余**一律报错**。⛔ 没有「认识但忽略」的中间态 —— 静默忽略一个子句，
//! 数字员工会以为过滤生效了，而结果集是全量的。
//!
//! # 支持（v1 清单，逐条有测试）
//!
//! | 能力 | 形状 |
//! | --- | --- |
//! | BGP | `?s :类__列 ?o .` / `?s :类__父类 ?p`，1–3 跳 |
//! | 类型 | `?s a :类`（只要 `rdf:type`，且宾语必须是类 IRI） |
//! | 简写 | `;`（同主语多谓词）、`,`（同谓词多宾语） |
//! | 字面量宾语 | `"RMB"` / `1` / `1.5` / `true` / `"2026-01-01"^^xsd:date` |
//! | 实例宾语 | `<…/客户/c-1>`（按映射里的实例 IRI 模板反解 id） |
//! | FILTER | `= != < > <= >=`、`&&`、`\|\|`、`!`、括号 |
//! | 分页 | `LIMIT n` / `OFFSET n`（顺序任意） |
//! | 排序 | `ORDER BY ?v` / `ORDER BY DESC(?v)`（单列） |
//! | 聚合 | `COUNT` / `SUM` / `MIN` / `MAX` / `AVG` + `GROUP BY ?v`（单列） |
//! | 去重 | `SELECT DISTINCT`（v1 清单之外的**唯一**额外项，见模块尾） |
//!
//! # 不支持（⛔ 报错，不忽略）
//!
//! `OPTIONAL` / `UNION` / `MINUS` / 子查询 / `BIND` / `VALUES` / `SERVICE` / `GRAPH` /
//! 属性路径 `* + / ^ |` / `CONSTRUCT` / `DESCRIBE` / `ASK` / `INSERT` / `DELETE` /
//! `REGEX` 与一切函数调用 / 算术表达式 / 谓词变量 / 多列 GROUP BY / HAVING / FROM。
//!
//! 错误一律**可读 + 带位置**（`（位置 N）` 是输入里的字符偏移），并说清「v1 支持什么」。
//!
//! # 前缀从哪来
//!
//! 与 TTL 同源：`:` = 本体 IRI + `#`（由调用方给出，见 `TtlOptions::ontology_iri`）；
//! `rdf` / `rdfs` / `xsd` / `owl` 内置；查询里 `PREFIX` 声明的覆盖内置的。
//! `BASE` 接受但只用于解析相对 IRI（v1 不支持相对 IRI，给了会报错）。

use std::collections::BTreeMap;

use super::error::QueryError;

// ══════════════════════════════════════════════════════════════
// AST
// ══════════════════════════════════════════════════════════════

/// 解析结果：一条 `SELECT`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub distinct: bool,
    pub projection: Projection,
    pub patterns: Vec<TriplePattern>,
    pub filters: Vec<Expr>,
    /// `GROUP BY` 的变量（v1 只允许一个）。
    pub group_by: Option<String>,
    pub order_by: Option<OrderBy>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

impl Query {
    /// 这条查询里出现过的变量（**首次出现顺序**，`SELECT *` 用）。
    pub fn vars_in_order(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut push = |v: &str| {
            if !out.iter().any(|x| x == v) {
                out.push(v.to_string());
            }
        };
        for p in &self.patterns {
            if let SubjectTerm::Var(v) = &p.subject {
                push(v);
            }
            if let ObjectTerm::Var(v) = &p.object {
                push(v);
            }
        }
        out
    }
}

/// 投影。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Projection {
    /// `SELECT *`
    All,
    Items(Vec<SelectItem>),
}

/// 一个投影项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectItem {
    Var(String),
    /// `(COUNT(?x) AS ?n)` —— `var = None` 表示 `COUNT(*)`。
    Agg { func: AggFunc, var: Option<String>, distinct: bool, alias: String },
}

impl SelectItem {
    /// 输出列名（= 变量名，不带 `?`）。
    pub fn column_name(&self) -> &str {
        match self {
            SelectItem::Var(v) => v,
            SelectItem::Agg { alias, .. } => alias,
        }
    }
}

/// 五个聚合。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggFunc {
    Count,
    Sum,
    Min,
    Max,
    Avg,
}

impl AggFunc {
    pub fn as_str(self) -> &'static str {
        match self {
            AggFunc::Count => "COUNT",
            AggFunc::Sum => "SUM",
            AggFunc::Min => "MIN",
            AggFunc::Max => "MAX",
            AggFunc::Avg => "AVG",
        }
    }

    /// SUM / AVG 要求数值列（MIN / MAX 什么类型都行）。
    pub fn needs_numeric(self) -> bool {
        matches!(self, AggFunc::Sum | AggFunc::Avg)
    }
}

/// `ORDER BY` 单列。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderBy {
    pub var: String,
    pub descending: bool,
}

/// 一个三元组模式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriplePattern {
    pub subject: SubjectTerm,
    /// 已解析的谓词 IRI（`a` 已展开成 `rdf:type`）。
    pub predicate: String,
    pub object: ObjectTerm,
    /// 在输入里的字符位置（报错用）。
    pub pos: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubjectTerm {
    Var(String),
    Iri(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectTerm {
    Var(String),
    Iri(String),
    Literal(Literal),
}

/// 字面量。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Literal {
    /// 词法形式（字符串已解转义）。
    pub text: String,
    pub kind: LiteralKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiteralKind {
    String,
    Integer,
    Decimal,
    Boolean,
}

impl Literal {
    /// 能不能当数字用（决定「数值列 + 数字字面量」时是否原样写进 SQL）。
    pub fn numeric_value(&self) -> Option<f64> {
        match self.kind {
            LiteralKind::Integer | LiteralKind::Decimal => self.text.parse::<f64>().ok(),
            _ => None,
        }
    }
}

/// FILTER 表达式（只有布尔组合与比较，⛔ 没有函数/算术）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Or(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Cmp(CmpOp, Operand, Operand),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

impl CmpOp {
    pub fn sql(self) -> &'static str {
        match self {
            CmpOp::Eq => "=",
            CmpOp::Ne => "!=",
            CmpOp::Lt => "<",
            CmpOp::Gt => ">",
            CmpOp::Le => "<=",
            CmpOp::Ge => ">=",
        }
    }
}

/// 比较的一侧：变量 / IRI / 字面量（⛔ 不支持嵌套表达式）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operand {
    Var(String),
    Iri(String),
    Literal(Literal),
}

// ══════════════════════════════════════════════════════════════
// 词法
// ══════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Iri(String),
    PName(String, String),
    Var(String),
    Str(String),
    Num(String),
    Ident(String),
    Punct(char),
    Eof,
}

#[derive(Debug, Clone)]
struct Token {
    tok: Tok,
    pos: usize,
}

fn lex(input: &str) -> Result<Vec<Token>, QueryError> {
    let cs: Vec<char> = input.chars().collect();
    let mut out: Vec<Token> = Vec::new();
    let mut i = 0usize;
    let n = cs.len();

    while i < n {
        let c = cs[i];
        // 空白
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        // 注释（`#` 到行尾）
        if c == '#' {
            while i < n && cs[i] != '\n' {
                i += 1;
            }
            continue;
        }
        // 变量
        if c == '?' || c == '$' {
            let start = i;
            i += 1;
            let mut name = String::new();
            while i < n && (cs[i].is_ascii_alphanumeric() || cs[i] == '_') {
                name.push(cs[i]);
                i += 1;
            }
            if name.is_empty() {
                return Err(QueryError::at("变量名不能为空（`?` 后面要跟名字）", start));
            }
            out.push(Token { tok: Tok::Var(name), pos: start });
            continue;
        }
        // 字符串
        if c == '"' || c == '\'' {
            let quote = c;
            let start = i;
            i += 1;
            let mut s = String::new();
            let mut closed = false;
            while i < n {
                let ch = cs[i];
                if ch == '\\' {
                    i += 1;
                    if i >= n {
                        break;
                    }
                    match cs[i] {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        'r' => s.push('\r'),
                        '\\' => s.push('\\'),
                        '"' => s.push('"'),
                        '\'' => s.push('\''),
                        'u' => {
                            // \uXXXX
                            if i + 4 >= n {
                                return Err(QueryError::at("\\u 转义后面要跟 4 位十六进制", i));
                            }
                            let hex: String = cs[i + 1..=i + 4].iter().collect();
                            let code = u32::from_str_radix(&hex, 16).map_err(|_| {
                                QueryError::at(&format!("\\u{hex} 不是合法的十六进制"), i)
                            })?;
                            let ch = char::from_u32(code)
                                .ok_or_else(|| QueryError::at("\\u 转义不是一个合法字符", i))?;
                            s.push(ch);
                            i += 4;
                        }
                        other => {
                            return Err(QueryError::at(
                                &format!("字符串里的转义 `\\{other}` 不支持（只认 \\n \\t \\r \\\\ \\\" \\' \\uXXXX）"),
                                i,
                            ))
                        }
                    }
                    i += 1;
                    continue;
                }
                if ch == quote {
                    closed = true;
                    i += 1;
                    break;
                }
                s.push(ch);
                i += 1;
            }
            if !closed {
                return Err(QueryError::at("字符串没有闭合（少了收尾的引号）", start));
            }
            out.push(Token { tok: Tok::Str(s), pos: start });
            continue;
        }
        // IRI 或 `<` / `<=`
        if c == '<' {
            if cs.get(i + 1) == Some(&'=') {
                out.push(Token { tok: Tok::Punct('<'), pos: i });
                out.push(Token { tok: Tok::Punct('='), pos: i + 1 });
                i += 2;
                continue;
            }
            // 只有「同一 token 内能找到 `>` 且含 `:`」才当 IRI，否则是比较运算符 `<`。
            let mut j = i + 1;
            let mut is_iri = false;
            while j < n {
                let ch = cs[j];
                if ch == '>' {
                    is_iri = cs[i + 1..j].contains(&':');
                    break;
                }
                if ch.is_whitespace() || ch == '<' || ch == '{' || ch == '}' || ch == '"' {
                    break;
                }
                j += 1;
            }
            if is_iri {
                let iri: String = cs[i + 1..j].iter().collect();
                out.push(Token { tok: Tok::Iri(iri), pos: i });
                i = j + 1;
                continue;
            }
            out.push(Token { tok: Tok::Punct('<'), pos: i });
            i += 1;
            continue;
        }
        if c == '>' {
            if cs.get(i + 1) == Some(&'=') {
                out.push(Token { tok: Tok::Punct('>'), pos: i });
                out.push(Token { tok: Tok::Punct('='), pos: i + 1 });
                i += 2;
            } else {
                out.push(Token { tok: Tok::Punct('>'), pos: i });
                i += 1;
            }
            continue;
        }
        if c == '!' {
            if cs.get(i + 1) == Some(&'=') {
                out.push(Token { tok: Tok::Punct('!'), pos: i });
                out.push(Token { tok: Tok::Punct('='), pos: i + 1 });
                i += 2;
            } else {
                out.push(Token { tok: Tok::Punct('!'), pos: i });
                i += 1;
            }
            continue;
        }
        if c == '&' {
            if cs.get(i + 1) == Some(&'&') {
                out.push(Token { tok: Tok::Punct('&'), pos: i });
                i += 2;
                continue;
            }
            return Err(QueryError::at("`&` 要写成 `&&`", i));
        }
        if c == '|' {
            if cs.get(i + 1) == Some(&'|') {
                out.push(Token { tok: Tok::Punct('|'), pos: i });
                i += 2;
                continue;
            }
            // 单个 `|` 只可能是属性路径
            return Err(QueryError::at(
                "v1 不支持属性路径（`|`）—— 只支持单条属性 IRI",
                i,
            ));
        }
        if matches!(c, '{' | '}' | '(' | ')' | '.' | ';' | ',' | '*' | '=' | '/' | '+' | '^') {
            out.push(Token { tok: Tok::Punct(c), pos: i });
            i += 1;
            continue;
        }
        // 数字
        if c.is_ascii_digit()
            || (c == '-' && cs.get(i + 1).is_some_and(|x| x.is_ascii_digit()))
            || (c == '+' && cs.get(i + 1).is_some_and(|x| x.is_ascii_digit()))
        {
            let start = i;
            let mut s = String::new();
            if c == '-' || c == '+' {
                s.push(c);
                i += 1;
            }
            while i < n && cs[i].is_ascii_digit() {
                s.push(cs[i]);
                i += 1;
            }
            if i < n && cs[i] == '.' && cs.get(i + 1).is_some_and(|x| x.is_ascii_digit()) {
                s.push('.');
                i += 1;
                while i < n && cs[i].is_ascii_digit() {
                    s.push(cs[i]);
                    i += 1;
                }
            }
            if i < n && (cs[i] == 'e' || cs[i] == 'E') {
                let save = i;
                let mut tail = String::new();
                tail.push(cs[i]);
                i += 1;
                if i < n && (cs[i] == '+' || cs[i] == '-') {
                    tail.push(cs[i]);
                    i += 1;
                }
                if i < n && cs[i].is_ascii_digit() {
                    while i < n && cs[i].is_ascii_digit() {
                        tail.push(cs[i]);
                        i += 1;
                    }
                    s.push_str(&tail);
                } else {
                    i = save;
                }
            }
            out.push(Token { tok: Tok::Num(s), pos: start });
            continue;
        }
        // 名字 / 前缀名
        if c.is_ascii_alphabetic() || c == '_' || c == ':' {
            let start = i;
            let mut first = String::new();
            while i < n && (cs[i].is_ascii_alphanumeric() || cs[i] == '_' || cs[i] == '-') {
                first.push(cs[i]);
                i += 1;
            }
            if i < n && cs[i] == ':' {
                i += 1;
                let mut local = String::new();
                while i < n
                    && (cs[i].is_ascii_alphanumeric() || cs[i] == '_' || cs[i] == '-' || cs[i] == '.')
                {
                    local.push(cs[i]);
                    i += 1;
                }
                out.push(Token { tok: Tok::PName(first, local), pos: start });
                continue;
            }
            if first.is_empty() {
                return Err(QueryError::at(&format!("看不懂的字符 `{c}`"), start));
            }
            out.push(Token { tok: Tok::Ident(first), pos: start });
            continue;
        }
        return Err(QueryError::at(&format!("看不懂的字符 `{c}`"), i));
    }
    out.push(Token { tok: Tok::Eof, pos: n });
    Ok(out)
}

// ══════════════════════════════════════════════════════════════
// 语法
// ══════════════════════════════════════════════════════════════

/// 内置前缀（与 TTL 的 `@prefix` 同一套口径；`:` 由调用方给）。
const BUILTIN_PREFIXES: &[(&str, &str)] = &[
    ("rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"),
    ("rdfs", "http://www.w3.org/2000/01/rdf-schema#"),
    ("xsd", "http://www.w3.org/2001/XMLSchema#"),
    ("owl", "http://www.w3.org/2002/07/owl#"),
    ("rr", "http://www.w3.org/ns/r2rml#"),
    ("ontok", "https://ontok.local/schema/"),
];

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

/// 解析一条 SPARQL SELECT。
///
/// `ontology_iri` = `TtlOptions::ontology_iri()`（`<base><project>`，无尾 `#`）——
/// 它决定默认前缀 `:`，与 TTL / R2RML 里的属性 IRI **逐字**相同。
pub fn parse(input: &str, ontology_iri: &str) -> Result<Query, QueryError> {
    let toks = lex(input)?;
    let mut p = Parser {
        toks,
        i: 0,
        prefixes: {
            let mut m: BTreeMap<String, String> = BTreeMap::new();
            m.insert(String::new(), format!("{ontology_iri}#"));
            for (k, v) in BUILTIN_PREFIXES {
                m.insert((*k).to_string(), (*v).to_string());
            }
            m
        },
        base: None,
    };
    p.parse_query()
}

struct Parser {
    toks: Vec<Token>,
    i: usize,
    prefixes: BTreeMap<String, String>,
    base: Option<String>,
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.i].tok
    }

    fn pos(&self) -> usize {
        self.toks[self.i].pos
    }

    fn bump(&mut self) -> Tok {
        let t = self.toks[self.i].tok.clone();
        if self.i + 1 < self.toks.len() {
            self.i += 1;
        }
        t
    }

    fn at_punct(&self, c: char) -> bool {
        matches!(self.peek(), Tok::Punct(x) if *x == c)
    }

    fn eat_punct(&mut self, c: char) -> bool {
        if self.at_punct(c) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// 当前是不是某个关键字（大小写不敏感）。
    fn at_kw(&self, kw: &str) -> bool {
        matches!(self.peek(), Tok::Ident(s) if s.eq_ignore_ascii_case(kw))
    }

    fn eat_kw(&mut self, kw: &str) -> bool {
        if self.at_kw(kw) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect_punct(&mut self, c: char, what: &str) -> Result<(), QueryError> {
        if self.eat_punct(c) {
            Ok(())
        } else {
            Err(QueryError::at(&format!("这里应当是 {what}"), self.pos()))
        }
    }

    fn parse_query(&mut self) -> Result<Query, QueryError> {
        // ── Prologue ─────────────────────────────────────────
        loop {
            if self.at_kw("PREFIX") {
                self.bump();
                let (prefix, pos) = match self.bump() {
                    Tok::PName(p, local) if local.is_empty() => (p, self.toks[self.i - 1].pos),
                    Tok::PName(_, local) => {
                        return Err(QueryError::at(
                            &format!("PREFIX 声明里前缀名后面不能带本地名（`{local}`）"),
                            self.pos(),
                        ))
                    }
                    _ => return Err(QueryError::at("PREFIX 后面要跟前缀名（如 `PREFIX ex:`）", self.pos())),
                };
                let _ = pos;
                let iri = match self.bump() {
                    Tok::Iri(s) => s,
                    _ => return Err(QueryError::at("PREFIX 后面要跟 `<IRI>`", self.pos())),
                };
                if let Some(old) = self.prefixes.insert(prefix.clone(), iri) {
                    let _ = old;
                }
                continue;
            }
            if self.at_kw("BASE") {
                self.bump();
                match self.bump() {
                    Tok::Iri(s) => {
                        self.base = Some(s);
                        continue;
                    }
                    _ => return Err(QueryError::at("BASE 后面要跟 `<IRI>`", self.pos())),
                }
            }
            break;
        }

        // ── 只支持 SELECT ────────────────────────────────────
        for (kw, note) in [
            ("ASK", "ASK 只回答「有没有」，表单里没有列 —— 数字员工无法据此解释数字"),
            ("CONSTRUCT", "CONSTRUCT 产出图、不产出表"),
            ("DESCRIBE", "DESCRIBE 由实现自由决定返回什么，不能作为契约"),
            ("INSERT", "本端点只读（⛔ 没有写路径）"),
            ("DELETE", "本端点只读（⛔ 没有写路径）"),
        ] {
            if self.at_kw(kw) {
                return Err(QueryError::at(
                    &format!("v1 只支持 SELECT，不支持 {kw} —— {note}"),
                    self.pos(),
                ));
            }
        }
        if !self.eat_kw("SELECT") {
            return Err(QueryError::at(
                "v1 只支持以 SELECT 开头的查询（且只能是一条 SELECT）",
                self.pos(),
            ));
        }
        let distinct = self.eat_kw("DISTINCT");

        // ── 投影 ─────────────────────────────────────────────
        let projection = if self.eat_punct('*') {
            Projection::All
        } else {
            let mut items: Vec<SelectItem> = Vec::new();
            loop {
                match self.peek().clone() {
                    Tok::Var(v) => {
                        self.bump();
                        items.push(SelectItem::Var(v));
                    }
                    Tok::Punct('(') => {
                        let start = self.pos();
                        self.bump();
                        let func_name = match self.peek().clone() {
                            Tok::Ident(s) => s,
                            _ => {
                                return Err(QueryError::at(
                                    "v1 的投影只支持变量与聚合（`(COUNT(?x) AS ?n)`），不支持表达式",
                                    start,
                                ))
                            }
                        };
                        let func = match func_name.to_ascii_uppercase().as_str() {
                            "COUNT" => AggFunc::Count,
                            "SUM" => AggFunc::Sum,
                            "MIN" => AggFunc::Min,
                            "MAX" => AggFunc::Max,
                            "AVG" => AggFunc::Avg,
                            other => {
                                return Err(QueryError::at(
                                    &format!(
                                        "v1 不支持函数 {other}（投影里只允许 COUNT/SUM/MIN/MAX/AVG）"
                                    ),
                                    start,
                                ))
                            }
                        };
                        self.bump(); // 函数名
                        self.expect_punct('(', "聚合的 `(`")?;
                        let star = self.eat_punct('*');
                        let distinct = self.eat_kw("DISTINCT");
                        let var = if star {
                            None
                        } else {
                            match self.bump() {
                                Tok::Var(v) => Some(v),
                                _ => {
                                    return Err(QueryError::at(
                                        &format!("{} 的参数要么是 `*`，要么是一个变量", func.as_str()),
                                        self.pos(),
                                    ))
                                }
                            }
                        };
                        self.expect_punct(')', "聚合的 `)`")?;
                        if star && func != AggFunc::Count {
                            return Err(QueryError::at(
                                &format!("只有 COUNT 支持 `*`，{} 需要一个变量", func.as_str()),
                                self.pos(),
                            ));
                        }
                        if !self.eat_kw("AS") {
                            return Err(QueryError::at(
                                &format!(
                                    "聚合必须写成 `({}(…) AS ?名字)` —— 没有名字，结果列没法被下游引用",
                                    func.as_str()
                                ),
                                self.pos(),
                            ));
                        }
                        let alias = match self.bump() {
                            Tok::Var(v) => v,
                            _ => return Err(QueryError::at("AS 后面要跟一个变量名", self.pos())),
                        };
                        self.expect_punct(')', "投影项的 `)`")?;
                        items.push(SelectItem::Agg { func, var, distinct, alias });
                    }
                    _ => {
                        return Err(QueryError::at(
                            "SELECT 后面要跟变量、`*` 或 `(聚合 AS ?变量)`",
                            self.pos(),
                        ))
                    }
                }
                if self.eat_punct(',') {
                    continue;
                }
                // SPARQL 的投影是**空格分隔**的（逗号也可）：后面还是变量或 `(` 就继续。
                if matches!(self.peek(), Tok::Var(_) | Tok::Punct('(')) {
                    continue;
                }
                break;
            }
            if items.is_empty() {
                return Err(QueryError::at("SELECT 后面至少要有一个投影项", self.pos()));
            }
            Projection::Items(items)
        };

        // ── WHERE ────────────────────────────────────────────
        let _ = self.eat_kw("WHERE");
        if self.at_kw("FROM") {
            return Err(QueryError::at(
                "v1 不支持 FROM（数据源就是本项目这一版映射，⛔ 不许换库）",
                self.pos(),
            ));
        }
        if !self.at_punct('{') {
            return Err(QueryError::at(
                "SELECT 之后应当是 `{ … }` 的基本图模式",
                self.pos(),
            ));
        }
        self.bump();
        let (patterns, filters) = self.parse_group()?;
        self.expect_punct('}', "基本图模式的 `}`")?;

        // ── 尾部子句（GROUP BY / ORDER BY / LIMIT / OFFSET，顺序任意）────
        let mut group_by: Option<String> = None;
        let mut order_by: Option<OrderBy> = None;
        let mut limit: Option<u64> = None;
        let mut offset: Option<u64> = None;
        loop {
            if self.at_kw("GROUP") {
                if group_by.is_some() {
                    return Err(QueryError::at("GROUP BY 只能出现一次", self.pos()));
                }
                self.bump();
                if !self.eat_kw("BY") {
                    return Err(QueryError::at("GROUP 后面要跟 BY", self.pos()));
                }
                let name = self.parse_single_var("GROUP BY")?;
                group_by = Some(name);
                continue;
            }
            if self.at_kw("ORDER") {
                if order_by.is_some() {
                    return Err(QueryError::at("ORDER BY 只能出现一次", self.pos()));
                }
                self.bump();
                if !self.eat_kw("BY") {
                    return Err(QueryError::at("ORDER 后面要跟 BY", self.pos()));
                }
                order_by = Some(self.parse_order()?);
                continue;
            }
            if self.at_kw("LIMIT") {
                if limit.is_some() {
                    return Err(QueryError::at("LIMIT 只能出现一次", self.pos()));
                }
                self.bump();
                limit = Some(self.parse_u64("LIMIT")?);
                continue;
            }
            if self.at_kw("OFFSET") {
                if offset.is_some() {
                    return Err(QueryError::at("OFFSET 只能出现一次", self.pos()));
                }
                self.bump();
                offset = Some(self.parse_u64("OFFSET")?);
                continue;
            }
            if self.at_kw("HAVING") {
                return Err(QueryError::at(
                    "v1 不支持 HAVING —— 聚合后的过滤请用 `--query` 拿到结果后在外面做，\
                     或把条件收窄到 WHERE（v1 的 WHERE 在分组前生效）",
                    self.pos(),
                ));
            }
            break;
        }

        match self.peek() {
            Tok::Eof => {}
            other => {
                return Err(QueryError::at(
                    &format!(
                        "查询结束后还有多余内容 `{}` —— v1 不支持一条命令里放多条查询，也不支持 \
                         LIMIT/OFFSET 之外的尾部子句",
                        describe(other)
                    ),
                    self.pos(),
                ))
            }
        }

        if patterns.is_empty() {
            return Err(QueryError::at(
                "基本图模式里一个三元组都没有 —— v1 不支持无模式的 SELECT",
                self.pos(),
            ));
        }

        Ok(Query { distinct, projection, patterns, filters, group_by, order_by, limit, offset })
    }

    /// `GROUP BY ?v` / `GROUP BY (?v)` —— 单列。
    fn parse_single_var(&mut self, what: &str) -> Result<String, QueryError> {
        if self.eat_punct('(') {
            let name = match self.bump() {
                Tok::Var(v) => v,
                _ => return Err(QueryError::at(&format!("{what} 后面要跟一个变量"), self.pos())),
            };
            self.expect_punct(')', "`)`")?;
            Ok(name)
        } else {
            match self.bump() {
                Tok::Var(v) => Ok(v),
                _ => Err(QueryError::at(
                    &format!("{what} 只支持单列：后面要跟一个变量（v1 不支持多列分组）"),
                    self.pos(),
                )),
            }
        }
    }

    /// `ORDER BY ?v` / `ORDER BY DESC(?v)` / `ORDER BY ?v DESC`。
    fn parse_order(&mut self) -> Result<OrderBy, QueryError> {
        let mut descending = false;
        if self.at_kw("ASC") || self.at_kw("DESC") {
            descending = self.at_kw("DESC");
            self.bump();
            self.expect_punct('(', "`(`")?;
            let var = match self.bump() {
                Tok::Var(v) => v,
                _ => return Err(QueryError::at("ASC/DESC 的括号里要跟一个变量", self.pos())),
            };
            self.expect_punct(')', "`)`")?;
            return Ok(OrderBy { var, descending });
        }
        let var = match self.bump() {
            Tok::Var(v) => v,
            _ => {
                return Err(QueryError::at(
                    "ORDER BY 只支持单列：后面要跟一个变量（v1 不支持多列排序，也不支持排序表达式）",
                    self.pos(),
                ))
            }
        };
        if self.at_kw("ASC") || self.at_kw("DESC") {
            descending = self.at_kw("DESC");
            self.bump();
        }
        Ok(OrderBy { var, descending })
    }

    fn parse_u64(&mut self, what: &str) -> Result<u64, QueryError> {
        match self.bump() {
            Tok::Num(s) => s
                .parse::<u64>()
                .map_err(|_| QueryError::at(&format!("{what} 后面要是非负整数，`{s}` 不是"), self.pos())),
            _ => Err(QueryError::at(&format!("{what} 后面要跟一个非负整数"), self.pos())),
        }
    }

    /// `{ … }` 里面：三元组块 + FILTER（顺序任意）。
    fn parse_group(&mut self) -> Result<(Vec<TriplePattern>, Vec<Expr>), QueryError> {
        let mut patterns: Vec<TriplePattern> = Vec::new();
        let mut filters: Vec<Expr> = Vec::new();
        loop {
            if self.at_punct('}') || matches!(self.peek(), Tok::Eof) {
                break;
            }
            // 不支持的关键字：**在这里点名**，而不是含糊地报「语法错误」
            if let Tok::Ident(word) = self.peek().clone() {
                if let Some(msg) = unsupported_keyword(&word) {
                    return Err(QueryError::at(&msg, self.pos()));
                }
            }
            if self.at_kw("FILTER") {
                self.bump();
                if self.at_kw("EXISTS") || self.at_kw("NOT") {
                    return Err(QueryError::at(
                        "v1 不支持 FILTER EXISTS / NOT EXISTS —— 不支持子查询式的存在性判断",
                        self.pos(),
                    ));
                }
                let expr = self.parse_constraint()?;
                filters.push(expr);
                self.eat_punct('.');
                continue;
            }
            if self.at_punct('{') {
                return Err(QueryError::at(
                    "v1 不支持嵌套的图模式 / UNION / 子查询（`{ … }` 里再出现 `{`）—— \
                     一条查询只能是一个扁平的基本图模式",
                    self.pos(),
                ));
            }
            // 三元组
            let subject = self.parse_subject()?;
            let subj_pos = self.pos();
            self.parse_property_list(&subject, subj_pos, &mut patterns)?;
            if self.eat_punct('.') {
                continue;
            }
            if self.at_punct('}') {
                break;
            }
            if self.at_kw("FILTER") {
                continue;
            }
            // 到这儿还没轮到 `.` / `}` / FILTER ⇒ 先看是不是一个**不认识但要点名**的关键字
            self.check_unsupported()?;
            return Err(QueryError::at(
                "三元组之间要用 `.` 分隔（或者 `;` 接同一主语的多个谓词）",
                self.pos(),
            ));
        }
        Ok((patterns, filters))
    }

    /// 当前 token 是「不支持的关键字」⇒ 点名报错（⛔ 不落到含糊的「语法错误」）。
    fn check_unsupported(&self) -> Result<(), QueryError> {
        if let Tok::Ident(word) = self.peek() {
            if let Some(msg) = unsupported_keyword(word) {
                return Err(QueryError::at(&msg, self.pos()));
            }
        }
        Ok(())
    }

    fn parse_subject(&mut self) -> Result<SubjectTerm, QueryError> {
        match self.peek().clone() {
            Tok::Var(v) => {
                self.bump();
                Ok(SubjectTerm::Var(v))
            }
            Tok::Iri(iri) => {
                self.bump();
                Ok(SubjectTerm::Iri(self.resolve_iri(&iri)?))
            }
            Tok::PName(prefix, local) => {
                self.bump();
                Ok(SubjectTerm::Iri(self.resolve_pname(&prefix, &local)?))
            }
            Tok::Ident(word) if word.eq_ignore_ascii_case("FILTER") => {
                Err(QueryError::at("FILTER 不能出现在主语位置", self.pos()))
            }
            other => Err(QueryError::at(
                &format!("三元组的主语只能是变量或 IRI，这里是 `{}`", describe(&other)),
                self.pos(),
            )),
        }
    }

    fn parse_property_list(
        &mut self,
        subject: &SubjectTerm,
        subj_pos: usize,
        patterns: &mut Vec<TriplePattern>,
    ) -> Result<(), QueryError> {
        loop {
            // 谓词
            if self.at_punct('^') {
                return Err(QueryError::at(
                    "v1 不支持属性路径（`^` 反向遍历）—— 反向关系请直接写它的反向谓词名（映射里已给出）",
                    self.pos(),
                ));
            }
            let pred_pos = self.pos();
            let predicate = match self.peek().clone() {
                Tok::Var(v) => {
                    return Err(QueryError::at(
                        &format!("v1 不支持谓词变量 `?{v}` —— 属性名必须写死，否则校验不了它是否存在"),
                        pred_pos,
                    ))
                }
                Tok::Iri(iri) => {
                    self.bump();
                    self.resolve_iri(&iri)?
                }
                Tok::PName(prefix, local) => {
                    self.bump();
                    self.resolve_pname(&prefix, &local)?
                }
                Tok::Ident(word) if word == "a" || word.eq_ignore_ascii_case("a") => {
                    self.bump();
                    RDF_TYPE.to_string()
                }
                other => {
                    return Err(QueryError::at(
                        &format!(
                            "谓词位置只能是属性 IRI 或 `a`（rdf:type），这里是 `{}`",
                            describe(&other)
                        ),
                        pred_pos,
                    ))
                }
            };
            // 属性路径运算符紧跟在谓词后面
            if let Tok::Punct(c @ ('*' | '+' | '/' | '^' | '|')) = self.peek() {
                return Err(QueryError::at(
                    &format!(
                        "v1 不支持属性路径（`{c}`）—— 只支持单个属性 IRI；\
                         多跳请写成多个三元组（沿映射里的 join）"
                    ),
                    self.pos(),
                ));
            }
            // 宾语列表
            loop {
                let object = self.parse_object()?;
                patterns.push(TriplePattern {
                    subject: subject.clone(),
                    predicate: predicate.clone(),
                    object,
                    pos: subj_pos,
                });
                if self.eat_punct(',') {
                    continue;
                }
                break;
            }
            if self.eat_punct(';') {
                // 允许 `; }` / `; .` 收尾
                if self.at_punct('}') || self.at_punct('.') || matches!(self.peek(), Tok::Eof) {
                    break;
                }
                continue;
            }
            break;
        }
        Ok(())
    }

    fn parse_object(&mut self) -> Result<ObjectTerm, QueryError> {
        match self.peek().clone() {
            Tok::Var(v) => {
                self.bump();
                Ok(ObjectTerm::Var(v))
            }
            Tok::Iri(iri) => {
                self.bump();
                Ok(ObjectTerm::Iri(self.resolve_iri(&iri)?))
            }
            Tok::PName(prefix, local) => {
                self.bump();
                // `:类` 出现在宾语位置 = 类 IRI；`:"RMB"` 这种不存在
                Ok(ObjectTerm::Iri(self.resolve_pname(&prefix, &local)?))
            }
            Tok::Str(s) => {
                self.bump();
                let mut lit = Literal { text: s, kind: LiteralKind::String };
                // `@lang` / `^^<datatype>`
                if let Tok::Punct('@') = self.peek() {
                    self.bump();
                    if let Tok::Ident(_) = self.peek() {
                        self.bump();
                    }
                }
                if let Tok::Punct('^') = self.peek() {
                    // `^^`
                    if matches!(self.toks.get(self.i + 1).map(|t| &t.tok), Some(Tok::Punct('^'))) {
                        self.bump();
                        self.bump();
                        match self.peek().clone() {
                            Tok::Iri(_) | Tok::PName(_, _) => {
                                self.bump();
                            }
                            _ => {
                                return Err(QueryError::at(
                                    "`^^` 后面要跟数据类型 IRI",
                                    self.pos(),
                                ))
                            }
                        }
                    }
                }
                lit.kind = LiteralKind::String;
                Ok(ObjectTerm::Literal(lit))
            }
            Tok::Num(s) => {
                self.bump();
                let kind = if s.contains('.') || s.contains('e') || s.contains('E') {
                    LiteralKind::Decimal
                } else {
                    LiteralKind::Integer
                };
                Ok(ObjectTerm::Literal(Literal { text: s, kind }))
            }
            Tok::Ident(word) if word.eq_ignore_ascii_case("true") || word.eq_ignore_ascii_case("false") => {
                self.bump();
                Ok(ObjectTerm::Literal(Literal {
                    text: word.to_ascii_lowercase(),
                    kind: LiteralKind::Boolean,
                }))
            }
            other => Err(QueryError::at(
                &format!(
                    "宾语只能是变量、IRI 或字面量（字符串/数字/true/false），这里是 `{}`",
                    describe(&other)
                ),
                self.pos(),
            )),
        }
    }

    /// `FILTER ( … )`
    fn parse_constraint(&mut self) -> Result<Expr, QueryError> {
        if !self.at_punct('(') {
            // `FILTER regex(?x, "…")` 这类函数调用：点名函数
            if let Tok::Ident(f) = self.peek().clone() {
                if matches!(self.toks.get(self.i + 1).map(|t| &t.tok), Some(Tok::Punct('('))) {
                    return Err(QueryError::at(
                        &format!("v1 不支持函数 `{}` —— 只支持 = != < > <= >=、&& || ! 与括号", f),
                        self.pos(),
                    ));
                }
            }
            return Err(QueryError::at(
                "FILTER 的表达式要用括号包起来，例如 `FILTER (?金额 > 100)`",
                self.pos(),
            ));
        }
        self.bump();
        let e = self.parse_or()?;
        self.expect_punct(')', "FILTER 表达式的 `)`")?;
        Ok(e)
    }

    fn parse_or(&mut self) -> Result<Expr, QueryError> {
        let mut left = self.parse_and()?;
        while self.at_punct('|') {
            self.bump();
            let right = self.parse_and()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, QueryError> {
        let mut left = self.parse_unary()?;
        while self.at_punct('&') {
            self.bump();
            let right = self.parse_unary()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, QueryError> {
        if self.at_punct('!') {
            self.bump();
            let e = self.parse_unary()?;
            return Ok(Expr::Not(Box::new(e)));
        }
        if self.at_punct('(') {
            let start = self.pos();
            self.bump();
            let e = self.parse_or()?;
            self.expect_punct(')', "表达式的 `)`")?;
            // 括号表达式只能作为布尔项；后面紧跟比较运算符 ⇒ 不支持
            if let Tok::Punct(c @ ('=' | '<' | '>' | '!')) = self.peek() {
                return Err(QueryError::at(
                    &format!(
                        "v1 不支持对括号表达式再做比较（`{c}`）—— 比较的两侧只能是变量或字面量"
                    ),
                    start,
                ));
            }
            return Ok(e);
        }
        let left = self.parse_operand()?;
        let op = match self.peek().clone() {
            Tok::Punct('=') => {
                self.bump();
                CmpOp::Eq
            }
            Tok::Punct('!') => {
                // `!` + `=` 已被 lexer 拆成一前一后
                if matches!(self.toks.get(self.i + 1).map(|t| &t.tok), Some(Tok::Punct('='))) {
                    self.bump();
                    self.bump();
                    CmpOp::Ne
                } else {
                    return Err(QueryError::at(
                        "`!` 只能用于布尔取反（`!(…)`），不能放在这个位置",
                        self.pos(),
                    ));
                }
            }
            Tok::Punct('<') => {
                self.bump();
                if self.eat_punct('=') {
                    CmpOp::Le
                } else {
                    CmpOp::Lt
                }
            }
            Tok::Punct('>') => {
                self.bump();
                if self.eat_punct('=') {
                    CmpOp::Ge
                } else {
                    CmpOp::Gt
                }
            }
            _ => {
                // 算术运算符出现在比较运算符的位置 ⇒ 点名叫它
                if let Tok::Punct(c @ ('+' | '-' | '*' | '/')) = self.peek() {
                    return Err(QueryError::at(
                        &format!(
                            "v1 不支持算术表达式（`{c}`）—— FILTER 里只能做 = != < > <= >= 的比较，\
                             不能在比较的一侧做运算"
                        ),
                        self.pos(),
                    ));
                }
                return Err(QueryError::at(
                    &format!(
                        "FILTER 里的 `{}` 后面应当是一个比较运算符（= != < > <= >=）—— \
                         v1 不支持裸变量、算术与函数",
                        describe_operand(&left)
                    ),
                    self.pos(),
                ))
            }
        };
        let right = self.parse_operand()?;
        Ok(Expr::Cmp(op, left, right))
    }

    fn parse_operand(&mut self) -> Result<Operand, QueryError> {
        match self.peek().clone() {
            Tok::Var(v) => {
                self.bump();
                Ok(Operand::Var(v))
            }
            Tok::Iri(iri) => {
                self.bump();
                Ok(Operand::Iri(self.resolve_iri(&iri)?))
            }
            Tok::PName(prefix, local) => {
                self.bump();
                Ok(Operand::Iri(self.resolve_pname(&prefix, &local)?))
            }
            Tok::Str(s) => {
                self.bump();
                Ok(Operand::Literal(Literal { text: s, kind: LiteralKind::String }))
            }
            Tok::Num(s) => {
                self.bump();
                let kind = if s.contains('.') || s.contains('e') || s.contains('E') {
                    LiteralKind::Decimal
                } else {
                    LiteralKind::Integer
                };
                Ok(Operand::Literal(Literal { text: s, kind }))
            }
            Tok::Ident(word) if word.eq_ignore_ascii_case("true") || word.eq_ignore_ascii_case("false") => {
                self.bump();
                Ok(Operand::Literal(Literal {
                    text: word.to_ascii_lowercase(),
                    kind: LiteralKind::Boolean,
                }))
            }
            Tok::Ident(word) => {
                if matches!(self.toks.get(self.i + 1).map(|t| &t.tok), Some(Tok::Punct('('))) {
                    Err(QueryError::at(
                        &format!("v1 不支持函数 `{word}` —— 只支持 = != < > <= >=、&& || ! 与括号"),
                        self.pos(),
                    ))
                } else {
                    Err(QueryError::at(
                        &format!("`{word}` 不是 v1 支持的操作数（只支持变量、字面量、IRI）"),
                        self.pos(),
                    ))
                }
            }
            Tok::Punct(c @ ('+' | '-' | '*' | '/')) => Err(QueryError::at(
                &format!("v1 不支持算术表达式（`{c}`）—— 只在 FILTER 里做比较"),
                self.pos(),
            )),
            other => Err(QueryError::at(
                &format!("这里应当是一个变量、字面量或 IRI，而不是 `{}`", describe(&other)),
                self.pos(),
            )),
        }
    }

    /// `<IRI>` 原样（v1 不做 BASE 相对解析）。
    fn resolve_iri(&self, iri: &str) -> Result<String, QueryError> {
        if iri.contains(':') {
            Ok(iri.to_string())
        } else {
            Err(QueryError::at(
                &format!("相对 IRI `<{iri}>` 不支持（v1 只认绝对 IRI）"),
                self.pos(),
            ))
        }
    }

    fn resolve_pname(&self, prefix: &str, local: &str) -> Result<String, QueryError> {
        let Some(ns) = self.prefixes.get(prefix) else {
            let known: Vec<String> = self
                .prefixes
                .keys()
                .map(|k| if k.is_empty() { "`:`".to_string() } else { format!("`{k}:`") })
                .collect();
            return Err(QueryError::at(
                &format!(
                    "前缀 `{prefix}:` 没有声明。已内置：{} —— 其它前缀请用 `PREFIX {prefix}: <…>` 声明",
                    known.join("、")
                ),
                self.pos(),
            ));
        };
        if local.is_empty() {
            return Err(QueryError::at(
                &format!("`{prefix}:` 后面缺少本地名"),
                self.pos(),
            ));
        }
        Ok(format!("{ns}{local}"))
    }
}

/// 图模式里出现这些关键字 ⇒ **点名拒绝**（⛔ 不静默忽略）。
fn unsupported_keyword(word: &str) -> Option<String> {
    let up = word.to_ascii_uppercase();
    let msg = match up.as_str() {
        "OPTIONAL" => "v1 不支持 OPTIONAL —— 可选匹配会让「没有这条关系」和「值为空」混成一件事",
        "UNION" => "v1 不支持 UNION —— 一条查询只能是一个基本图模式",
        "MINUS" => "v1 不支持 MINUS",
        "BIND" => "v1 不支持 BIND —— 查询里不能定义新变量",
        "VALUES" => "v1 不支持 VALUES —— 请把取值写成 FILTER 里的 `||`",
        "SERVICE" => "v1 不支持 SERVICE —— 不跨端点查询",
        "GRAPH" => "v1 不支持 GRAPH —— 只有一个默认图",
        _ => return None,
    };
    Some(format!("{msg}（⛔ 不会静默忽略该子句）"))
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Iri(s) => format!("<{s}>"),
        Tok::PName(p, l) => format!("{p}:{l}"),
        Tok::Var(v) => format!("?{v}"),
        Tok::Str(s) => format!("\"{s}\""),
        Tok::Num(n) => n.clone(),
        Tok::Ident(w) => w.clone(),
        Tok::Punct(c) => c.to_string(),
        Tok::Eof => "（结尾）".to_string(),
    }
}

fn describe_operand(o: &Operand) -> String {
    match o {
        Operand::Var(v) => format!("?{v}"),
        Operand::Iri(i) => format!("<{i}>"),
        Operand::Literal(l) => l.text.clone(),
    }
}

// ══════════════════════════════════════════════════════════════
// 测试
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    const ONT: &str = "https://ontok.local/demo";

    fn p(q: &str) -> Result<Query, QueryError> {
        parse(q, ONT)
    }

    fn err(q: &str) -> String {
        p(q).expect_err("这条查询应当被拒绝").message().to_string()
    }

    #[test]
    fn a_single_hop_select_parses() {
        let q = p(r#"SELECT ?d ?a WHERE { ?d :deductions__deduction_amount ?a }"#).unwrap();
        assert_eq!(q.patterns.len(), 1);
        assert_eq!(q.patterns[0].predicate, format!("{ONT}#deductions__deduction_amount"));
        assert_eq!(q.patterns[0].subject, SubjectTerm::Var("d".into()));
        assert_eq!(q.patterns[0].object, ObjectTerm::Var("a".into()));
    }

    #[test]
    fn a_two_hop_bgp_with_semicolon_and_comma_parses() {
        let q = p(r#"SELECT * { ?u :a__b ?x ; :a__c ?y , ?z . ?x :b__c ?w }"#).unwrap();
        assert_eq!(q.patterns.len(), 4);
        assert_eq!(q.projection, Projection::All);
        assert_eq!(q.vars_in_order(), vec!["u", "x", "y", "z", "w"]);
    }

    #[test]
    fn prefix_declarations_override_the_builtin_ones() {
        let q = p(r#"PREFIX ex: <https://example.com/> SELECT ?s { ?s ex:p ?o }"#).unwrap();
        assert_eq!(q.patterns[0].predicate, "https://example.com/p");
    }

    #[test]
    fn the_default_prefix_is_the_ontology_namespace() {
        let q = p(r#"SELECT ?s { ?s a :customers . ?s :customers__customer_name ?n }"#).unwrap();
        assert_eq!(q.patterns[0].predicate, RDF_TYPE);
        assert_eq!(q.patterns[1].predicate, format!("{ONT}#customers__customer_name"));
    }

    #[test]
    fn filters_with_and_or_not_parentheses_parse() {
        let q = p(
            r#"SELECT ?s { ?s :a__b ?x . FILTER ((?x > 1 && ?x <= 9) || !(?x != 3)) }"#,
        )
        .unwrap();
        assert_eq!(q.filters.len(), 1);
        assert!(matches!(q.filters[0], Expr::Or(_, _)));
    }

    #[test]
    fn limit_offset_order_by_in_any_order_parse() {
        let q = p(r#"SELECT ?s { ?s :a__b ?x } OFFSET 5 LIMIT 3 ORDER BY DESC(?x)"#).unwrap();
        assert_eq!(q.limit, Some(3));
        assert_eq!(q.offset, Some(5));
        assert_eq!(q.order_by, Some(OrderBy { var: "x".into(), descending: true }));
    }

    #[test]
    fn aggregates_and_group_by_parse() {
        let q = p(
            r#"SELECT ?c (SUM(?a) AS ?total) (COUNT(*) AS ?n) { ?s :a__c ?c . ?s :a__a ?a } GROUP BY ?c"#,
        )
        .unwrap();
        assert_eq!(q.group_by.as_deref(), Some("c"));
        match &q.projection {
            Projection::Items(items) => {
                assert_eq!(items.len(), 3);
                assert!(matches!(&items[1], SelectItem::Agg { func: AggFunc::Sum, var: Some(v), alias, .. } if v == "a" && alias == "total"));
                assert!(matches!(&items[2], SelectItem::Agg { func: AggFunc::Count, var: None, alias, .. } if alias == "n"));
            }
            _ => panic!("应当是 Items"),
        }
    }

    #[test]
    fn literals_of_every_supported_kind_parse() {
        let q = p(
            r#"SELECT ?s { ?s :a__s "RMB" ; :a__i 3 ; :a__d -1.5 ; :a__b true }"#,
        )
        .unwrap();
        let kinds: Vec<LiteralKind> = q
            .patterns
            .iter()
            .map(|x| match &x.object {
                ObjectTerm::Literal(l) => l.kind,
                _ => panic!("应当是字面量"),
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                LiteralKind::String,
                LiteralKind::Integer,
                LiteralKind::Decimal,
                LiteralKind::Boolean
            ]
        );
    }

    #[test]
    fn instance_iris_and_typed_literals_parse() {
        let q = p(
            r#"SELECT ?s { ?s :a__c <https://ontok.local/demo#customers/c-1> . ?s :a__t "2026-01-01"^^xsd:date }"#,
        )
        .unwrap();
        assert!(matches!(&q.patterns[0].object, ObjectTerm::Iri(i) if i.ends_with("#customers/c-1")));
        assert!(matches!(&q.patterns[1].object, ObjectTerm::Literal(l) if l.text == "2026-01-01"));
    }

    #[test]
    fn string_escapes_are_decoded() {
        let q = p(r#"SELECT ?s { ?s :a__s "a\"b\\c\nd" }"#).unwrap();
        match &q.patterns[0].object {
            ObjectTerm::Literal(l) => assert_eq!(l.text, "a\"b\\c\nd"),
            _ => panic!(),
        }
    }

    // ── 不支持的语法：逐条报错（AC-002 / R1）─────────────────────

    #[test]
    fn optional_is_rejected_by_name() {
        let e = err(r#"SELECT ?s { ?s :a__b ?x OPTIONAL { ?s :a__c ?y } }"#);
        assert!(e.contains("OPTIONAL"), "{e}");
        assert!(e.contains("不会静默忽略"), "{e}");
    }

    #[test]
    fn union_is_rejected_by_name() {
        let e = err(r#"SELECT ?s { { ?s :a__b ?x } UNION { ?s :a__c ?y } }"#);
        // 嵌套 `{` 会先被当成子查询拦下 —— 两条都必须是「点名拒绝」
        assert!(e.contains("UNION") || e.contains("子查询"), "{e}");
    }

    #[test]
    fn a_plain_union_between_triples_is_rejected_by_name() {
        let e = err(r#"SELECT ?s { ?s :a__b ?x . ?s :a__c ?y UNION ?s :a__d ?z }"#);
        assert!(e.contains("UNION"), "{e}");
    }

    #[test]
    fn a_subquery_is_rejected() {
        let e = err(r#"SELECT ?s { { SELECT ?s { ?s :a__b ?x } } }"#);
        assert!(e.contains("子查询") || e.contains("嵌套的图模式"), "{e}");
    }

    #[test]
    fn bind_is_rejected() {
        let e = err(r#"SELECT ?s { ?s :a__b ?x BIND(?x AS ?y) }"#);
        assert!(e.contains("BIND"), "{e}");
    }

    #[test]
    fn property_paths_are_rejected() {
        for q in [
            r#"SELECT ?s { ?s :a__b* ?x }"#,
            r#"SELECT ?s { ?s :a__b+ ?x }"#,
            r#"SELECT ?s { ?s :a__b/:b__c ?x }"#,
            r#"SELECT ?s { ?s ^:a__b ?x }"#,
        ] {
            let e = err(q);
            assert!(e.contains("属性路径"), "{q} → {e}");
        }
    }

    #[test]
    fn construct_describe_ask_are_rejected() {
        for (q, kw) in [
            (r#"CONSTRUCT { ?s :a__b ?x } WHERE { ?s :a__b ?x }"#, "CONSTRUCT"),
            (r#"DESCRIBE ?s WHERE { ?s :a__b ?x }"#, "DESCRIBE"),
            (r#"ASK { ?s :a__b ?x }"#, "ASK"),
        ] {
            let e = err(q);
            assert!(e.contains(kw), "{q} → {e}");
            assert!(e.contains("只支持 SELECT"), "{e}");
        }
    }

    #[test]
    fn writes_are_rejected_as_read_only() {
        for q in ["INSERT DATA { <a:x> <a:y> 1 }", "DELETE WHERE { ?s ?p ?o }"] {
            let e = err(q);
            assert!(e.contains("只读"), "{q} → {e}");
        }
    }

    #[test]
    fn regex_and_functions_are_rejected_by_name() {
        for q in [
            r#"SELECT ?s { ?s :a__b ?x FILTER regex(?x, "a") }"#,
            r#"SELECT ?s { ?s :a__b ?x FILTER (?x = STR(?s)) }"#,
            r#"SELECT (YEAR(?d) AS ?y) { ?d :a__b ?x }"#,
        ] {
            let e = err(q);
            assert!(e.contains("不支持函数"), "{q} → {e}");
        }
    }

    #[test]
    fn having_and_from_are_rejected() {
        assert!(err(r#"SELECT ?s { ?s :a__b ?x } HAVING (?x > 1)"#).contains("HAVING"));
        assert!(err(r#"SELECT ?s FROM <x:y> { ?s :a__b ?x }"#).contains("不支持 FROM"));
    }

    #[test]
    fn a_predicate_variable_is_rejected() {
        let e = err(r#"SELECT ?s { ?s ?p ?x }"#);
        assert!(e.contains("谓词变量"), "{e}");
    }

    #[test]
    fn multiple_group_by_columns_are_rejected() {
        let e = err(r#"SELECT ?a ?b (COUNT(*) AS ?n) { ?s :a__a ?a . ?s :a__b ?b } GROUP BY ?a ?b"#);
        assert!(e.contains("单列") || e.contains("GROUP BY") || e.contains("多余内容"), "{e}");
    }

    #[test]
    fn an_aggregate_without_as_is_rejected() {
        let e = err(r#"SELECT (COUNT(?x)) { ?s :a__b ?x }"#);
        assert!(e.contains("AS"), "{e}");
    }

    #[test]
    fn arithmetic_is_rejected() {
        let e = err(r#"SELECT ?s { ?s :a__b ?x FILTER (?x + 1 > 2) }"#);
        assert!(e.contains("算术"), "{e}");
    }

    #[test]
    fn unknown_prefixes_are_named() {
        let e = err(r#"SELECT ?s { ?s ex:p ?o }"#);
        assert!(e.contains("ex:") && e.contains("没有声明"), "{e}");
    }

    #[test]
    fn an_empty_where_is_rejected() {
        let e = err("SELECT ?s { }");
        assert!(e.contains("一个三元组都没有"), "{e}");
    }

    #[test]
    fn a_query_without_select_is_rejected() {
        let e = err(r#"?s :a__b ?x"#);
        assert!(e.contains("只支持以 SELECT 开头"), "{e}");
    }

    #[test]
    fn error_positions_point_into_the_input() {
        let e = err(r#"SELECT ?s { ?s :a__b ?x OPTIONAL { ?s :a__c ?y } }"#);
        assert!(e.contains("位置"), "{e}");
    }
}
