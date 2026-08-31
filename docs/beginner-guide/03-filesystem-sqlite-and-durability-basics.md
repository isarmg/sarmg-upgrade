# 03. 文件系统、SQLite 与持久性基础

## 3.1 路径字符串不是身份

攻击者或并发进程可在检查后把路径替换成 symlink、挂载点或另一 inode。工具应从可信父目录锚定、拒绝
链接/特殊文件、验证 owner/mode/link count，并在关键阶段复核物理身份。

例如 `/srv/state/app.db` 在参数校验时和真正打开时可能已经不是同一个 inode。只调用
`canonicalize()` 或先 `exists()` 再 `open()` 都留有 TOCTOU 窗口。SQLite-only 路径层主要位于
`src/sqlite.rs::SecureDirectory`、`DatabaseLocation` 与 secure resolve flags；恢复代码继续用受信父目录的
dirfd 操作固定名称。root 权限不会消除路径竞态，只会扩大错误目标的影响。

需要分别验证：路径必须绝对；父目录的控制域可信；最终对象是普通文件/目录；不得跟随 symlink；单文件
Secret 和备份资源需满足 link-count 约束；关键打开前后 metadata/身份不变。

## 3.2 SQLite generation

一个 WAL/rollback 模式数据库的状态代不仅是 main 文件，还包括当时存在并属于同一代的 `-wal`、`-shm`
或 journal。已提交数据可能只在 WAL；普通 `cp app.db` 会产生逻辑旧快照。

本工具当前备份 live SQLite 时使用 SQLite online backup API 生成独立一致的 `database.sqlite3`，随后对输出
运行只读完整性、foreign-key、metadata 与 schema 验证。恢复目标已有 main file 时，`-wal`、`-shm`、
`-journal` 则作为原 generation 的伴随项一起保全，避免新 main file 被旧 sidecar 污染。

两个场景不能混淆：backup 输出只发布一个规范化数据库文件；restore 的 preserved original 要忠实保留
目标当时存在的 main 与允许的 sidecar，供 rollback 恢复原始字节。

## 3.3 先复制原始字节

未来历史 adapter 必须在 exclusive maintenance lock 下先复制 main 与 sidecar，不用 SQLite 打开原件。
当前仓库没有历史 adapter；这里记录的是未来准入要求，而不是可调用能力。

这条未来规则与当前 online backup 并不矛盾：历史 source 可能需要先保留不被新 SQLite/driver 解释的原始
generation，current backup 则只接受 binary 已知的当前 Schema 并生成一致 snapshot。当前源码没有实现前者，
不能从本段推导出任何 source-backup 命令。

## 3.4 文件与目录持久性

写文件后要同步文件，原子 rename/交换后要同步父目录。只调用 `rename` 不保证掉电后目录项持久。manifest
最后写并同步，完整 manifest 是 backup 发布完成的标志。

一个简化的掉电模型如下：

```text
write bytes
  -> fsync(file)       # 文件内容持久
  -> rename entry      # 名称切换可能原子，但目录项未必持久
  -> fsync(parent dir) # 名称/删除/rename 的目录事实持久
```

对 backup，先同步资源和 manifest，再把 private pending no-clobber rename 到请求 output，并同步 output 父目录。
对 restore，journal 每次 phase 更新也需要持久化；否则内存已进入下一阶段而磁盘 journal 仍停在旧阶段，崩溃
恢复就无法安全判断。

## 3.5 Same-filesystem stage

恢复 stage 与目标位于同一文件系统，才能依赖原子 rename/交换。跨设备复制不是同一提交语义，工具应在
mutation 前拒绝。

同文件系统不代表空间足够。replace 流程峰值可能同时存在 incoming、installed、preserved original、SQLite
sidecar、journal 和源 backup。空间预算应在操作前针对目标挂载点计算，并包含 inode；不要指望运行中删除
原件释放空间，因为原件是 rollback 证据。

## 3.6 No-clobber

output、stage、目标新文件和 journal 使用 create-new 或等价排他机制。`exists()` 后再创建有竞态；绝不
为方便递归删除一个“不像完成”的现有目录。

no-clobber 同时保护三类对象：用户指定 backup output、内部 pending/recovery 名称、已有 target。用户若确实
要替换 current target，必须显式 `--replace-existing`，但工具仍先验证目标、保全原代并写 journal，而非
直接 truncate/overwrite。

## 3.7 Tree inventory

