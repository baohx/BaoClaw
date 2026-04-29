# Project Notes

- ## 500 错误修复 (2026-04-28)

**问题**：agent loop 每次都以 "server error 500" 终止，无法恢复。

**根因**：`query_engine.rs:710` 对所有非 RateLimited 的 API 错误（包括 500）直接 `return` 退出 loop，不重试。

**修复**：
1. **500 重试**：`query_engine.rs` 新增 `ApiError::ServerError` 分支，指数退避重试 3 次，耗尽后尝试 fallback model
2. **FallbackController**：新增 `server_error_count`、`on_server_error()`、`on_server_error_exhausted()` 方法
3. **400 context overflow**：新增 `ApiError::BadRequest` 分支，检测到 context/token/too large 关键字时自动 compact 后重试
4. **loop 内 token 检查**：每轮 API 调用前检查 `estimate_tokens` > 400K 时自动 compact（原来是 800K 且只在 submit 时检查一次）
5. **compact_messages**：新增独立函数，在 loop 中复用，避免 borrow checker 冲突

**涉及文件**：
- `baoclaw-core/src/engine/query_engine.rs` — 三处修改
- `baoclaw-core/src/api/fallback.rs` — FallbackController 扩展
