# Sarmg Upgrade

`sarmg-upgrade 0.2.0` 是 Sarmg 产品的离线备份、验证、恢复和未来升级适配器仓库。业务产品只创建并
接受自身当前版本，不携带旧 Schema reader、自动 migration、兼容 alias、backup writer 或 restore code。

项目仍处于开发阶段，当前没有任何历史升级边；`support --json` 的 `upgrade_edges` 全部为空，二进制也不
提供 `upgrade-*` 命令。已实现范围是 Media Backup 当前组合状态，以及 Host Monitoring、Sunshine Manager
当前 SQLite 的备份/验证/恢复。备份不可变、带摘要且不覆盖；恢复先暂存验证，再通过持久 journal 切换。

## 快速验证

```bash
cargo +1.98.0 fmt --all -- --check
cargo +1.98.0 check --locked --all-targets --all-features
cargo +1.98.0 clippy --locked --all-targets --all-features -- -D warnings
cargo +1.98.0 test --locked --all-targets --all-features
./scripts/check-workflow-supply-chain.py
```

先用机器可读命令确认当前二进制能力：

```bash
sarmg-upgrade support --json
sarmg-upgrade catalog --json
```

## 文档

- [文档总览](docs/README.md)
- [初学者学习指南](docs/beginner-guide/README.md)
- [项目工作流程与流程树](docs/project-workflow.md)
- [完整功能与取舍清单](docs/feature-inventory-and-tradeoffs.md)
- [备份、升级、恢复、安全与发行运维](docs/operations.md)

代码采用 [Apache License 2.0](LICENSE-APACHE)。