Media 当前组合目录只接受目录和普通文件。唯一 current manifest version 3 记录 tree 根 mode、非根目录
规范 path/mode、普通文件 path/mode/size/SHA，再把这些字段纳入聚合 inventory；最多 2,000,000 entries、
深度 128，manifest 最大 128 MiB。symlink、hardlink 和特殊文件直接拒绝，而不是跟随或归档；backup 顶层
也必须恰好是 database、tree、manifest 三项。

当前实现拒绝 hardlink，并不承诺保留 xattr、ACL、稀疏 extent、birth time、owner/group 或文件系统专属 flag。
如果业务将其中任一语义纳入持久状态，必须先升级产品资源合同、manifest、复制/验证实现、预算、负例与
恢复测试；不能因普通文件内容相同就声称已支持。Sentinel recordings 与 Dufs shared root 尚无 current
adapter，更不存在默认继承 Media tree 语义。

tree 路径必须相对于可信根且只由 normal components 组成。inventory 既用于发现 missing/extra/tamper，也
用于在复制完成后分别读取 destination 与当时的 source，并要求两份完整 inventory 相等；当前不是“复制前、
复制后”两次 source 快照，因此只能发现落在复制结果与随后 source 读取之间可观察到的差异。inventory 不替代
源物理路径 identity，也不证明业务数据库对树的引用全部有效，后者属于产品专用验证。

## 3.8 锁

maintenance、runtime、companion/config/shared-root 按产品规范顺序取得。锁文件路径本身也要防替换。工具
不能假设“服务已 stop”就不需要锁，watchdog 或误操作仍可能并发启动。

当前 Host/Sunshine/Media adapter 会使用产品 maintenance lock；工具本身不会调用 systemctl，也不会禁用
自动拉起。运维顺序应是：先由操作者停止服务和 companion、屏蔽 watchdog/定时任务，再由工具取得合同
规定的锁。锁失败是安全拒绝，不应通过删除 lock file 或修改权限绕过。

锁的 shared/exclusive 语义也有目的：current backup 在产品合同允许时可持 shared lock 并使用 SQLite online
snapshot；restore/recover 会改变目标，必须 exclusive。将 shared 放宽为“无锁”或把 exclusive 改为 shared
都会破坏 generation 边界。

## 3.9 故障思考

每次写代码都问：掉电在此行后，磁盘上有哪些已同步事实？恢复者能否仅凭 journal 判断？原件是否仍
完整？若答案依赖内存 bool，设计还不够持久。

建议对每个持久阶段写四列表：操作前已持久事实、即将修改对象、操作后新增事实、崩溃后的唯一合法动作。
例如第一次 preserve rename 前，journal 必须已经描述 original/incoming 与 `prepared`；rename 后但 phase 更新
前，recover 仍应能检查磁盘身份并安全续接，而不是假定 rename 没发生。

## 3.10 文件系统能力不是跨平台承诺

正式 target 是 Linux AMD64 GNU。代码使用 Linux 文件系统语义和防护，不声明 NFS、FUSE、对象存储挂载、
overlay 或不实现可靠 fsync/rename/locking 的远程文件系统都安全。生产前必须在实际文件系统做断电与锁
演练；仅因 API 返回成功不能证明底层持久保证符合预期。

## 3.11 Schema identity 是数据库内容之外的边界

工具读取 `product_metadata` 的唯一五列/单行，按 Foundation 的 canonical query 获取纳入范围的
`sqlite_schema` rows，并对每个 `type/name/tbl_name/sql` UTF-8 字段使用 unsigned 64-bit big-endian 长度
framing 后计算 SHA-256。这样能避免拼接歧义和 driver 遍历顺序差异。

identity 验证同时包含：application slug、application version、schema revision、声明的 schema SHA 和实际
重算 SHA。新增一个 index/trigger/table、删除对象或改变 DDL 都会自然 mismatch；当前没有
`_sqlx_migrations` 或其他旧表名特判，也不会先试旧算法再试新算法。

## 3.12 本章检查

读者应能解释：为什么 live WAL 数据库不能只复制 main file；为什么 rename 后仍要同步父目录；为什么
`--replace-existing` 不能直接覆盖；Media 当前 tree 明确保存和明确不保存哪些语义；为什么 catalog 中的
Sentinel/Dufs 目录资源不会自动获得 Media adapter；以及 extra SQLite object 为什么无需专门列黑名单也会
被 current schema fingerprint 拒绝。
