# Sarmg Upgrade 运维文档

## 1. 运行前检查

1. 确认主机和制品都是 Linux AMD64 GNU，并验证正式发行签名、checksum、binary SHA 和版本；保存 `support --json` 输出，且 `formal_release_target` 必须精确为 `x86_64-unknown-linux-gnu`。
2. 确认所需产品/版本/操作确实出现在 support；当前所有 `upgrade_edges` 必须为空。
3. 停止目标产品及 companion，并禁止 service manager/watchdog 自动拉起。
4. 核对数据库、数据树、输出、recovery、key file 的绝对物理路径和文件系统容量。
5. 备份输出必须不存在，父目录只允许可信操作者写；不要以 root 信任用户可替换路径。
6. 保存命令、binary SHA、时间和操作者，但不要记录 raw credentials key。

## 2. 只读发现

```bash
sarmg-upgrade support --json
sarmg-upgrade catalog --json
sarmg-upgrade inspect-manifest /backup/manifest.json
```

`inspect-manifest` 只证明 JSON 可按当前格式解析，不读取资源字节、SQLite 或 external key，不能替代
`verify-*`。`catalog` 也不等于已实现支持。

## 3. Media Backup 当前组合备份

```bash
sarmg-upgrade backup-media \
  --database /var/lib/isarmg/media-backup/db/app.db \
  --data-dir /var/lib/isarmg/media-backup/data \
  --output /srv/backup/media-backup-0.2.0-20260830

sarmg-upgrade verify-media-backup \
  --input /srv/backup/media-backup-0.2.0-20260830
```

恢复到新目标的演练：

```bash
sarmg-upgrade restore-media \
  --input /srv/backup/media-backup-0.2.0-20260830 \
  --database /srv/restore-test/media/app.db \
  --data-dir /srv/restore-test/media/data
```

生产替换必须显式增加 `--replace-existing`。数据库和 data tree 不可拆开复制。中断时保留错误报告中的
recovery 路径：

```bash
sarmg-upgrade recover-media-restore \
  --expect-version 0.2.0 \
  --input /srv/backup/media-backup-0.2.0-20260830 \
  --database /var/lib/isarmg/media-backup/db/app.db \
  --data-dir /var/lib/isarmg/media-backup/data \
  --recovery /exact/path/from/error \
  --action rollback
```

五个上下文参数都必须来自原 restore 记录，第六项 `--action` 必须由操作者明确选择；不能只凭 recovery 目录
猜测 source、target 或动作。命令会把显式 current version、backup、database、tree、recovery 与 journal 逐项
比对，并在任何 mutation 前重新取得 DB/tree 两把 non-blocking exclusive sibling lock。

## 4. Host Monitoring 当前 SQLite

```bash
sarmg-upgrade backup-sqlite \
  --product host-monitoring \
  --database /var/lib/isarmg/host-monitoring/db/host-monitoring.sqlite3 \
  --output /srv/backup/host-monitoring-0.7.0-20260830

sarmg-upgrade verify-sqlite \
  --product host-monitoring \
  --input /srv/backup/host-monitoring-0.7.0-20260830

sarmg-upgrade restore-sqlite \
  --product host-monitoring \
  --expect-version 0.7.0 \
  --input /srv/backup/host-monitoring-0.7.0-20260830 \
  --database /var/lib/isarmg/host-monitoring/db/host-monitoring.sqlite3 \
  --replace-existing
```

Host restore 中断可显式恢复：

```bash
sarmg-upgrade recover-sqlite \
  --product host-monitoring \
  --expect-version 0.7.0 \
  --recovery /exact/path/from/error \
  --action commit
```

## 5. Sunshine Manager 当前 SQLite

Sunshine 命令在 Host 参数基础上必须同时提供当前 key ID 和私有 key 文件：

