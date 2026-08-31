# 01. 项目定位、威胁模型与支持边界

## 1.1 一句话定位

Sarmg Upgrade 是停机运行的备份、验证、恢复和未来历史转换工具。产品只理解一个当前版本；当前开发
阶段没有历史转换 adapter，避免把试验数据格式误当长期合同。

## 1.2 为什么它高风险

工具读取数据库、配置、媒体/录像/共享树和 external key，并可能原子替换生产状态。路径替换、恶意旧
数据、磁盘满、掉电、错误产品/版本或操作者误选都可能导致数据损坏或越权。

## 1.3 信任边界

binary 和 release metadata 必须已验证；命令行产品/版本/路径只是操作者声明，工具仍要从代码 allowlist、
manifest、Schema、文件身份和 key 认证独立证明。自洽输入不自动可信。

## 1.4 两类能力

当前 backup/restore 保存产品此刻支持的状态。historical edge 是未来概念：把一个精确 source 转成一个
精确 target；当前支持矩阵全部为空，二进制没有此类命令。

## 1.5 三个结果

- immutable backup：原件未动，完整输出只在 manifest 最后落盘后发布。
- installed target：stage 经过 code-owned 验证并按 journal 安装。
- recovery evidence：中断后保留原件/来件和阶段，等待明确 commit/rollback。

## 1.6 当前产品身份

仓库、crate、binary、发行包和文档统一使用 `sarmg-upgrade`。不提供另一可执行名、命令 alias、环境变量
fallback 或旧 manifest 宽松解析。

## 1.7 明确不做

不作为 daemon/API/Web；不自动停止启动服务；不覆盖 output；不跟随不可信链接；不猜版本；不自动跨多
edge；不把 raw key 写备份；不删除 recovery 证据；不为产品 runtime 生成兼容 shim。

## 1.8 主要取舍

停机换取明确状态边界；全量 immutable backup 换取更多空间；显式 recovery action 换取避免误判；
external key 分离换取独立 Secret 运维；开发期删除历史 edge 换取更小的兼容负担。

## 1.9 本章检查

能说明为什么工具不自动选择“最近版本”、为何 manifest 自报不能替代 code allowlist、为什么错误后保留
recovery 目录比自动清理更安全。
