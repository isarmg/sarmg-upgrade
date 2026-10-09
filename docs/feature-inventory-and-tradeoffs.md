# 功能、验证与取舍

`0.4.0` 只支持当前基线到未来同结构发行物。六个 Server 使用统一机制；产品负责普通实际结构/资源诊断，本工具维护签名升级制品与部署定义，Foundation 无产品分支。

| 功能 | 实现与证据 | 约束 |
|---|---|---|
| 来源可信 | Ed25519、独立 DER 信任锚、严格签名 DTO | 不信任包自带摘要或密钥 |
| 实际发行身份 | ELF/AMD64/hash、封存目标 `release-identity` | unbound 只在明确开发测试控制器可用 |
| 部署合同 | 工具受签名 artifact，单 ELF 或完整 Root | 不能用 binary-only 升级需完整 Web/companion 的产品 |
| 完整 Root | 精确模式/树 hash、NOREPLACE 发布、受控 current | 任意 symlink 和未知 inode 拒绝 |
| 完整原组备份 | program/config/data/外置资源/Root 全量 inventory | 停服、持锁，WAL/SHM 同组保护 |
| 真实当前状态 | 源/目标只读 config validate、state_paths 覆盖 | 完全相同 SchemaIdentity，变化合同明确拒绝 |
| 权限桥 | .7 行政 API、root-owned 执行目录、FD4、清组降权 | 服务不能写 sealed ELF/祖先，DynamicUser 不支持 |
| UID/GID 恢复 | 逐项所有权、nofollow FD fchown 和模式验证 | 不复制 ACL/xattr/硬链接关系 |
| 互斥与门 | 公共实例/维护锁、pending、Root 部署协调锁 | 普通 runtime 严格 actual-euid，不放宽 |
| 明确阶段 | intent 先持久、快照完备标记、原子切换 | 部分快照不用于恢复 |
| 中断恢复 | 依据 journal+实际程序/Root/备份校验恢复整组 | 未知状态不覆盖 |
| 新写入保护 | 交接前可能写入标记、显式 allow-data-loss | 启动失败不证明没有写入 |
| 真实就绪 | systemd MainPID/UID/exe hash+正确 service HTTP ready | 端口可连或进程存在不够 |
| 有界进程 | stdout/stderr 各 1MiB、1–600s、超限 kill+wait | stderr 不进入普通错误 |
| 有界快照 | 128 资源、128 深度、200万项/资源、1TiB 最大预算 | 精确输入与完整 hash 仍有 I/O 成本 |
| 统一日志 | Foundation LogRecord、安全码和操作 UUID | 不输出内部链或配置秘密 |
| 权威支持表 | 六 Server 的 generic upgrade/recovery；库无 runtime | 不登记历史 source 或推断数据版本 |

## Foundation 边界

共享 contracts/state-file/fs-safety/log 固定官方 URL、精确 0.10.8 与 full revision。Foundation 提供中立合同、权限、锁、原子文件和日志，工具实现阶段/恢复/信任与部署策略。产品验证真实 DDL/完整性/业务约束；生产工具没有独立 SQLite 历史 parser。真实产品 tests 才使用 rusqlite 核对完整业务行。

## 已验证与未验证

自动机制测试覆盖签名、资源外漏、危险路径、完整树 Python/Rust 摘要一致、未知/损坏 Root、维修中断、实际服务 UID65534、验证器写入、启动失败与新写入保护。真实当前产品 opt-in 测试验证业务数据和 actual runtime ready，仅生命周期管理器为测试替身。

本地测试、source-bound 制品验收、真实 systemd 和正式远端 CI 分别报告。同一当前发行物重装不证明不存在的未来版本。软件版本和稳定数据格式身份分离；今后真实结构改变时才增加该发行的明确转换。

## 未纳入范围

本轮之前的旧格式升级、未知结构猜测、任意迁移 hooks、零停机更新、DynamicUser、ACL/xattr/硬链接保留均未实现。保留数据优先于自动猜测恢复。预基线备份 adapters 因输入/结构合同不匹配删除，整组安全恢复由统一流程继续提供。
