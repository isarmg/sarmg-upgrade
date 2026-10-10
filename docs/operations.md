# 操作、演练与发行

工具仅在 Linux x86_64 GNU 运行，没有账户配对或常驻服务。首次安装、发行包验签、命令路径检查和工具卸载见[部署指南](platform-setup.md)；本页关注被维护的 Server 事务与恢复。

使用 `support --json` 与 `--help` 确认当前构建能力。`1.0.0` 支持六个当前 Server 到未来同结构发行物的统一升级。产品校验声明全部保护资源，发行签名绑定精确目标，软件和数据版本分别验证。

## 操作步骤

1. 从独立可信渠道固定产品签名公钥的 DER SHA-256，并核对正式产品制品和源码身份。
2. 执行当前和目标产品标准命令确认状态、发行身份、完整部署 Root。不要从软件版本推断数据格式。
3. 按[完整契约](offline-upgrades.md)准备私有 plan：固定 unit、config、data-dir、全部外置数据/密钥、全新 work-directory 和本地 readiness 地址。
4. 检查空间和权限。root 管理安装程序，服务 UID/GID 管理私有 0700 数据及 0600 JSON；配置父目录归服务用户。systemd 固定 User/Group 必须与数据一致。
5. 调用 `apply-upgrade --plan`。先由操作者停止全部 writer 并关闭其他管理器自动重启；工具只检查停止状态、设置门、持锁和备份；不要同时人工替换程序、启动服务或执行另一个维护流程。
6. 保存阶段结果和恢复目录。成功必须实际 MainPID/exeSHA/UID 与正确产品业务 ready 全部通过。

## 故障处理

```sh
# 只读检查事务阶段、已完成备份、实际安装程序及维护门状态。
xssc inspect-upgrade --work-directory /absolute/private/recovery
# 条件满足时恢复原程序、配置及数据，并验证原服务业务就绪。
xssc recover-upgrade --work-directory /absolute/private/recovery
```

保留 journal、维护门、所有快照与损坏原 Root 留证。可修正明确环境问题，例如磁盘空间；不能手改 journal/hash、删门或仅换回程序。

在 `start-intent` 或恢复自身启动后，程序可能已写入。默认恢复保全新增业务状态并返回 `RECOVERY_AUTHORIZATION_REQUIRED`。只有操作者明确接受丢弃快照之后的写入时才增加 `--allow-data-loss`。授权后恢复仍逐项验证原程序、原配置、数据和发行 Root，并实际启动原程序验证业务 ready。

未知 schema/现行程序/Root inode、损坏备份、替换的父目录、错误属主、危险链接均停止恢复；保全状态后由操作者处理原因。工具不能从证据不足推断可以覆盖。

## 验收和演练

在私有临时实例验证成功切换、启动失败、验证器写入、Root 资产损坏、恢复中断和启动后新写入保护。机制测试使用真实签名/复制/锁；产品测试运行真实当前 init/validate/run/ready，仅替换 systemd 生命周期。

```sh
# 检查 Rust 格式，不改源码。
cargo fmt --all -- --check
# 使用锁定依赖图运行全部目标/特性的严格静态检查。
cargo clippy --locked --all-targets --all-features -- -D warnings
# 执行 Rust 机制和产品集成测试。
cargo test --locked --all-targets --all-features
# 执行受签名升级制品制作脚本的 Python 测试。
python3 -m unittest discover -s scripts/tests -v
# 验证发行定稿的身份、签名与回解包流程。
python3 tests/test_finalize_release.py
# 验证离线发行文档引用闭包及锚点。
python3 tests/test_release_docs.py
# 检查工作流的权限、不可变来源和供应链约束。
python3 scripts/check-workflow-supply-chain.py
```

root 环境额外运行实际 UID65534 的安全演练，不创建系统用户或改系统服务。真实产品测试参数见[验证边界](offline-upgrades.md#验证边界)。开发未绑定源码的二进制不能用于生产升级；正式验收必须真实 `release-identity` 与签名一致。使用当前制品重装只证明同结构机制，不宣称验证尚未发行的未来版本。

## 发行

只生成唯一 Linux AMD64 GNU 制品。`scripts/stage-release.sh` 要求源码 clean、对应 annotated 软件 tag，将完整当前HEAD作为严格编译输入，进行 locked release 构建并打包 binary、真实 support JSON、文档和源码绑定公钥。`scripts/finalize-release.sh` 核对事件 revision/version、binary/catalog hash 与签名公钥，还在打开签名私钥前实际执行有界 binary support/version，逐项复核编译source/target、catalog与provenance，再签名和解包自验证。

离线包根说明入口链接到 `docs/` 的原始相对结构；`scripts/stage-release-docs.py` 从运维、完整升级合同和部署指南递归封装相对引用，检查本地锚点、越界/链接输入及既有文件覆盖。签名前和解包后再次验证实际文件，缺少部署指南或任一后续引用时拒绝签名。全部封装文档由同一 `SHA256SUMS` 和签名覆盖。

工具只依赖一个 `xcsc =1.0.0` 包，显式启用 `offline-maintenance`；完整 revision、官方 URL 与 `Cargo.lock` 精确绑定，不使用邻仓 path 或远端自动 fallback。`contracts`、`state_file`、`fs_safety`、`log`、`sqlite` 是该 Client 包内的模块。维护仍遵循被维护产品的运行属主与权限合同。

本地冻结、受控缓存/离线构建、正式远端 CI 和公开发行分别记录。每次正式发行都独立核验其完整源码、CI 和回下载资产，不以本地缓存解析代替远端可获取证明。当前唯一发布标签为 `v1.0.0`；历史验收记录用于追溯，不作为当前发行制品的说明。

## 删除的预基线入口

旧独立 xszs、keyed SQLite 和 composite backup 命令依赖预基线硬配置文件名和已不匹配的结构，已删除。稳定数据版本数字低于软件版是正常的；删除依据是当前合同不匹配。升级必需的完整快照、维护锁、中断恢复和业务就绪保护仍包含在统一 apply/inspect/recover 中。

旧版本专用准备入口、旧结构夹具及相关操作说明已删除；当前升级仍严格验证完整备份、状态合同和业务就绪。