```bash
sarmg-upgrade backup-sqlite \
  --product sunshine-manager \
  --database /var/lib/isarmg/sunshine-manager/db/sunshine-manager.sqlite3 \
  --output /srv/backup/sunshine-manager-0.7.0-20260830 \
  --credentials-key-id current-key-1 \
  --credentials-key-file /run/credentials/sunshine-manager.key

sarmg-upgrade verify-sqlite \
  --product sunshine-manager \
  --input /srv/backup/sunshine-manager-0.7.0-20260830 \
  --credentials-key-id current-key-1 \
  --credentials-key-file /run/credentials/sunshine-manager.key
```

key 文件内容为 Base64 编码的精确 32 bytes；文件必须为单硬链接普通文件、无 group/other 权限、读取中
身份和元数据不变。原始 key 不进入备份、manifest 或输出。工具会实际认证全部非 NULL Host `secret` 和
全部非 NULL operation `request_ciphertext`，不按 operation 完成状态过滤，也不只比较 key ID。当前
AES-256-GCM envelope 使用 12-byte nonce 与 empty AAD。

恢复也必须提供同一组 key 参数。Sunshine restore 的中断 recovery 当前未列为对外支持能力：遇到残留时
保持服务停止、保全目录和日志，禁止调用 Host recovery 或手工替换文件。

## 6. 当前禁止操作

- 不存在任何 `upgrade-*`、`verify-*-source-backup` 或 `recover-*-upgrade` 命令；
- Sentinel 和 Dufs 暂无完整当前组合备份，不能用 `backup-sqlite` 替代；
- 不对未知产品、版本、Schema、manifest 增加临时兼容；
- 不编辑 metadata/SHA 让旧库冒充当前库；
- 不直接 `cp app.db`，WAL 中可能仍有已提交数据；
- 不把 raw key 和数据备份放在同一信任域；
- 不删除或移动未完成 recovery；
- 不使用跨文件系统 copy 模拟原子安装。

## 7. 中断处置

保持产品停止并保存完整错误、binary SHA、support snapshot 和 recovery 路径。Media 还必须把原 source backup
留在 journal 绑定的规范路径和同一目录 inode；移动、替换或用内容相同的副本顶替都会失去 recovery identity。
只修复可证明的环境问题，例如空间不足或只读挂载；不要改变数据库、sidecar、stage、journal。仅对 support 明确列出的 recover 命令，
使用相同 product/version/path/binary 选择 `commit` 或 `rollback`。完成后运行相应 verify、产品 offline doctor
和最小业务 smoke，最后才恢复 service manager。

## 8. 备份保管和演练

备份目录按不可变、最小权限、静态加密、异地 3-2-1 管理。external key 单独进入 Secret 系统，分别测试
数据丢失和 key 丢失告警。每次正式发行至少演练：新目标 restore、已有目标 replace、一次中断 rollback、
一次篡改 verify 失败、错误 product/version/key 拒绝。

## 9. 正式发行

annotated `v0.2.0` 触发构建和发布两阶段：完整 Rust 门禁，暂存 source-bound binary、support/catalog、
CycloneDX SBOM、环境和 provenance；发布 job 不 checkout source，签名 `SHA256SUMS`，解包复验后发布固定
`.tar.zst` 和 outer digest。已有 tag/release/asset 不覆盖。发布验收必须确认 binary 输出没有历史 edge。

Ed25519 信任锚固定在 `release/sarmg-upgrade-release-signing-public.pem`。其 DER 编码 SHA-256 必须为
`547e3a4566e7db00725b0bb764125a5dc9152ac06942957cf406de3de2b71ef5`，stage 会把公钥和该指纹写入
source-bound package/release metadata。publish job 从 `RELEASE_SIGNING_KEY_PEM` Secret 取得私钥后，必须先
派生公钥并与上述源码公钥逐字节相等；缺少 Secret、错误私钥、公钥漂移或 metadata 指纹漂移均在产生签名
前失败。轮换密钥是新的明确发行合同：先在独立安全变更中提交新公钥、指纹、文档和负例，再原子更新
Secret；不得同时接受新旧两把 key，也不得从下载归档本身建立唯一信任。

