# xssc 文档

本文档描述 `1.0.0` 候选当前到未来版本的统一升级流程。正式目标为 Linux AMD64 GNU；工具没有常驻服务或前端。`support --json` 列出六个服务端的统一机制支持。实际资源、结构和部署方式始终由产品当前校验与签名目标定义决定。

| 文档 | 读者任务 |
|---|---|
| [Linux 部署与维护](platform-setup.md) | 安装依赖、下载和验签、安装工具、诊断、更新与卸载；确认无需配对和无常驻服务 |
| [初学者指南](beginner-guide/README.md) | 理解信任、快照、维护锁、阶段和恢复 |
| [流程](project-workflow.md) | 阅读入口与状态机并定位实现 |
| [功能与取舍](feature-inventory-and-tradeoffs.md) | 核对实现、验证和限制 |
| [离线升级](offline-upgrades.md) | 制作制品、准备计划和执行恢复 |
| [运维](operations.md) | 检查条件、演练、故障处理与发行 |
| [产品仓库](server-client-repositories.md) | 确认产品权威输入的归属 |
| [1.0.0 候选](releases/1.0.0.md) | 查看本轮范围和验证边界 |

本工具只依赖一个 `xcsc` 包，启用 `offline-maintenance` 特性；`xcsc::contracts`、`xcsc::state_file`、`xcsc::fs_safety`、`xcsc::log` 和 `xcsc::sqlite` 均为内部模块。该包固定官方 Git 来源、精确 `=1.0.0` 与完整提交修订 `c45e48e93e360542c2e1db6c6441a9e29b344b03`。本工具拥有编排、签名/发行策略、文件快照及恢复状态机；产品拥有结构和业务约束。SQLite 用于当前状态的有界只读验证和产品测试；它不提供任意 SQL 钩子或运行时历史兼容。

当前共享输入是 xcsc 单体；历史 0.6.0 原生 CI 与正式发行记录仅证明其当时输入。1.0.0 的源码、原生 CI 与签名制品证据独立记录，不能继承上个版本的发行结论。

[原生边界审核](unsafe-audit.md)记录当前工具的必要 unsafe、已采用的安全替代与实际验证范围。

正式离线包保留仓库中的 `docs/` 相对结构；根 `README.md` 与 `OFFLINE-UPGRADES.md` 指向完整运维、升级和部署说明。所有相对文档/资源引用与本地锚点在封装、签名前和解包后验证，源文档不为打包重复维护一套。

公共支撑的职责、单体依赖、平台边界与验证方法见[公共支撑说明](common-support.md)。

## 当前合同与源码组织

签名精确绑定当前与目标软件的发行身份；源和目标必须使用完全相同的真实状态合同，软件版本与稳定数据格式身份分别验证。未来实际结构变化随该次发行提供明确转换，当前不提供旧版本专用适配。完整发行根由本工具的签名制品定义约束；配置、密钥、数据库与媒体由产品普通只读校验确认。

仓库是单个 Rust 包：根 `Cargo.toml` 和 `Cargo.lock` 定义唯一依赖图，`src/` 实现 CLI 与升级模块，根 `tests/` 验证发行脚本。`src/upgrade/` 按快照、完整发行树、身份委派和状态机划分责任，模块行为测试放各自测试文件；`scripts/` 负责构建、签名与策略检查，`docs/` 保存升级说明和验收事实。本工具没有 Web 或平台 UI 包。

当前工具链为 Rust `1.99.0`、SQLx `0.9`，共享客户端库为 `xcsc =1.0.0`，只启用 `offline-maintenance`。正式来源固定官方 Git URL、完整修订和精确版本；本地受控缓存验证不能代替远端发布证据。工具遵循被维护产品已有的属主与权限合同。

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

## 升级与恢复入口

先按[部署指南](platform-setup.md)验证并安装已绑定源码身份的正式程序，再按[完整离线升级合同](offline-upgrades.md)准备受签名制品、独立信任锚、私有计划和停止全部相关写入进程：

```sh
xssc apply-upgrade --plan /absolute/private/plan.json
xssc inspect-upgrade --work-directory /absolute/private/recovery
xssc recover-upgrade --work-directory /absolute/private/recovery
```

三条命令分别执行升级、只读检查事务阶段和实际安装身份、按完整条件恢复原程序/配置/数据并验收原服务，不能作为无条件连续执行的脚本。取得运行权后可能有新业务写入；恢复默认保全这些写入，只有明确接受损失时才使用 `--allow-data-loss`。详细故障处理见[运维](operations.md)。

代码采用 [Apache License 2.0](../LICENSE-APACHE)。当前版本与范围见 [1.0.0 发布说明](releases/1.0.0.md)，唯一发布标签为 `v1.0.0`；历史验收仅用于追溯。
