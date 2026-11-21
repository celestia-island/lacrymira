# AGENTS.md — lacrymira 不变量

## 提交署名（2026-11-22 用户裁决）

- **作者/提交者一律 = 伊欧 <langyo.china@gmail.com>**（repo 级 git config 已设；勿回落到 global noreply）。
- **智能体经手的提交必须追加尾注** `Co-authored-by: demiurge <demiurge@celestia.world>`——是额外署名，
  不是顶替作者；禁止以智能体身份作为 author/committer 出现（历史教训：`lead@celestia.island` 事故）。
- squash merge 的 `squash_commit_message` 同样带该尾注。

## 分支与 PR

- 只有 `master`；改动一律 worktree（`_tools/wt_new.py`）+ feature 分支 + PR + squash merge；不设长期 dev 分支。
- 标题 `<gitmoji> 一句英文陈述句.`；不维护 CHANGELOG；设计说明写 PR 描述。

## 门禁（合并前本地真跑；`verify:` 行写进 PR 描述）

`just gate` = fmt / clippy `-D warnings` / `cargo test` / `wasm32-wasip2`（core+redb+wasi）/ wit-check / tokio-check。

- WASI 三个 crate（lacrymira、lacrymira-redb、lacrymira-wasi）依赖树**零 tokio**。
- `crates/core/tests/contract.rs` 的 msgpack 契约（编码→解码→再编码逐字节相同）是不可跳过的冻结测试。
- DB-backed 测试（exec / migrate / save_e2e）默认 `#[ignore]`，配方在各自文件头；改动执行器/迁移器时须对真 PG 跑过并留证。

## 依赖

- 版本声明单段 caret（`^1` / `^0.4`）；禁三段式、`^*`、`^0.*`、裸 `*`（技能 dep-spec 全文为准）。
- 线格式唯一 = msgpack（rmp-serde）；时间库 jiff 一等、chrono 仅 feature 门后。