Foundation 依赖是发布输入而不是运行时服务。发行前另行核对：

1. `sarmg-contracts` 与 `sarmg-schema-identity` 均精确为 `=0.3.0`，Git rev 精确为
   `1fe326081cfd896f05ff502e80f99504797c14c6`；
2. `Cargo.lock` 中没有第二版本，也没有 registry/path fallback 或可漂移 branch；
3. `cargo test --locked --all-targets --all-features` 覆盖 shared manifest parser、metadata column/row adapter、
   schema fingerprint 和本仓库产品级负例；
4. SBOM/provenance 同时记录两个 crate 的版本与来源 revision；
5. 从 clean checkout 离线复建后，Host/Sunshine/Media fixture 的 SHA 与 code-owned official identity 相等；
6. `support --json` 中仍不存在历史 edge，CLI 中不存在 `upgrade-*` 命令。
7. staged `RELEASE-SIGNING-PUBLIC.pem`、源码公钥、`release.json` 指纹与 Secret 私钥派生公钥四者一致。

禁止为了处理依赖不可用而复制 Foundation 源文件、放宽到 `^0.3`、改用分支 HEAD、workspace sibling、
Cargo path dependency 或本地副本，或增加旧 manifest/fingerprint fallback。依赖或合同不匹配时发行必须
停止；当前无 edge 的旧开发数据默认重建。未来稳定版本若确有迁移需求，只能使用本 `sarmg-upgrade`
仓库中已由 `support --json` 精确公开、并与 source/target 身份绑定的独立迁移入口。

## 10. 安全事件

停止工具和产品，保全 backup/recovery、binary、release、SBOM 摘要与只读日志；隔离可能泄露的数据库和
媒体树并轮换 external key。不要在公开 issue 上传生产库、key、manifest 私有路径或 recovery journal。

## 11. 平台、安装与目录约定

正式工具只支持 Linux AMD64 GNU，精确 target 是 `x86_64-unknown-linux-gnu`。它不是常驻 Server，不监听
端口，也没有 React/Vite 或其他前端。仓库没有 `config/`、`deploy/`、`clients/` 是刻意结果：当前没有
daemon 配置、systemd unit 或客户端资产。不要创建空目录来“看起来一致”，也不要把产品配置复制进本仓库。

建议把经验证的 binary 安装到只允许受信管理员更新的位置，并把实际 binary SHA-256 与 release provenance
写入变更单。运行账号需要读取源状态、在备份父目录创建私有目录，以及恢复时修改目标父目录；权限越大，
路径父级越必须由同一受信域控制。`root` 不能把用户可写父目录变安全，反而会放大替换错误的后果。

首次验收：

```bash
uname -m
ldd --version
sarmg-upgrade --version
sarmg-upgrade support --json
sarmg-upgrade catalog --json
```

必须确认 `uname -m` 为 `x86_64`，support 的 `formal_release_target` 精确匹配，并保存完整 JSON。不要仅凭文件名
推断架构；也不要在 ARM、musl 或其他 OS 上把“能启动”当正式支持。

## 12. 每次操作的变更单

| 字段 | 必填内容 | 不能记录 |
|---|---|---|
| 工具身份 | 版本、binary SHA、签名/checksum、formal target | 可变下载 URL 代替摘要 |
| 能力身份 | 完整 support JSON、product、operation、current version | catalog 推断或计划中能力 |
| 输入 | 绝对路径、父目录 owner/mode、挂载点、容量、快照标识 | raw key、生产敏感行内容 |
| 输出 | 不存在的绝对路径、保留策略、异地目标 | 已有备份相同路径 |
| 停机 | unit/process、禁止自动拉起的证据、窗口 | “应该已经停了”的口头假设 |
| external key | 非秘密 key ID、Secret 版本、独立演练记录 | Base64 key bytes、key file 内容 |
| 恢复 | replace policy、RPO/RTO、commit/rollback 决策人 | 自动选择 action 的默认脚本 |

