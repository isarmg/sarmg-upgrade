# 09. 正式发行、安全与生产运维

## 9.1 发行信任

从干净 annotated tag 构建 source-bound binary，运行完整门禁，暂存 capability catalog、release metadata、
CycloneDX SBOM、环境和 provenance。publish job 不 checkout source，以 Ed25519 签名 checksum。

## 9.2 操作者验签

在触碰生产前，用独立可信渠道得到 public key，验证 `SHA256SUMS` 签名、outer digest、archive checksum，
解包后再次核对 binary identity/support/catalog。不要直接执行下载目录里未验制品。

## 9.3 变更窗口

记录服务/companion 停机与 watchdog 禁用、路径/uid/gid、产品/from/to、key ID、空间/inode/文件系统、命令、
binary SHA、backup destination、commit/rollback 判断点。记录不含 raw key。

## 9.4 执行原则

先 backup/verify，再 upgrade/restore；持续监控空间与日志；错误立即停止，不追加手工动作。工具不自动启动
服务，完成后先 offline doctor，再启服务和业务 smoke。

## 9.5 备份保管

不可变、最小权限、静态加密、异地 3-2-1；external key 在独立 Secret 系统。记录 retention、访问审计与
可验证销毁。定期从真实介质恢复，不只运行 checksum。

## 9.6 Recovery 事件

发现 recovery path 时冻结服务，保全 binary/log/journal/目录身份，修复环境后明确 commit/rollback。交接
时必须传递全部参数和当前阶段，不能让下一班“从头再跑”。

## 9.7 安全事件

停止工具和产品，隔离可能泄露的数据库、配置、媒体/录像/共享树与 key，保全 release/SBOM/provenance/
日志摘要，轮换 external key 并按 adapter 重新加密。公开报告不包含私有路径或 manifest 敏感内容。

## 9.8 监控发行

关注 workflow 权限/action pin、异常 tag/release、签名 key、制品覆盖尝试和支持目录漂移。已发布 asset
不得覆盖；问题使用新版本修复。

## 9.9 完成条件

backup 已 verify；target 通过工具与产品 doctor；服务 smoke 成功；无未解释 recovery/pending stage；
变更记录含所有 Hash 和判断；原备份/key 的保管策略已确认。
