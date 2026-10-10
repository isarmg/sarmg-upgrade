# xssc

xssc `1.0.0` 使用单体客户端公共支撑 xcsc `1.0.0`，提供离线升级事务、恢复日志、签名验证和独立信任锚。操作说明与升级文档随发行包提供。改动见[发行说明](docs/releases/1.0.0.md)。

离线包根说明入口指向 `docs/` 中完整的操作、升级合同和部署指南，递归封装相对引用并核验锚点；签名前及解包后都验证实际文档闭包。当前工具链为 Rust 1.99 和 SQLx 0.9，共享客户端基础库版本为 xcsc `1.0.0`。当前唯一发布标签为 `v1.0.0`，历史验收记录仅用于追溯。

签名精确绑定当前与目标软件的发行身份；源和目标必须使用完全相同的真实状态合同。未来实际结构变化随该次发行提供明确转换；当前不提供旧版本专用适配。完整发行 Root 由本工具签名制品定义约束，配置、密钥、数据库及媒体由产品普通只读校验确认；软件版本和稳定数据格式身份分别验证。

## 开始使用

正式部署先按[Linux 安装、检查与卸载](docs/platform-setup.md)下载和验证签名，安装已绑定源码身份的发行二进制。xssc 仅在 Linux x86_64 GNU 上运行，无需配对，没有自己的常驻服务。以下是源码开发构建，不能替代正式发行验收：

```sh
# 使用锁定依赖图构建本机 Release 模式的开发程序。
cargo build --release --locked
# 查看编译目标、源码身份及升级/恢复能力；不执行升级。
./target/release/xssc support --json
# 查看实际升级参数；不改产品状态。
./target/release/xssc apply-upgrade --help
```

按[离线升级与恢复](docs/offline-upgrades.md)准备受签名制品、独立信任锚和私有升级计划：

```sh
# 根据私有计划进行目标验签、备份、切换和业务就绪检查。
xssc apply-upgrade --plan /absolute/private/plan.json
# 只读核验本次事务阶段、已完成备份及实际安装身份。
xssc inspect-upgrade --work-directory /absolute/private/recovery
# 在完整恢复条件满足后恢复原程序、配置和数据，并验收原服务。
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

依赖固定官方 Git URL、完整 revision 和精确版本；本地受控缓存验证不等于远端已发布。只依赖一个 `xcsc =1.0.0` 包，并显式启用 `offline-maintenance`；完整源码 revision 由 `Cargo.toml` 与 `Cargo.lock` 固定。发行身份、状态文件、文件安全、日志及 SQLite 维护是包内模块。工具通过独立的 Client 实现离线维护，并遵循被维护产品的既定属主与权限合同。

## 文档

- [文档总览](docs/README.md)
- [Linux 安装、检查与卸载](docs/platform-setup.md)
- [初学者指南](docs/beginner-guide/README.md)
- [项目工作流程](docs/project-workflow.md)
- [功能与取舍](docs/feature-inventory-and-tradeoffs.md)
- [操作与恢复](docs/operations.md)

代码采用 [Apache License 2.0](LICENSE-APACHE)。

当前发布版本：**1.0.0**。参见 [1.0.0 发布说明](docs/releases/1.0.0.md)和[项目命名](docs/naming.md)。

公共支撑的职责、单体依赖、平台边界与验证方法见[公共支撑说明](docs/common-support.md)。