保存退出码、stdout JSON 和 stderr，但日志同样要限制权限。stderr 若带 recovery path，该路径是恢复证据的一部分，
不能只保存最后一行而丢掉 binary、support 和命令上下文。

## 13. Current identity 核对表

| 产品 | version | revision | canonical schema SHA-256 | 当前能力 |
|---|---:|---:|---|---|
| Media Backup | `0.2.0` | 1 | `2563e6afc3fff272d02b7a5615272cc773862243bfd15aec51655abf1d9c6b1c` | composite backup/verify/restore/recover |
| Host Monitoring | `0.7.0` | 1 | `12dd1e61426b6b99df3d429b8c36ee3a5b22d1da776d98fc960b45b4f58c8e05` | SQLite backup/verify/restore/recover |
| Sunshine Manager | `0.7.0` | 1 | `a717bcd5a591e7f7cc6da5826af88ad0deab2fdc339ce4649ad84f21ea879dbc` | keyed SQLite backup/verify/restore；无 recover |

这些值用于核对 binary/code/release，不是允许手写进数据库或 manifest 的“修复参数”。工具会从实际
`sqlite_schema` 计算 fingerprint，并检查 `product_metadata` 五列/单行和 integrity/FK。任何额外对象都会
自然改变 fingerprint；没有 `_sqlx_migrations` 或其他旧表名特判。

## 14. 备份集布局与完成判定

SQLite-only 目录必须恰好是：

```text
OUTPUT/
├── database.sqlite3
└── manifest.json
```

manifest 最大 1 MiB；database resource 必须恰好为 `name=database`、`kind=sqlite`、
`path=database.sqlite3`、`files=1`，bytes/SHA/schema 与真实文件一致。extra sidecar、说明文件或手工 checksum
都会使 verify 失败；外部说明应放到 OUTPUT 之外。

Media composite 目录为：

```text
OUTPUT/
├── database.sqlite3
├── tree/
└── manifest.json
```

Media manifest 的唯一 current version 固定为 3；version 2 和其他版本直接拒绝，不双读。backup 根目录必须
恰好是上图三个 entry，任何顶层 extra/missing/type 漂移都失败。tree inventory 最多 2,000,000 项/深度 128，
manifest 最大 128 MiB；树只接受目录和普通文件，并把 tree 根 mode、非根目录 path/mode、普通文件
path/mode/size/SHA 纳入聚合摘要。`manifest.json` 出现不等于完成；必须以 backup 成功退出并随即通过
`verify-media-backup` 为准。

发布使用 private pending + directory-FD `renameat2(RENAME_NOREPLACE)`；并发者抢先创建 output 时发布失败，
绝不覆盖。普通错误由 guard 清理本次拥有的 pending；若 kill/掉电留下残余，禁止手工把它改名，应先保全
调查，再选择全新 output 重跑。

## 15. Restore 前的强制演练

生产 replace 前至少完成：

1. 从异地介质重新取得 backup，验证存储层 checksum 与 release signature；
2. 在隔离 AMD64 GNU 主机用同一 binary 运行对应 `verify-*`；
3. 恢复到完全不存在的新 DB/tree 路径，不使用 `--replace-existing`；
4. 用目标产品 current offline doctor 检查 schema 与业务不变量；
5. 启动隔离产品，完成最小读取/写入/重启 smoke；
6. 主动在 journal 不同阶段中断，分别演练 commit 与 rollback；
7. 记录实际 RTO、空间峰值、key 可用性和人工决策时间；
8. 删除隔离恢复前，确认生产路径从未被命令引用。

Media 的 DB/tree 必须同时不存在或同时存在；混合代在 mutation 前拒绝。Host/Sunshine restore 会把目标 main
及当时存在的 `-wal/-shm/-journal` 视为一个 generation。不要停服后预先删除 sidecar“清理现场”，否则可能
删除已提交数据或破坏 rollback 证据。

