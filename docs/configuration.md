# 准备升级计划

每次事务使用一份私有 JSON 计划。计划将发行方提供的签名制品映射到本机服务、配置、数据和恢复目录。完整示例见[执行升级](offline-upgrades.md#执行升级)。

## 准备输入

1. 从发行方取得目标程序或完整发行目录、`upgrade-release.json` 和 `upgrade-release.sig`。
2. 从独立可信渠道取得产品发行公钥及其 DER SHA-256 指纹。
3. 查看本机 unit 的 `User`、`Group`、`ExecStart`，以及产品当前 `release-identity`、`config validate` 和 `state-contract` 输出。
4. 按实际路径填写计划，保存到操作者的私有目录；恢复目录使用尚未存在、且位于业务数据之外的绝对路径。

xssc 工具本身和目标产品使用各自的发行信任锚；此计划填写目标产品的公钥身份。

## 字段参考

除标为可选或给出默认值的字段外，其余字段均需填写。JSON 只接受已定义字段。

| 字段 | 值与用途 |
|---|---|
| `plan_version` | `1` |
| `service` | 本机主 systemd 单元，例如 `xocs.service` |
| `additional_services` | 可选，默认空；与签名 `additional_service_roles` 顺序一致的 `{role, service}` 数组，最多 16 项 |
| `installed_binary` | 当前程序的规范绝对路径 |
| `target_binary` | 已取得的目标程序绝对路径 |
| `release_manifest` / `release_signature` | 签名升级清单及其签名文件 |
| `trusted_public_key` | 产品发行公钥 PEM 文件 |
| `trusted_public_key_sha256` | 独立确认的公钥 DER SHA-256，64 位小写十六进制 |
| `config` / `data_dir` | 当前产品配置文件与主状态目录 |
| `resources` | 与签名定义顺序、名称和类型一致的 `{name, kind, path}` 数组；`kind` 为 `file` 或 `directory` |
| `work_directory` | 本次全新的私有事务与恢复目录 |
| `readiness_address` | 回环 IP 与端口，例如 `127.0.0.1:8080`，供业务就绪检查 |
| `timeout_seconds` | 默认 `60`；允许 `1`–`600` 秒，限制辅助命令和就绪等待 |
| `max_backup_bytes` | 默认 `1099511627776`（1 TiB）；正整数，限制本次完整备份字节预算 |
| `native_release` | 可选，完整发行目录使用；包含 `current_link`、`source_root` 和 `install_root` |

## 确认保护范围

`resources` 覆盖产品报告的全部 `state_paths`。SQLite 使用数据库父目录保护主库与 WAL/SHM；外置媒体、录像或密钥各自映射到签名定义中的资源。资源根之间保持独立，避免重叠。

数据目录按产品要求为服务 UID/GID 拥有的物理 `0700` 目录，配置 JSON 为 `0600`，配置父目录可由服务用户访问。程序及封存执行目录由受信任的管理身份控制。当前使用固定 systemd `User`/`Group`；DynamicUser、ACL/xattr 和硬链接关系恢复不在支持范围。

计划准备好后，再协调停止全部写入进程并[执行升级](offline-upgrades.md#执行升级)。工具会在产生事务变更前核对签名、身份、路径和停服状态；它没有独立的 `--dry-run` 命令。
