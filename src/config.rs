//! 运行时配置 —— **只从环境变量读，代码里不出现任何口令**。
//!
//! 口令不入库是硬约束（`sprint/current.md` 阻塞项 4）：
//! `.env.example` 只写变量名，不写真实值。

/// 测试库连接串的环境变量名。**不提供默认值** —— 缺了必须报错，
/// 不能静默连到某个「看起来像」的库上。
pub const TEST_DATABASE_URL_ENV: &str = "ONTOK_TEST_DATABASE_URL";

/// 应用运行时连接串的环境变量名。
pub const DATABASE_URL_ENV: &str = "ONTOK_DATABASE_URL";

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
    require_env(TEST_DATABASE_URL_ENV)
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("缺少环境变量 {0}（口令不得写入代码或文档，只走环境变量）")]
    MissingEnv(&'static str),
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
