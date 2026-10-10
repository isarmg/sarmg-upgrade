# 1. 项目要解决什么

升级包含程序、配置和业务数据的对应关系。工具在操作者停止六个 Server 及相关 writer 后处理可信制品、可恢复快照、完整发行物切换和实际业务就绪。xcss 是公共库，不运行服务；独立 Client 的本地状态不由此工具处理。

当前范围仅从本轮当前基线到未来同结构版本。源/目标 SchemaIdentity 必须相同，由两个产品程序只读检查真实状态。软件版可以增长而数据标签保持不变。未来出现真实结构变化时再提供该次发行的明确转换。

必须分清三个事实：文件已下载不证明可信；启动命令成功不证明业务 ready；程序被换回不证明配置/数据恢复。工具分别记录这些事实并设置维护门，失败保留证据。

主要入口：`apply-upgrade --plan`、`inspect-upgrade --work-directory`、`recover-upgrade --work-directory`。`support` 列出机制能力，实际实例资源以产品验证和签名定义为准。

继续阅读[环境与初次验证](02-safe-environment-and-first-validation.md)。