## 16. Recovery 决策矩阵

Media 只接受 current journal v2（最大 1 MiB），不兼容 v1。v2 把 tool/product/version/adapter/Schema/time、
source backup 规范路径+inode/path identity、manifest version/time/bytes/SHA、source tree identity、database/tree
目标及父路径 identity、同 nonce 精确推导的 stage/original、incoming/optional original 的 DB/tree 完整 inventory、
空 configuration/external requirements 和 phase 绑定起来。`recover-media-restore` 仍要求操作者显式重给
`--expect-version/--input/--database/--data-dir/--recovery/--action` 六项，不能只相信 journal 自报。

| 观察事实 | 合法下一步 | 原因 |
|---|---|---|
| 命令未报告 recovery，目标未变 | 修复输入/权限，使用新 output 或重新 verify | 无 durable journal 授权继续 |
| Media recovery，目标名缺失且 incoming 存在 | 保持停服，审阅 journal 后显式 commit/rollback | 可能处于 originals-preserved |
| Media 目标 current 可验证且 originals 存在 | 通常 commit cleanup，但仍显式决定 | 目标存在不证明 parent sync/cleanup 完成 |
| Host destination 为 incoming hash | 用 `recover-sqlite` 显式 commit 或 rollback | 工具可证明 incoming 与 originals |
| Host destination/recovery 出现不匹配文件 | 停止并复制全部证据 | 自动动作可能销毁唯一 generation |
| Sunshine restore 残留 | 停止、保全、进入人工事件 | support 不声明 recover；Host 命令缺 key 认证 |
| journal 被编辑或损坏 | 保全原件/快照并进入事件响应 | journal 失去授权作用，不能重建“合理”JSON |

commit/rollback 不是“成功/失败”的别名。commit 是证明 incoming 后完成安装。SQLite rollback 会把可能已
安装的 incoming 保存为 `abandoned-new` 再恢复 original；Media rollback 只接受 journal 精确绑定的 incoming，
暂移回 stage、恢复并验证 original，随后在成功 cleanup 中删除该 stage。两者都要求相同
product/version/path/binary 上下文和 exclusive lock；journal 只校验 tool version，binary SHA 仍由变更单证明。

## 17. 保管、保留与销毁

- output 发布后视为 immutable；扫描、对象存储上传或加密封装不能修改目录内部；
- 数据和 Sunshine key 分离信任域、审批与丢失告警；
- 至少一份离线/不可变副本，并周期性从该副本恢复，而非只验证在线副本；
- retention 按 RPO 与合规确定；删除前确认它不是 recovery/审计的唯一 source；
- recovery 不服从普通 retention；Media recovery 绑定的 source backup 同样进入 hold，只有 commit/rollback、verify/doctor/smoke 和结案后才清理；
- Secret、DB、媒体树和日志分别执行销毁政策；删除 manifest 不等于销毁数据。

## 18. Current-only 事件边界

遇到非 current 数据，标准处置是重新部署并从业务源重新导入。本 binary 的 `upgrade_edges` 仍全空，CLI
仍无 `upgrade-*`，所以当前不得手写 SQL、改 metadata 或用 current restore 冒充迁移。未来产品稳定后若
确有不可重建数据和长期 edge 需求，只能在本 `sarmg-upgrade` 仓库以独立 adapter、source/target fixture、
CLI、故障矩阵、运维文档与 release 原子加入；不得把旧 parser、serde alias、“先新后旧”fallback 或任何
历史兼容分支合并进产品 runtime 或现有 current adapter。

保留的通用代码只能描述为 future-safe backup/verify/stage/journal/recover 原语，不是已支持历史迁移的引擎：
没有 graph search、source adapter、转换 SQL、target builder 或升级命令。运维材料不得用“支持升级”简称
当前 current-state 备份/恢复能力。
