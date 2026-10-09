# 6. 产品权威和完整保护范围

产品提供真实当前结构与业务校验。通用工具只调用固定标准命令，不根据产品名解释业务数据库，也不运行任意脚本。

`config validate --config PATH --data-dir PATH --json` 输出 `schema_identity` 与 `state_paths`。每个路径必须属于计划保护资源。SQLite 包含父目录，外置媒体、密钥、recordings 和不可重建配置必须完整列出。

`release-identity --json` 输出精确产品/软件版/full source revision/target/state-contract hash。工具验证签名和编译身份完全相同；unbound 开发程序不能生产升级。

完整发行方式由本工具受控产品定义与签名 `artifact` 声明。产品没有升级专用命令；普通当前状态校验仍核对真实 Web/companion/合同资产。完整 Root 包含所有不可变资产，不可仅替换其中 binary。

产品只读校验可能需要实际服务 UID。工具先打开 root 控制的固定 ELF FD4，再清环境、清 supplementary groups、降 GID/UID。私有 journal/备份留 root 0700，公开发行执行闭包不能交给服务用户修改。

当前支持检查、签名定义及路径规则见[完整合同](../offline-upgrades.md)。
