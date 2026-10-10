# 2. 准备环境和首次验证

正式执行环境为 Linux AMD64 GNU、systemd、OpenSSL Ed25519。先读当前程序 `config validate`、`release-identity`、`state-contract` 的只读结果，确认产品、完整数据路径及部署方式。

配置 JSON 为服务用户私有 0600，配置父目录归该用户并可访问；主状态根物理 0700。root 可管理服务 UID 的维护锁/门，但运行程序仍按实际属主权限执行。安装发行根目录/current 的可遍历父目录由 root 持有且组/其他用户不可写。

工具计划使用规范绝对路径、固定服务单元和本地就绪检查地址，资源不重叠。恢复目录全新、私有，不能位于业务数据内部。要有足够空间保存原程序/完整发行根目录/配置/数据以及封存目标。

```sh
xssc support --json
xssc catalog --json
xssc apply-upgrade --help
```

首次学习请用临时实例，不要手改真实实例事务日志或解除门。完整计划和签名制作步骤见[离线合同](../offline-upgrades.md)。
