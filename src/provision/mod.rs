//! ⭐ REQ-159 · **服务端与构建端共用的底层件**（从 `src/console/` 搬出来）
//!
//! | 模块 | 是什么 | 谁用 |
//! | --- | --- | --- |
//! | [`dsn`] | DSN / query key 的**加密列**（AES-256-GCM，密钥来自 `ONTOK_SECRET_KEY`） | `query serve`（写面）、console（`provision`） |
//! | [`bundle`] | `.ontokbundle` 的**只读解析**（store-only zip / manifest / 8 个拒绝码） | `query serve`（上传）、console（本地导出后自检） |
//!
//! **为什么单独成模块**：这两个件既不属于"服务面"也不属于"本地评审台"，
//! 放进 `src/console/` 会让"console 归谁"与"它们归谁"混在一起（2026-10-27 的真实事故：
//! 差点被当成 console 的代码一起删掉，而删了 `query serve` 就编译不过）。
//!
//! ⚠️ `provision_api`（8 条写端点的语义层）**暂时仍在 `src/console/`**：
//! 它依赖 `console::http` 的响应辅助函数，等 REQ-162 删掉 console 的旧库路由后
//! （那时它唯一的调用方只剩 `query serve`），再一并搬进来。

pub mod bundle;
pub mod dsn;
