//! ⭐ REQ-177 · **多项目模式下给 `--access-mapping` ⇒ 启动即拒（退出码 2）**。
//!
//! 背景（客户实测）：多项目部署里带着 `--access-mapping`，某个项目**自己没有 AM** ⇒
//! 回落到这份**进程级**映射 ⇒ `bind` 对不上（14 个类不存在）⇒ 整个项目 500
//! `project.load_failed`。REQ-174 之后 AM 的归属是**版本**（随构建包入库、按
//! `active_version_id` 各自加载），进程级一份只属于单项目模式。
//!
//! | 判据 | 本文件里的用例 |
//! | --- | --- |
//! | **AC-001** | [`multi_project_with_access_mapping_exits_2`]：多项目 + AM ⇒ **exit 2**，stderr 带「多项目」「随构建包入库」 |
//! | **AC-004** | 同一条用例把 `ONTOK_DATABASE_URL` 指向**连不上**的地址：仍是参数错（⛔ 不是连接错） |
//! | AC-002 | [`single_project_with_access_mapping_is_not_rejected`]：单项目 + AM **不会被入参闸拦**（文件优先由 `test_req144_access` / `test_req174_version_am` 的 AC-005 覆盖） |
//! | AC-003 | [`multi_project_without_access_mapping_is_not_rejected`]：多项目不给 AM **不会被入参闸拦** |
//!
//! ⚠️ 三条用例都**不连真库**（用 127.0.0.1:1 这种必拒的地址），所以它们是纯本地的；
//! 「真库 + 多项目照常服务」由 `test_req159b_multi` 覆盖。
//!
//! **红绿自证**：把 `query::serve::preflight` 里
//! `if matches!(scope, ProjectScope::Multi) && access_mapping.is_some()` 这段注掉 ⇒
//! AC-001 用例红（进程会去连库，退出码 1 + 连接错误）；恢复 ⇒ 绿。

use std::path::{Path, PathBuf};
use std::process::Command;

fn ontok_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ontok"))
}

/// 一个绝对连不上的库地址：连接被拒是**立即**失败（不会挂住用例）。
const UNREACHABLE_DB: &str = "postgresql://nobody:nobody@127.0.0.1:1/none";

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ontok-req177-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("建临时目录");
    dir
}

/// 多项目模式的 token 表（每一条都绑项目 —— 这是那套闸门的输入；本需求在它之前就拒）。
fn write_multi_tokens(dir: &Path) -> PathBuf {
    let tokens = dir.join("tokens.json");
    std::fs::write(
        &tokens,
        r#"{"version":2,"tokens":[
            {"label":"ops-assistant","token":"FAKE-REQ177-MULTI-aaaaaaaaaaaaaaaa","project":"cloud-resource"}
        ]}"#,
    )
    .expect("写 token 表");
    tokens
}

/// AM 文件内容无所谓（拒绝只看「给没给」）；给一份形状合法的，免得红绿实验时
/// 「文件不存在」抢在参数错前面报。
fn write_access_mapping(dir: &Path) -> PathBuf {
    let am = dir.join("access-mapping.json");
    std::fs::write(
        &am,
        r#"{"contract_version":1,"mapping_version":1,"entities":[]}"#,
    )
    .expect("写 AM");
    am
}

fn run_serve(tokens: &Path, extra: &[&str]) -> std::process::Output {
    Command::new(ontok_bin())
        .args([
            "query",
            "serve",
            "--bind",
            "127.0.0.1:0",
            "--tokens",
            tokens.to_str().unwrap(),
            "--ontology-only",
        ])
        .args(extra)
        .env("ONTOK_DATABASE_URL", UNREACHABLE_DB)
        .env_remove("ONTOK_TEST_DATABASE_URL")
        .output()
        .expect("跑 ontok query serve")
}

/// **AC-001 + AC-004** · 多项目（不给 `--project` / `--version`）+ `--access-mapping`：
/// - **退出码恰好 2**（入参问题，不是 1 —— 1 会被读成「服务/环境坏了」）；
/// - stderr 含「多项目」「随构建包入库」（说清归属与为什么）；
/// - stderr **不含连接错误**：库地址是必然连不上的，可它压根没被用到 ⇒ 拒绝在连库之前。
#[test]
fn multi_project_with_access_mapping_exits_2() {
    let dir = temp_dir("reject");
    let tokens = write_multi_tokens(&dir);
    let am = write_access_mapping(&dir);

    let out = run_serve(
        &tokens,
        &["--access-mapping", am.to_str().unwrap()],
    );
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    assert_eq!(
        out.status.code(),
        Some(2),
        "多项目 + --access-mapping 必须 exit 2（实际 {:?}）：\n{stderr}",
        out.status
    );
    assert!(stderr.contains("多项目"), "要点明多项目模式：\n{stderr}");
    assert!(
        stderr.contains("随构建包入库"),
        "要说清 AM 随构建包入库（REQ-174）：\n{stderr}"
    );
    assert!(stderr.contains("--access-mapping"), "要点名参数：\n{stderr}");
    // AC-004：连不上的库也没被碰到 —— 出现连接类错误就说明顺序错了。
    let lower = stderr.to_lowercase();
    assert!(
        !lower.contains("connection")
            && !lower.contains("connect")
            && !lower.contains("127.0.0.1:1")
            && !lower.contains("database")
            && !lower.contains("迁移"),
        "拒绝必须发生在连库之前（不该出现连接 / 迁移错误）：\n{stderr}"
    );
    // ⛔ 拒绝了就不该有「就绪」横幅。
    assert!(!stderr.contains("就绪"), "拒绝了就不该打就绪横幅：\n{stderr}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// **AC-003 的入参层** · 多项目 + **不给** AM ⇒ 入参闸放行：
/// 进程继续走到连库，于是以连接失败退出（`≠ 2`，且没有那句参数错）。
///
/// 变异：把拒绝写成「多项目一律拒」（不看 `access_mapping`）⇒ 本测试红。
#[test]
fn multi_project_without_access_mapping_is_not_rejected() {
    let dir = temp_dir("allow-multi");
    let tokens = write_multi_tokens(&dir);

    let out = run_serve(&tokens, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    assert_ne!(
        out.status.code(),
        Some(2),
        "多项目不给 AM 不该被参数闸拦：\n{stderr}"
    );
    assert!(
        !stderr.contains("随构建包入库"),
        "不该出现 REQ-177 的拒绝文案：\n{stderr}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// **AC-002 的入参层** · 单项目 + `--access-mapping` ⇒ 入参闸放行（文件优先，行为不变）。
///
/// 同一条命令只差 `--project` / `--version`：它同样只走到连库就停（测试故意用坏库），
/// 但那必须是**连接**失败，⛔ 不是 REQ-177 的参数错。
#[test]
fn single_project_with_access_mapping_is_not_rejected() {
    let dir = temp_dir("allow-single");
    let tokens = write_multi_tokens(&dir);
    let am = write_access_mapping(&dir);

    let out = run_serve(
        &tokens,
        &[
            "--project",
            "cloud-resource",
            "--version",
            "1",
            "--access-mapping",
            am.to_str().unwrap(),
        ],
    );
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    assert_ne!(
        out.status.code(),
        Some(2),
        "单项目 + AM 不该被参数闸拦：\n{stderr}"
    );
    assert!(
        !stderr.contains("随构建包入库"),
        "单项目模式不该出现 REQ-177 的拒绝文案：\n{stderr}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
