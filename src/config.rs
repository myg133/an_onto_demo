//! 运行时配置 —— **只从环境变量读，代码里不出现任何口令**。
//!
//! 口令不入库是硬约束（`sprint/current.md` 阻塞项 4）：
//! `.env.example` 只写变量名，不写真实值。

/// 测试库连接串的环境变量名。**不提供默认值** —— 缺了必须报错，
/// 不能静默连到某个「看起来像」的库上。
pub const TEST_DATABASE_URL_ENV: &str = "ONTOK_TEST_DATABASE_URL";

/// 应用运行时连接串的环境变量名。
pub const DATABASE_URL_ENV: &str = "ONTOK_DATABASE_URL";

/// REQ-093 · 只读图服务的**访问令牌**。
///
/// **只从环境变量读，代码里不出现任何令牌值**（与上面两条同一纪律）。
///
/// ⚠️ **没有开发模式开关**：缺这个变量就**拒绝启动**，不留「本地调试时
/// 可以先跳过」的旁路 —— 那正是三周后没人记得它还开着的那个开关。
pub const VIEW_TOKEN_ENV: &str = "ONTOK_VIEW_TOKEN";

/// 读一个必需的环境变量。
///
/// 参数是 `&'static str` 而非 `&str` —— 错误信息要带上变量名，
/// 而 `ConfigError::MissingEnv` 持有它，所以生命周期必须够长。
/// 调用方传的其实都是本文件里的常量或字面量，够用。
pub fn require_env(key: &'static str) -> Result<String, ConfigError> {
    match std::env::var(key) {
        Ok(v) if !v.trim().is_empty() => Ok(v),
        _ => Err(ConfigError::MissingEnv(key)),
    }
}

/// 测试库连接串。**测试专用** —— 缺失时测试应直接失败。
pub fn test_database_url() -> Result<String, ConfigError> {
    let url = require_env(TEST_DATABASE_URL_ENV)?;
    // ── 防污染闸 ──────────────────────────────────────────────────────────
    // 为什么需要（2026-10-27 / 10-28 两次事故）：本机能连到的库只有 UAT 那台，
    // 于是 `ONTOK_TEST_DATABASE_URL` 常年指着 **UAT 元数据库**。测试每跑一次就往里
    // 塞十几个 `t_req…` 项目（实测把 UAT 塞到 15 个项目、Studio 里全是打不开的）。
    // 测试库必须是**专用库**，⛔ 不许指向业务库。
    if !test_db_looks_dedicated(&url) && !allow_shared_test_db() {
        return Err(ConfigError::SharedDatabaseRefused {
            env: TEST_DATABASE_URL_ENV,
            db: database_name(&url).unwrap_or_else(|| "<解析不出库名>".into()),
        });
    }
    Ok(url)
}

/// 库名是否**看起来是专用测试库**：`test` / `scratch` / `tmp` / `ci` / `dev` 之一。
///
/// 判据刻意宽松（宁可误放行 `*_dev`，也不许写进业务库），但**必须**能识别 UAT 的 `ontok`。
fn test_db_looks_dedicated(url: &str) -> bool {
    let name = match database_name(url) {
        Some(n) => n.to_ascii_lowercase(),
        None => return false,
    };
    ["test", "scratch", "tmp", "ci", "dev"]
        .iter()
        .any(|k| name.contains(k))
}

/// 从连接串里取库名（`scheme://user:pass@host:port/db?…` 的最后一段，去 query）。
fn database_name(url: &str) -> Option<String> {
    let after_scheme = url.split("://").nth(1)?;
    let host_and_db = after_scheme.split('@').last()?;      // 去凭据
    let db_part = host_and_db.split('/').nth(1)?;           // 第一个 '/' 之后是库名
    let name = db_part.split(['?', '#']).next()?.trim();
    if name.is_empty() { None } else { Some(name.to_string()) }
}

/// 逃生阀：某些环境确实没有专用库时，显式设 `ONTOK_TEST_DB_ALLOW_SHARED=1`。
/// ⚠️ 只在**明知会污染**且**接受**时用（例如一次性 CI 容器）。
fn allow_shared_test_db() -> bool {
    matches!(
        std::env::var("ONTOK_TEST_DB_ALLOW_SHARED").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    )
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("缺少环境变量 {0}（口令不得写入代码或文档，只走环境变量）")]
    MissingEnv(&'static str),

    #[error(
        "⛔ {env} 指向的库 `{db}` 看起来不是专用测试库 —— 测试会往里塞满 `t_req…` 项目。\n         修：换成专用库（库名含 test/scratch/tmp/ci/dev），例如 \n         \x20   ONTOK_TEST_DATABASE_URL=postgresql://…:30432/ontok_test_scratch\n         确实要用共享库（明知污染）时：ONTOK_TEST_DB_ALLOW_SHARED=1"
    )]
    SharedDatabaseRefused { env: &'static str, db: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_missing_env_is_error_not_default() {
        // 关键：缺变量必须报错，**不得**回落到某个默认连接串
        let key = "ONTOK_DEFINITELY_NOT_SET_12345";
        assert!(matches!(require_env(key), Err(ConfigError::MissingEnv(_))));
    }

    #[test]
    fn test_config_empty_value_is_treated_as_missing() {
        // 空字符串等价于未设置 —— 否则会拿着 "" 去连库，报错信息误导
        let key = "ONTOK_EMPTY_TEST_VAR_12345";
        std::env::set_var(key, "   ");
        assert!(matches!(require_env(key), Err(ConfigError::MissingEnv(_))));
        std::env::remove_var(key);
    }
}
