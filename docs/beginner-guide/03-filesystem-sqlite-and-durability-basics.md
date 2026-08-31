# 03. 文件系统、SQLite 与持久性基础

## 3.1 路径字符串不是身份

攻击者或并发进程可在检查后把路径替换成 symlink、挂载点或另一 inode。工具应从可信父目录锚定、拒绝
链接/特殊文件、验证 owner/mode/link count，并在关键阶段复核物理身份。

## 3.2 SQLite generation

一个 WAL/rollback 模式数据库的状态代不仅是 main 文件，还包括当时存在并属于同一代的 `-wal`、`-shm`
或 journal。已提交数据可能只在 WAL；普通 `cp app.db` 会产生逻辑旧快照。

## 3.3 先复制原始字节

未来历史 adapter 必须在 exclusive maintenance lock 下先复制 main 与 sidecar，不用 SQLite 打开原件。
当前仓库没有历史 adapter；这里记录的是未来准入要求，而不是可调用能力。

## 3.4 文件与目录持久性

写文件后要同步文件，原子 rename/交换后要同步父目录。只调用 `rename` 不保证掉电后目录项持久。manifest
最后写并同步，完整 manifest 是 backup 发布完成的标志。

## 3.5 Same-filesystem stage

恢复 stage 与目标位于同一文件系统，才能依赖原子 rename/交换。跨设备复制不是同一提交语义，工具应在
mutation 前拒绝。

## 3.6 No-clobber

output、stage、目标新文件和 journal 使用 create-new 或等价排他机制。`exists()` 后再创建有竞态；绝不
为方便递归删除一个“不像完成”的现有目录。

## 3.7 Tree inventory

组合产品目录需记录每个 entry 的相对路径、type、mode、size/Hash、hardlink 关系、symlink/xattr/sparse
语义及聚合 identity，并受 entry/depth/logical/backup bytes/per-directory budgets 限制。

## 3.8 锁

maintenance、runtime、companion/config/shared-root 按产品规范顺序取得。锁文件路径本身也要防替换。工具
不能假设“服务已 stop”就不需要锁，watchdog 或误操作仍可能并发启动。

## 3.9 故障思考

每次写代码都问：掉电在此行后，磁盘上有哪些已同步事实？恢复者能否仅凭 journal 判断？原件是否仍
完整？若答案依赖内存 bool，设计还不够持久。
