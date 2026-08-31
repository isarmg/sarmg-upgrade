# Sarmg Upgrade 运维文档

## 1. 运行前检查

1. 验证正式发行签名、checksum、binary SHA 和版本；保存 `support --json` 输出。
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
  --recovery /exact/path/from/error \
  --action rollback
```

## 4. Host Monitoring 当前 SQLite

```bash
sarmg-upgrade backup-sqlite \
  --product host-monitoring \
  --database /var/lib/isarmg/host-monitoring/db/app.db \
  --output /srv/backup/host-monitoring-0.7.0-20260830

sarmg-upgrade verify-sqlite \
  --product host-monitoring \
  --input /srv/backup/host-monitoring-0.7.0-20260830

sarmg-upgrade restore-sqlite \
  --product host-monitoring \
  --expect-version 0.7.0 \
  --input /srv/backup/host-monitoring-0.7.0-20260830 \
  --database /var/lib/isarmg/host-monitoring/db/app.db \
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
  --database /var/lib/isarmg/sunshine-manager/db/app.db \
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
身份和元数据不变。原始 key 不进入备份、manifest 或输出。工具会实际认证 Host credential 和未完成
operation 密文，不只比较 key ID。

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

保持产品停止并保存完整错误、binary SHA、support snapshot 和 recovery 路径。只修复可证明的环境问题，
例如空间不足或只读挂载；不要改变数据库、sidecar、stage、journal。仅对 support 明确列出的 recover 命令，
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

## 10. 安全事件

停止工具和产品，保全 backup/recovery、binary、release、SBOM 摘要与只读日志；隔离可能泄露的数据库和
媒体树并轮换 external key。不要在公开 issue 上传生产库、key、manifest 私有路径或 recovery journal。
