# 从独立 Client 文档集中迁出的说明

按最新归属规则，以下当前工作树文档不再保存 Server 版本升级或本工具的说明。这里只记录原出处与清理范围，不把旧说明作为当前支持承诺。原有不可变 tag 与源码包保持原字节；清理在其后独立文档提交中完成。

| 原仓库 | 原出处 | 集中清理内容 |
|---|---|---|
| Host Client | `docs/releases/cli-unreleased.md` | 本工具与Client状态恢复的归属免责声明 |
| xscc | `docs/releases/0.1.0-rc.{1,2}.md` | Server升级/恢复的归属说明 |
| Media Client | `docs/manual-backup-implementation.md`、流程/功能/初学者文档、`docs/releases/1.0.0.md`及`0.4.0.md` | Server离线转换/组合备份/工具支持范围和跳转 |
| xcoc | `docs/operations.md`、`docs/releases/1.0.0.md` | Server版本变更次序说明 |

四个Client自身状态、软件版、普通安装器生命周期和当前Server协议依赖继续由各产品说明；本工具不处理移动本地队列或Client凭据。Server升级当前范围与实际证据分别见[规范](development-contract.md)和[验证](validation.md)。

## 后续用户帮助文本清理

以下间接旧说明从当前Client工作树移出；它们不能证明本工具支持Client本地状态。不可变运行时tag原文保持封存，随后提交只调整文档和卸载脚本的输出文字，未改Rust运行代码或卸载动作。

原出处：`xszc/docs/beginner-guide/09-deployment-security-and-operations.md`

```text
使用升级工具导出所需当前备份
```

原出处：`xszc/docs/beginner-guide/README.md`

```text
若它报告版本/Schema 不匹配，不要强行修表，应停止产品并使用独立升级工具。
```

原出处：`xszc/docs/project-workflow.md`

```text
   └─ 独立升级工具执行一致性备份、恢复或版本转换
```

原出处：`xszc/docs/feature-inventory-and-tradeoffs.md`

```text
三域术语、逐 route threat model、升级转换
```

原出处：`xsoc/packaging/linux/postremove.sh`

```text
配置文件只遵循包管理器的 config/noreplace 语义；产品不会备份或恢复它。完整备份、恢复和跨版本升级请使用独立升级仓库。
```
