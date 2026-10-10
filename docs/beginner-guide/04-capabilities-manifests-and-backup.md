# 4. 能力、签名和备份

`support --json` 声明六个 Server 具备统一升级/恢复机制。实际资源和软件身份来自产品；不要从 catalog 或数据库标签推测具体程序发行版。

签名 manifest 包含真实 ReleaseIdentity、ELF hash、相同 source/target schema、精确资源名称/种类及可选完整 Root artifact。额外字段拒绝。制品中的公钥不是信任来源，必须独立固定公钥 DER SHA-256。

完整 Root 的所有文件/目录、mode、长度和 hash 参与树摘要。目标先封存在 root 控制的执行闭包，校验实际 `release-identity` 与工具受签名 artifact，程序必须已由操作者停止。

备份包含程序、完整源 Root、所有配置和全部 state_paths 覆盖资源。停服+排他锁期间前后核对 inventory/UID/GID，完整后才允许恢复。日后真实结构变化需要明确版本实现，当前不接受 source/target 不同。

签名定义和资源示例见[离线升级](../offline-upgrades.md#制作受信任升级制品)。
