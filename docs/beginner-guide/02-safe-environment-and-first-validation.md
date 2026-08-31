# 02. 安全实验环境与第一次验证

## 2.1 不从生产开始

第一次练习使用仓库 fixture 或备份副本，路径位于新建私有临时根，服务不运行，external key 仅为实验
随机值。不要用 root 指向用户可替换目录，也不要拿生产 database/recordings 做测试。

## 2.2 工具链基线

```bash
cargo +1.98.0 check --locked --all-targets --all-features
cargo +1.98.0 test --locked
cargo +1.98.0 run -- support --json
cargo +1.98.0 run -- catalog --json
```

`support` 是 binary 实现能力，`catalog` 是产品资源知识；产品出现在 catalog 不等于有命令。

## 2.3 第一次只读检查

选一个仓库 fixture backup，运行对应 `verify-*`；再用 `inspect-manifest` 查看严格解析结果。inspect 不读
资源，不能作为 verification 的替代。

## 2.4 第一次备份

把 source 复制到临时产品布局，运行对应 current backup 到一个不存在的 output。成功后记录 manifest、
文件 mode/Hash 和目录同步点；再次使用同 output 应 no-clobber 失败。

## 2.5 第一次恢复

目标使用全新路径。先 verify backup，再 restore，最后用工具验证和目标产品 offline doctor 检查。只看
exit code 或数据库文件存在不足以证明可运行。

## 2.6 第一次中断

使用测试故障注入在 mutation 阶段中断，保留 recovery directory。分别在独立副本上执行 commit 与
rollback，检查 journal、preserved original、incoming stage 和最终 fsync/cleanup。

## 2.7 成功标准

- 所有路径/身份显式；
- output no-clobber；
- verify 会发现单字节篡改；
- restore 不触碰源 backup；
- 中断不产生无 journal 的混合代；
- 产品 doctor 接受恢复状态；
- raw external key 不出现在 JSON/日志/manifest。

## 2.8 常见错误

空间预算不足、output 已存在、锁被服务持有、数据库 sidecar 遗漏、key file mode 不安全、目标跨文件系统、
路径包含链接或 manifest 超预算，都应显式失败，不能用放宽检查继续。
