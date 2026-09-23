# Sarmg Upgrade

Sarmg Upgrade `0.3.2` 是 Sarmg 产品的离线维护工具。它依据显式产品适配器和版本清单执行支持检查、备份、校验与恢复，把高风险的数据操作从在线产品进程中分离出来。

当前实现只接受工具中明确登记的产品、平台和状态身份。它不会猜测未知版本，也不会把“能够读取备份”解释为“能够升级到任意版本”；执行前请先查询实际支持范围。

当前起点是五个 Server 的持久化状态：Host `0.9.26`、Sunshine `0.10.1`、Media `0.3.0`、Sentinel `0.2.2`、Dufs `0.51.0`。这些是数据库中的状态版本，可能低于 Server 二进制版本。后续 Server 若保持同一状态身份，可继续使用现有适配器；状态格式变化时须新增精确适配器和迁移边，并由新工具版本通过测试与发布。当前 `upgrade_edges` 为空。

## 快速开始

查看当前构建支持的产品与操作：

```sh
cargo build --release --locked
./target/release/sarmg-upgrade support --json
./target/release/sarmg-upgrade --help
```

执行任何写操作前，先停止对应产品服务，并使用绝对路径。具体命令参数取决于 `support --json` 返回的产品适配器；先查看子命令帮助，再执行预检：

```sh
./target/release/sarmg-upgrade backup-current --help
./target/release/sarmg-upgrade verify-current --help
./target/release/sarmg-upgrade restore-current --help
```

备份产物、签名与恢复目标不应放在产品状态目录内。完整的支持矩阵、操作示例、失败恢复和审计要求见[运维文档](docs/operations.md)。

## 开发验证

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
python3 scripts/check-workflow-supply-chain.py
```

## 文档

- [文档总览](docs/README.md)
- [初学者指南](docs/beginner-guide/README.md)
- [项目工作流程](docs/project-workflow.md)
- [功能范围与取舍](docs/feature-inventory-and-tradeoffs.md)
- [操作与恢复](docs/operations.md)

代码采用 [Apache License 2.0](LICENSE-APACHE)。
