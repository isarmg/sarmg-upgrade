> 项目名称已规范化，版本、提交、摘要及验收状态保持历史记录，不作为当前验收证据。未经规范化的原始文本仅保存在本次工作区审计备份中；本文件不是逐字原始记录。

> 此页记录历史 0.15.0 输入的专用离线工具，不是 1.0.0 的当前配置。1.0.0 的服务端和客户端均使用 v1；正常升级按 offline-upgrades.md 操作。

# xscs 0.15.0 → 0.16.0 离线观察缓存准备

本页和对应专项仅以规范化的 `xscs` 项目名称描述历史输入。`1.0.0` 不提供旧项目命令别名或旧命名协议前缀的兼容读取；历史版本号和结构摘要保留，不表示历史仓库、发行物或原始记录已按新名称重新验收。当前 `1.0.0` 产品的普通会话协议仍为 `xscs-management/1`。

历史 0.16.0 的会话协议为 `xscs-management/4`，能力声明新增两个必填布尔字段 `configuration_overwrite` 和 `pending_pairing_listing`。0.15.0 保存的完整 `/3` 能力只是上次 Client Hello 的观察缓存，不能推断这两个值。`prepare-xscs-protocol` 仅失效可重建观察，让新客户端按真实能力重新声明；不会改变业务任务协议 `/3` 或持久化指纹。

只接受实际发行身份为 xscs **0.15.0 / Linux AMD64 GNU**、数据身份为 `xscs` / `0.10.1` / revision `7` / SHA-256 `0466872562dde0c06ef73e42e683801c21cc1d7be3488ca332a5e9a3d9c0518b` 的源。核验完整真实 DDL，所有非空能力观察必须符合完整旧形状；未知字段、缺字段、重复字段、错误类型、错误协议及其他结构均拒绝。没有任意 SQL、用户脚本 hook 或在线兼容入口。

## 停服、身份与计划

先停止 systemd 主服务和所有访问这些持久资源的 writer，关闭其他管理器的自动重启。工具只查询固定 `/usr/bin/systemctl`，不停止、启动、下载、切换或修改 unit。数据库必须是数据目录的直接子文件，与源配置 `database_url`（缺省 `xscs.sqlite3`）一致。配置和程序在数据目录之外；新工作目录在程序、配置及数据边界之外且此前不存在。

以真实服务 User/Group 运行，包括经 `sudo -u SERVICE_USER -g SERVICE_GROUP` 调用。数据目录由该用户拥有、mode 0700，数据库为单链接普通文件、mode 0600，计划由调用用户拥有、mode 0600。root 不可通过行政权限桥打开其他服务用户的原数据库，以免新 journal/WAL 属主错误。工具取得 common 独占维护锁及该数据库的 instance/maintenance 独占锁，复查停服与原始身份；运行实例、待恢复标记、锁冲突均拒绝。

从独立可信发行来源取得源程序 SHA-256 和完整 `release-identity --json`，不要以同一未经验证的下载附带摘要替代信任。先以服务身份执行普通 `config validate --config PATH --data-dir PATH --json`，其输出必须覆盖且只覆盖配置文件与完整数据目录。工具再次验证实际程序输出与计划的完整身份精确相等；普通源配置验证只创建临时只读验证副本，不争用独占运行锁。

计划示意，所有大写占位必须替换为实际值：

```json
{
  "format": 1,
  "service": "xscs.service",
  "source_binary": "/opt/xscs-0.15.0/bin/xscs",
  "source_binary_sha256": "TRUSTED_SOURCE_BINARY_SHA256",
  "source_identity": {
    "product": "xscs",
    "version": "0.15.0",
    "source_revision": "TRUSTED_COMPLETE_SOURCE_REVISION",
    "target": "x86_64-unknown-linux-gnu",
    "state_contract_sha256": "TRUSTED_SOURCE_STATE_CONTRACT_SHA256"
  },
  "config": "/etc/xscs/config.json",
  "data_dir": "/var/lib/xscs",
  "database": "/var/lib/xscs/xscs.sqlite3",
  "work_directory": "/var/backups/xscs/preparation-UNIQUE_ID",
  "max_backup_bytes": 10737418240
}
```

```sh
sudo -u SERVICE_USER -g SERVICE_GROUP \
  xssc prepare-xscs-protocol --plan /absolute/private/prepare.json
```

