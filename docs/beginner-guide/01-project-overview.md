# 01. 项目定位、威胁模型与支持边界

## 1.1 一句话定位

Sarmg Upgrade 是停机运行的 current-state 备份、验证和恢复工具。仓库为稳定版本之后可能出现的历史
转换保留准入规则与少量数据模型，但当前二进制没有历史 source parser、转换 SQL、adapter registry、
edge 搜索或 `upgrade-*` 命令，因此不能称为已经实现的升级引擎。

业务产品只创建并接受自身唯一 current 格式。开发期数据默认重建；当前没有已支持 edge 时，不可重建的
旧数据也只能保全并停止。未来产品版本稳定后，固定 source/target 的精确 edge 只在本 `sarmg-upgrade`
仓库中以独立 adapter、fixture、CLI 与 release 完整准入，不把兼容分支带回业务产品或 current adapter。

## 1.2 为什么它高风险

工具读取数据库、配置、媒体/录像/共享树和 external key，并可能原子替换生产状态。路径替换、恶意旧
数据、磁盘满、掉电、错误产品/版本或操作者误选都可能导致数据损坏或越权。

风险可按首次产生不可逆影响的位置理解：

| 阶段 | 主要输入 | 典型风险 | 代码必须提供的证明 |
|---|---|---|---|
| 能力选择 | binary、product、version | 把计划中功能当成已实现 | `support_matrix()` 的精确 allowlist |
| 备份读取 | live SQLite、数据树、key | WAL 漏页、路径替换、Secret 泄露 | online backup、锁、no-follow、key 私有读取 |
| 备份发布 | pending output | 半成品被当成完成、覆盖已有证据 | manifest 最后写、fsync、no-clobber rename |
| 恢复准备 | backup、目标、stage | 篡改输入或跨文件系统切换 | 全量 verify、同文件系统 stage、目标身份检查 |
| 首次 mutation | original/incoming | 崩溃形成混合代 | 先持久化 journal，再 preserve/install |
| 中断恢复 | journal、保全原件 | 自动猜错并删除唯一完整代 | 显式 `commit`/`rollback` 和重新验证 |

## 1.3 信任边界

binary 和 release metadata 必须已验证；命令行产品/版本/路径只是操作者声明，工具仍要从代码 allowlist、
manifest、Schema、文件身份和 key 认证独立证明。自洽输入不自动可信。

不同输入的信任级别不能混用：

- `support --json` 来自正在执行的 binary，说明该 binary 实际公开的操作；它是选择命令的唯一机器事实。
- `catalog --json` 说明六个产品理论上由哪些持久资源组成，不代表 adapter 已实现。
- manifest 是不可信输入。即使字段彼此自洽，也必须重算文件 SHA、SQLite Schema identity、tree inventory
  和 external key 认证结果。
- CLI 参数是操作者意图，不是证明。`--product host-monitoring` 不能把任意 SQLite 变成 Host 数据库。
- external key 文件是独立 Secret 输入。key ID、摘要和算法可进 manifest，原始 key bytes 不得进入备份、
  JSON、Debug 或日志。
- 文档和变更单用于解释与审计，不能替代当次 binary 的 support 输出和实际 verify。

## 1.4 两类能力

当前 backup/restore 保存产品此刻支持的状态。historical edge 是未来概念：把一个精确 source 转成一个
精确 target；当前支持矩阵全部为空，二进制没有此类命令。

当前能力与未来概念应始终用不同词汇：

| 名称 | 当前是否存在 | 含义 |
|---|---|---|
| current backup | 是，限 Media/Host/Sunshine | 保存 binary 明确允许的当前状态 |
| current verify | 是，限 Media/Host/Sunshine | 只读验证备份的全部已声明资源 |
| current restore | 是，限 Media/Host/Sunshine | 把已验证 current backup 安装到目标 |
| current recover | 仅 Media 与 Host | 延续已开始且有 durable journal 的 restore |
| historical edge | 否 | 未来某个精确旧版本到精确 current 版本的转换 |
| migration engine | 否 | 当前没有 parser、graph、转换器和执行命令闭包 |

“有通用 stage/journal 代码”不等于“支持升级”。这些原语只减少将来实现时的重复工作，不产生任何历史
输入兼容承诺。

## 1.5 三个结果

- immutable backup：原件未动，完整输出只在 manifest 最后落盘后发布。
- installed target：stage 经过 code-owned 验证并按 journal 安装。
- recovery evidence：中断后保留原件/来件和阶段，等待明确 commit/rollback。

三种结果不能互相代替。一个有 `manifest.json` 的目录仍可能是被篡改或不完整的 backup，必须通过对应
`verify-*`；一个目标文件存在也不能证明 restore 已提交，因为目录项可能尚未同步；一个 recovery 目录
存在则说明流程需要续接或事件处置，不能把它当临时目录删除。

## 1.6 当前产品身份

仓库、crate、binary、发行包和文档统一使用 `sarmg-upgrade`。不提供另一可执行名、命令 alias、环境变量
fallback 或旧 manifest 宽松解析。

正式发行平台同样只有一个：Linux AMD64 GNU，target 精确为 `x86_64-unknown-linux-gnu`。在 ARM、musl、
macOS 或 Windows 上“能够编译/启动”不构成支持。该工具是一次性 CLI，不是 Server/daemon，不监听端口，
没有 React/Vite 前端，也不需要空的 `clients/`、`config/` 或 `deploy/` 目录。

