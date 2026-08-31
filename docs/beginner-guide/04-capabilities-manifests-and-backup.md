# 04. 能力目录、Manifest 与备份流程

## 4.1 `support --json`

输出当前 binary 真正实现的 command、产品、版本/edge 和所需能力。自动化在执行前保存此快照，并拒绝
不存在的能力；不要从文档标题猜命令。

## 4.2 `catalog --json`

catalog 描述产品的当前状态版本、Schema、数据库、树、config、companion 和 external key 要求。它可
包含尚无 adapter 的产品，因此不是执行授权。

## 4.3 Manifest 作用

manifest 把 backup 身份、工具版本、产品/版本、资源、文件 mode/size/Hash、tree aggregate、预算和非秘密
external requirement 固化。它不包含 raw key，也不单凭自报内容获得信任。

## 4.4 严格解析

拒绝 unknown fields、重复 key、非法数值、非规范路径、绝对/父穿越、超长集合和不受支持算法/version。
解析后仍要按 code allowlist 比对产品合同。

## 4.5 通用备份时序

```text
validate args -> acquire canonical locks -> prove current identity/key
 -> create private pending output -> snapshot all resources
 -> compute inventory/Hash -> verify copy -> manifest last
 -> fsync -> rename to requested output -> fsync parent
```

## 4.6 备份期间的源

online backup 只有产品合同明确允许 maintenance shared 时才可使用；组合资源通常需要停止应用/companion
并取得更多排他锁。工具不自动停止服务。

## 4.7 Verification

`verify-*` 重新读取所有资源、mode/Hash/tree、SQLite integrity/FK/Schema/metadata，并在需要时用 external
key 认证全部密文。只校验 manifest checksum 不够。

## 4.8 空间预算

运行前估算 source logical/physical bytes、pending copy、target stage、preserved original、WAL 和 recovery。
tree budgets 是本次授权并写入合同，不使用“无限”值绕过。

## 4.9 失败清理

mutation 前的 private pending 可按工具证明安全后清理；已发布 output 不覆盖；任何涉及原目标 mutation 的
失败保留 recovery evidence。清理策略不能只看名字匹配。