父级备份目录须允许服务用户创建 mode 0700 的新工作目录。预算覆盖完整数据目录（包括 SQLite sidecar、密钥、媒体等实际文件）和配置，不能只计算主数据库。该命令没有修改或替换源程序，所以备份覆盖本次可受影响的原配置与全部数据，不等于完整发行物切换备份。

## 写入与持久记录

在全部锁持有期间生成逐文件 SHA-256、模式及原 uid/gid 清单，复制并同步完整原组，再核验备份和原组一致。工作目录的 `data/`、`config` 和 `xscs-preparation.json` 保留，不重用已有目录。

SQLite 开启外键、防御模式、1 MiB 原生值限制、5 秒 busy timeout 及 30 秒进度期限；最多检查 10000 个非空观察，每条最多 64 KiB。严格校验全部旧观察后，单事务仅将 `capabilities_json`、`session_id`、`last_seen_at_micros`、`health_at_micros`、`sunshine_reachable` 置 NULL。`snapshot_json`、秘密、saved revision、时间戳、任务、任务状态/指纹/请求、账户和审计均保留。关闭原生 worker 后核验数据库 inode/属主与配置一致，再报告 `prepared`。

| 记录 phase | 能确认的事实 |
| --- | --- |
| `backup-complete` | 原组备份已校验，此进程尚未进入缓存写入 |
| `cache-write-intent` | 已持久化写入意图；事务可能提交，也可能未提交，必须核验 |
| `cache-invalidated` | 事务已成功提交；最终文件/完成记录检查尚未全部完成 |
| `prepared` | 事务提交、worker 关闭和最终文件/记录检查完成 |

输出稳定错误码：`XSCS_PLAN_INVALID`、`XSCS_OWNER_INVALID`、`XSCS_SOURCE_INVALID`、`XSCS_WRITER_NOT_EXCLUDED`、`XSCS_BACKUP_FAILED`、`XSCS_CACHE_STATE_UNCONFIRMED`、`XSCS_POSTCOMMIT_FAILED`。错误不会反射含秘密的配置、SQLite 值或产品 stderr。后两类明确不能假定尚未修改。

## 中断后核验与恢复

任何失败均保持停服，保留现状和工作目录；本命令不自动重启或自动回滚。普通 `inspect-upgrade` / `recover-upgrade` 仅处理 signed-upgrade journal，不能处理此准备记录。

1. 先确认 writer 仍停止，取得独占维护权；按记录逐项验证备份 `data_entries` / `config_entries` 的 SHA-256、模式和 `data_ownership` / `config_ownership`。清单须完整匹配，没有缺文件或额外业务文件；勿对未经验证的备份恢复。
2. 使用只读 SQLite 检查本次五个字段以及其余业务状态。若所有五字段已经 NULL，这是可重建观察失效已完成；核对账户、秘密、快照、任务/指纹及审计与备份保持。若所有受影响行仍等于备份，则事务未提交。SQLite 单事务不会合法产生部分提交；混合状态、源 inode 改变、未知结构或外部写入须保全现场并人工处理。
3. 只有原组备份已完整验证、无交接后的业务写入且恢复范围被明确接受时，才按清单在停服和独占锁下恢复**整组数据和配置**（主 DB 与原 sidecar 同时恢复），并恢复原 uid/gid/mode；不能只覆盖主数据库。再以 0.15.0 原程序的普通只读校验确认原状态。避免将备份中的锁文件替换到仍持有的锁 inode：保留当前已验证锁，并在恢复后重新取得锁。
4. 缓存已失效且业务保全时，继续安装独立验证的 0.16.0 完整发行物，以其普通 `config validate` 验证并启动。必须完成新 `/4` 客户端真实 Hello 与业务就绪核验；没有 Hello 时观察保持空，不伪造能力。若选择重新准备，须先完成上述核验并使用新工作目录；已有工作目录拒绝覆盖。

当前自动测试包括严格旧形状拒绝、真实 SQLite 原子性及业务保留、完整配置/数据备份、源接口夹具、摘要/身份拒绝、common/DB 锁冲突和错误阶段记录。源接口与停服夹具不代表真实 systemd 部署、真实 xscs 发行升级或设备验收。原生 Linux CI 必须运行这些测试通过，才能发行新入口。
