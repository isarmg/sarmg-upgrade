# 08. 测试、调试与新增 Adapter

## 8.1 基础门禁

```bash
python3 scripts/check-workflow-supply-chain.py
cargo +1.98.0 fmt --all -- --check
cargo +1.98.0 check --locked --all-targets --all-features
cargo +1.98.0 clippy --locked --all-targets --all-features -- -D warnings
cargo +1.98.0 test --locked
bash -n scripts/*.sh
git diff --check
```

## 8.2 Fixture 原则

source fixture 由对应历史产品事实生成并固定 identity；target 由当前 target SQL/code 创建。测试数据不得
包含生产 Secret。恶意 fixture 覆盖 Schema 假报、sidecar、链接、mode、路径穿越和超预算。

## 8.3 故障注入

在 copy、file sync、manifest、directory sync、stage、journal、preserve、install、verify、cleanup 每个持久
边界注入错误/kill。检查原件、backup、recovery 和重复执行的可解释性。

## 8.4 新 Adapter 步骤

1. 固定 product/from/to 和完整资源合同。
2. 取得可信 source fixture 与规范 identity。
3. 实现只读 source validator。
4. 实现 source backup/verify。
5. 从零构造 target，并显式转换字段/资源。
6. 复用 journal framework但保留产品专用阶段。
7. 加密文、树、锁、预算、崩溃负例。
8. 更新 support/catalog/docs/release tests。

## 8.5 调试顺序

先确认 binary/support，再查参数/path identity、锁、source identity、clone、manifest、target build、journal
阶段。不要边调试边修改生产原件。

## 8.6 安全审查问题

不可信数据在哪首次解析？容量在哪里限制？路径如何锚定？哪一步首次 mutation？之前是否有 source backup？
掉电后 journal 是否足够？raw key 能否进入输出？错误是否会误删证据？

## 8.7 名称/合同变化

同步 crate/binary/package、support/catalog product slug、manifest、命令、recovery 名、脚本、SBOM/provenance、
测试与文档；删除旧 alias。全文和路径搜索只是开始，还需真实 release 解包验证。

## 8.8 提交标准

一个 adapter/重大安全边界一个提交；完整门禁和故障矩阵通过；无 fixture 漂移、Secret、target、临时 backup
或 recovery；文档命令由当前 CLI 帮助核对。