## 1.7 当前支持表

| 产品 | current identity | backup / verify / restore | recover | 说明 |
|---|---|---|---|---|
| Media Backup | `0.2.0` / r1 / `2563e6afc3fff272d02b7a5615272cc773862243bfd15aec51655abf1d9c6b1c` | 是，DB + tree 专用命令 | 是 | 两类资源必须同代处理 |
| Host Monitoring | `0.7.0` / r1 / `12dd1e61426b6b99df3d429b8c36ee3a5b22d1da776d98fc960b45b4f58c8e05` | 是，SQLite-only | 是 | Agent 本地状态不在 Server backup 内 |
| Sunshine Manager | `0.8.0` / r2 / `c9dedb33dd7a5ad613e762eb135a7aa5184ce1df52166459bee7b3485b4b3be3` | 是，keyed SQLite-only | 否 | backup/verify/restore 必须提供 external key |
| Sentinel Monitor | `0.2.0` / r1 / `f547ddc817d830d23b5305bb1f88b29898d6531568edd6eb194c2b629eb560c0` | 是，composite current | 是 | DB/recordings/三配置/key 精确同代 |
| Dufs RAM | `0.50.1` / r1 / `3659ff0c703515f555af95f0f1c08c35fa0555a8978f5f0e5a658fd93d225423` | 是，composite current | 是 | DB/shared root/`dufs.yaml` 精确同代 |
| Sarmg Foundation | 无 runtime state | 不适用 | 不适用 | 源码与 package 由 Git/registry 管理 |

表中的 SHA 是 code-owned current identity，供审计和 fixture 校验；它不是让操作者写入 metadata 或修改
manifest 的“修复值”。任意实际 DDL 漂移都应失败。

## 1.8 明确不做

不作为 daemon/API/Web；不自动停止启动服务；不覆盖 output；不跟随不可信链接；不猜版本；不自动跨多
edge；不把 raw key 写备份；不删除 recovery 证据；不为产品 runtime 生成兼容 shim。

还要特别区分以下非目标：

- 工具不替代产品 offline doctor；工具验证备份合同，产品仍需验证业务可运行性。
- 工具不负责 3-2-1、对象存储 immutability、retention 或 Secret 托管；它只生成可验证的本地备份集。
- `inspect-manifest` 不读取资源、不复算 SHA/schema、不验证 key，不能当作 `verify-*`。
- generic SQLite 命令只允许 Host 与 Sunshine；Media、Sentinel、Dufs 都是组合资源边界。
- Sunshine 当前 restore 中断没有公开 recover；不得借用 Host recover 或手工拼接。
- 工具不自动从 catalog、文件名、表结构相似度或版本号大小推断 adapter。

## 1.9 主要取舍

停机换取明确状态边界；全量 immutable backup 换取更多空间；显式 recovery action 换取避免误判；
external key 分离换取独立 Secret 运维；开发期删除历史 edge 换取更小的兼容负担。

| 取舍 | 得到什么 | 付出什么 |
|---|---|---|
| 停机窗口 + maintenance lock | 明确 generation 和恢复边界 | 需要协调 service/watchdog |
| immutable 全量备份 | 可独立验证、可保留证据 | 占用更多容量和 inode |
| exact identity allowlist | 不会“差不多匹配”错误库 | 每次 current Schema 变化都要同步代码/fixture/文档 |
| same-filesystem stage | rename 具有明确原子语义 | 目标卷需预留 stage + original 峰值空间 |
| 显式 recovery action | 不自动删除唯一好副本 | 需要人工判断和演练 |
| key 与数据分离 | 单份 backup 泄露不等于密钥泄露 | 灾备必须同时证明 key 可取得 |
| 当前不实现历史 edge | 当前代码与攻击面更小 | 旧开发数据默认重建；未来稳定 edge 只有在本仓库完整准入后才成为支持范围 |

## 1.10 从哪里核对代码事实

| 问题 | 首选代码锚点 |
|---|---|
| binary 支持哪些操作 | `src/support.rs::support_matrix` |
| CLI 真实注册哪些命令 | `src/main.rs::Command` |
| 产品有哪些持久资源 | `src/catalog.rs::Product::contract` |
| Media current 合同 | `src/current.rs` |
| Host/Sunshine current SQLite 合同 | `src/sqlite.rs` |
| SQLite restore/recover 阶段 | `src/sqlite/restore.rs` |
| Foundation manifest 包装策略 | `src/manifest.rs` |

阅读顺序很重要：先看 support 判断“能否做”，再看 catalog 理解“完整状态是什么”，最后进入具体 adapter
判断“如何证明”。只看到 catalog 中出现 Sentinel 或 Dufs 时，不得据此寻找或编造可运行命令。

## 1.11 本章检查

能说明为什么工具不自动选择“最近版本”、为何 manifest 自报不能替代 code allowlist、为什么错误后保留
recovery 目录比自动清理更安全。还应能不看文档说出三种 current adapter 的资源差异、哪些产品有 recover，
以及为何 `UpgradeEdge` 类型存在但所有 `upgrade_edges=[]` 仍表示没有升级能力。
