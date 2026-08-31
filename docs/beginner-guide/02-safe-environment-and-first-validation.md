# 02. 安全实验环境与第一次验证

## 2.1 不从生产开始

第一次练习使用仓库 fixture 或备份副本，路径位于新建私有临时根，服务不运行，external key 仅为实验
随机值。不要用 root 指向用户可替换目录，也不要拿生产 database/recordings 做测试。

建议把第一次演练分成三个互不复用的私有根：source、backup、restore-target。父目录由当前操作者拥有且不允许
group/other 写入；每轮演练创建全新 output，绝不把失败目录“清空后重用”。这样能看清 no-clobber、stage
和 recovery 的真实行为，也避免把测试结果写回 fixture。

```text
private-lab/
├── source/          # fixture 的工作副本；不是仓库原件
├── backups/         # 每次使用不同且原先不存在的子目录
├── targets/         # 新目标或明确 replace 演练目标
├── keys/            # 仅 Sunshine 实验 key，权限 0600
└── evidence/        # 保存命令、退出码、support 与摘要，不保存 raw key
```

不要照抄以上相对路径直接运行正式命令；Media adapter 要求关键路径为绝对路径，生产变更单也应固定物理挂载
与父目录身份。

## 2.2 工具链基线

```bash
cargo +1.98.0 check --locked --all-targets --all-features
cargo +1.98.0 test --locked
cargo +1.98.0 run -- support --json
cargo +1.98.0 run -- catalog --json
```

`support` 是 binary 实现能力，`catalog` 是产品资源知识；产品出现在 catalog 不等于有命令。

以上 Rust 命令是开发者门禁示例，不是生产操作的先决命令。正式环境应执行已经验签的 release binary，
而不是在生产主机 `cargo run`。首次学习可额外保存：

```bash
rustc --version
cargo --version
cargo +1.98.0 run -- --version
cargo +1.98.0 run -- --help
```

预期 `--help` 只有 current-state 命令；若出现 `upgrade-*`、`from-version` 或 `to-version`，说明你执行的不是
本文描述的当前 binary，应立即停止。`support --json` 还必须报告
`formal_release_target=x86_64-unknown-linux-gnu`，且六个产品的 `upgrade_edges` 全为空。

## 2.3 建立实验记录

每个命令至少记录以下非秘密事实：

| 字段 | 用途 |
|---|---|
| binary 版本与 SHA-256 | 保证后续 recovery 使用同一实现 |
| 完整 `support --json` | 固定当次真实能力 |
| product、operation、expect-version | 防止事后混淆命令族 |
| source/output/target 绝对路径 | 复核没有指向同一目录或错误挂载 |
| 父目录 owner/mode 与文件系统 | 分析权限、替换与 `EXDEV` |
| 开始/结束时间、退出码、stderr | 识别停在哪个阶段 |
| key ID/Secret 版本 | Sunshine 审计；不得记录 key bytes |

shell history、CI log 和截图都可能成为泄露通道。Sunshine key 只通过私有 key file 提供，不放环境变量、
命令参数值、README 示例或粘贴板记录中。

## 2.4 第一次只读检查

选一个仓库 fixture backup，运行对应 `verify-*`；再用 `inspect-manifest` 查看严格解析结果。inspect 不读
资源，不能作为 verification 的替代。

`inspect-manifest PATH` 仅适用于 Foundation SQLite manifest，也就是 Host/Sunshine 的 SQLite-only 清单格式；
它不会解析 Media composite manifest。对 Media 必须使用 `verify-media-backup --input DIR`。只读不等于输入
可信：manifest parser 仍会拒绝 unknown field、坏路径、重复资源和非法 identity。

建议做两个负例：在备份副本中修改一个数据库字节，确认 verify 失败；再加入一个额外文件，确认严格目录
布局拒绝。不要在唯一备份上制造负例。

## 2.5 第一次 Media 备份

把 source 复制到临时产品布局，运行对应 current backup 到一个不存在的 output。成功后记录 manifest、
文件 mode/Hash 和目录同步点；再次使用同 output 应 no-clobber 失败。

Media 是最适合理解组合状态的示例。它要求 SQLite 和 data tree 同时进入一份 backup：

```bash
sarmg-upgrade backup-media \
  --database /absolute/lab/source/media.sqlite3 \
  --data-dir /absolute/lab/source/media \
  --output /absolute/lab/backups/media-run-001

sarmg-upgrade verify-media-backup \
  --input /absolute/lab/backups/media-run-001
```

成功目录必须恰好是 `database.sqlite3`、`tree/`、`manifest.json` 三项，顶层 extra entry 也会失败。Media
manifest 只接受 current version 3，不读取 version 2。tree 只接受目录和普通文件；symlink、FIFO、socket、
device 和 hardlink 是拒绝项。v3 合同记录 tree 根 mode、非根目录 path/mode、文件 path/mode/size/SHA 和
聚合 inventory，不承诺 xattr、ACL 或 sparse 布局保真。

