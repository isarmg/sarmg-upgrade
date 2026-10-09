# xssc

xssc `1.0.0` 更新到正式发布的 Foundation Server 0.11.7。离线升级事务、恢复日志、签名和信任锚沿用现有合同，操作说明与升级文档继续随发行包提供。改动见[发行说明](docs/releases/1.0.0.md)。

`1.0.0` 修复正式离线包中文档引用不完整的问题：根说明入口指向 `docs/` 中完整的操作、升级合同和 Sunshine 准备说明，封装递归相对引用并核验锚点，签名前及解包后都验证实际闭包。Rust 1.99、SQLx 0.9、Foundation 0.11.7 与 0.6.0 的运行机制继续使用。已公开的旧标签和签名资产保持封存；本补丁须通过实际 CI 后发行。改动见[发行说明](docs/releases/1.0.0.md)。

签名精确绑定当前与目标软件的发行身份；源和目标必须使用完全相同的真实状态合同。未来实际结构变化随该次发行提供明确转换，本轮另提供唯一的 Sunshine 0.15.0 → 0.16.0 [离线观察缓存准备](docs/sunshine-protocol-preparation.md)，与受签名自动切换流程分开；它只失效可重建观察，保留任务、指纹和业务事实。完整发行 Root 由本工具签名制品定义约束，配置、密钥、数据库及媒体由产品普通只读校验确认；软件版本和稳定数据格式身份分别验证。

## 开始使用

```sh
cargo build --release --locked
./target/release/xssc support --json
./target/release/xssc apply-upgrade --help
```

按[离线升级与恢复](docs/offline-upgrades.md)准备受签名制品、独立信任锚和私有升级计划：

```sh
xssc apply-upgrade --plan /absolute/private/plan.json
xssc inspect-upgrade --work-directory /absolute/private/recovery
xssc recover-upgrade --work-directory /absolute/private/recovery
```

取得运行权后可能已有新增业务写入；恢复默认保全这些写入，只有明确接受损失时才使用 `--allow-data-loss`。Linux AMD64、systemd 和 OpenSSL 是当前执行条件。完整运维说明见[操作文档](docs/operations.md)。

## 仓库布局

本仓库是单个 Rust 包：根 `Cargo.toml` 和 `Cargo.lock` 定义唯一依赖图，`src/` 实现 CLI 与升级模块，根 `tests/` 验证发行脚本。`src/upgrade/` 按快照、完整发行树、身份委派和状态机划分责任，模块行为测试放各自测试文件；`scripts/` 负责构建、签名与策略检查，`docs/` 保存全部升级说明和验收事实。本工具没有 Web 或平台 UI 包，不添加空目录。

## 开发验证

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
python3 -m unittest discover -s scripts/tests -v
python3 tests/test_finalize_release.py
python3 tests/test_release_docs.py
python3 scripts/check-workflow-supply-chain.py
```

依赖固定官方 Git URL、完整 revision 和精确版本；本地受控缓存验证不等于远端已发布。五个 Foundation 依赖为 `=0.11.7` / `d58b9ef0822984ee0d29fb8b8139cfd2787374fb`，行政权限桥只由工具消费，运行服务使用同一 `0.11.7` 的严格属主入口。

## 文档

- [文档总览](docs/README.md)
- [初学者指南](docs/beginner-guide/README.md)
- [项目工作流程](docs/project-workflow.md)
- [功能与取舍](docs/feature-inventory-and-tradeoffs.md)
- [操作与恢复](docs/operations.md)

代码采用 [Apache License 2.0](LICENSE-APACHE)。

当前发布版本：**1.0.0**。参见 [1.0.0 发布说明](docs/releases/1.0.0.md)和[项目命名](docs/naming.md)。