## 2.6 第一次 SQLite-only 备份

Host 不需要 key；Sunshine 必须同时提供 key ID 与 key file：

```bash
sarmg-upgrade backup-sqlite \
  --product host-monitoring \
  --database /absolute/lab/source/host.sqlite3 \
  --output /absolute/lab/backups/host-run-001

sarmg-upgrade verify-sqlite \
  --product host-monitoring \
  --input /absolute/lab/backups/host-run-001
```

SQLite-only backup 目录必须恰好包含 `database.sqlite3` 与 `manifest.json`。不要放 README、额外 checksum 或
sidecar；这些都是严格验证的 extra entry。Sunshine 的实验 key file 必须是权限私有、单硬链接的普通文件，
内容 Base64 解码后精确为 32 bytes；key ID 为 1..64 个 `[A-Za-z0-9_-]` 字符。

## 2.7 第一次恢复

目标使用全新路径。先 verify backup，再 restore，最后用工具验证和目标产品 offline doctor 检查。只看
exit code 或数据库文件存在不足以证明可运行。

第一次必须恢复到完全不存在的新目标，先不要使用 `--replace-existing`。Media 的 database 与 data-dir 必须
同时不存在；一个存在、另一个不存在代表混合代风险，会在 mutation 前失败。Host/Sunshine 还要显式提供
`--expect-version`，它不是版本转换请求，只是操作者对 current 目标的第二次确认。

恢复完成后的验证分三层：工具 verify 证明输入 backup；工具在安装前后证明 current identity；目标产品的
offline doctor 与隔离启动 smoke 证明业务可运行。三层都完成才能把演练标记成功。

## 2.8 第一次 replace 与中断

使用测试故障注入在 mutation 阶段中断，保留 recovery directory。分别在独立副本上执行 commit 与
rollback，检查 journal、preserved original、incoming stage 和最终 fsync/cleanup。

`--replace-existing` 不是覆盖开关，而是允许工具进入“验证现有 current 目标 → 建 incoming → 写 durable
journal → preserve original → install → verify”的状态机。故障注入只能在受控测试中进行。正常运维若命令
报告 recovery path，应立即冻结服务并保全现场，不能为了练习强行重跑。

Media 与 Host 可以分别用对应 recover 命令续接；Sunshine current support 不公开 recover。不要因为底层
SQLite restore 代码相似就把 Host recover 用于 Sunshine。

## 2.9 预期失败也是成功标准的一部分

安全工具必须在错误输入上清晰失败。建议逐一构造副本验证：

| 负例 | 预期 |
|---|---|
| 重用同一 output | no-clobber 拒绝，原 backup 不变 |
| 修改 manifest product/version/SHA | strict parser 或 code allowlist 拒绝 |
| 修改 database 一个字节 | size/SHA、SQLite 或 Schema 验证拒绝 |
| Media tree 多一个文件/少一个文件 | inventory 不匹配 |
| Media tree 放 symlink/FIFO | strict tree 拒绝 |
| Media tree 根 chmod 或 backup 根加入说明文件 | v3 root mode / exact top-level 拒绝 |
| Sunshine 少一个 key 参数或 key 错误 | mutation 前拒绝 |
| `backup-sqlite --product dufs-ram` | 产品边界拒绝 |
| 目标在另一文件系统导致非原子切换 | mutation 前或 rename 时明确拒绝，无 copy fallback |

“让命令继续跑”不是调试目标；每个失败后都应检查 source、既有 backup 和目标未被意外改变。

## 2.10 成功标准

- 所有路径/身份显式；
- output no-clobber；
- verify 会发现单字节篡改；
- restore 不触碰源 backup；
- 中断不产生无 journal 的混合代；
- 产品 doctor 接受恢复状态；
- raw external key 不出现在 JSON/日志/manifest。

## 2.11 常见错误

空间预算不足、output 已存在、锁被服务持有、数据库 sidecar 遗漏、key file mode 不安全、目标跨文件系统、
路径包含链接或 manifest 超预算，都应显式失败，不能用放宽检查继续。

另一个常见误区是把 catalog 输出当命令目录。Sentinel 与 Dufs 会出现在 catalog，因为未来完整 adapter 必须
知道它们的组合资源；它们的 `current_state` 在 support 中仍为空。若实验步骤需要为这两个产品运行 backup，
说明步骤本身越过了当前边界，应停止而不是寻找 generic 绕过方式。

## 2.12 本章练习与验收

1. 解释为什么生产主机不应 `cargo run`。
2. 从同一个 binary 保存 support/catalog，并指出两者对 Sentinel 的答案为什么不同。
3. 完成一次 Media 或 Host 的新 output backup/verify，再证明重用 output 会失败且旧内容不变。
4. 在副本上完成一次单字节篡改负例。
5. 写出 restore 后仍需产品 doctor/smoke 的原因。
6. 说明 Sunshine restore 中断时为什么不能调用 `recover-sqlite --product host-monitoring`。
