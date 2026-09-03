# 06. 产品 Adapter 与组合状态

## 6.1 Adapter 不是通用脚本

每个 adapter 编译进精确产品、版本、Schema、资源和不变量。它不会根据表名相似或 manifest 自报选择逻辑。
缺失产品/版本意味着不支持，需要开发新 adapter。

一个可发布 adapter 至少同时拥有：显式 CLI 路由、support 条目、code-owned current identity、完整资源
合同、输入/业务验证器、backup/verify/restore 状态机、必要的 recover、恶意 fixture、故障注入、运维与
功能取舍说明。只有 catalog entry、`Product` enum 或通用函数调用都不构成 adapter。

## 6.2 为什么按产品分边界

同样使用 SQLite 的产品仍可能在以下方面完全不同：是否有数据树/config/companion；是否需要 external key；
业务行如何引用文件；哪些服务必须停止；哪些 lock 组成一致 generation；是否能公开 recover。generic 层只
能复用哈希、manifest、SQLite snapshot、stage/journal 等机制，不能替产品决定状态闭包。

## 6.3 Media Backup

当前状态是 SQLite + media data tree。备份、恢复和 recovery 使用 `backup-media`、`verify-media-backup`、
`restore-media`、`recover-media-restore` 专用命令；两者不可拆分，也不能使用 generic SQLite。

当前 exact identity 是 `0.2.0` / revision 1 /
`2563e6afc3fff272d02b7a5615272cc773862243bfd15aec51655abf1d9c6b1c`。adapter ID 固定为
`media-backup-current-0.2.0-r1`，manifest 的唯一 current version 固定为 3；version 2 不再读取。

产品专用验证会读取数据库 `blobs` 与 `accounts`：组合 account/blob storage path，要求为安全相对路径，
再确认树中是单硬链接普通文件、长度等于 `stored_size`、BLAKE3 等于 `content_blake3`。备份 tree inventory
另用 SHA-256 绑定 tree 根 mode、非根目录 path/mode 与归档文件 path/mode/size/SHA；backup 顶层还必须
exact 只有 DB、tree、manifest。BLAKE3 证明业务引用，SHA-256/inventory 证明备份资源，两者职责不同。

当前 Media 没有 configuration 与 external key 资源。不要因为 composite option 内部预留字段就向 CLI
添加空参数或在 manifest 中写空壳配置。

## 6.4 Host Monitoring

当前 Server 状态为 code-allowlisted SQLite，可使用受限 generic SQLite command。Agent 本地身份/Spool 不
自动包含在 Server backup。当前没有 Host 历史 edge。

当前 exact identity 是 `0.7.0` / revision 1 /
`12dd1e61426b6b99df3d429b8c36ee3a5b22d1da776d98fc960b45b4f58c8e05`。Host 使用
`backup-sqlite`、`verify-sqlite`、`restore-sqlite`，并且是 SQLite-only 产品中唯一公开
`recover-sqlite` 的产品。

这里的 Host 指 Server current database。`host-monitor` Agent 的设备身份、local config、spool 或诊断文件
属于 Agent 自己的运行边界，不会因为 Server backup 成功而被灾备。运维若需要全系统恢复，必须另行记录
Agent 重注册/重部署策略。

## 6.5 Sunshine Manager

SQLite-only 物理资源还依赖 external credential key。verify/restore 必须提供精确 key ID/file，并实际认证
所有非 NULL Host `secret` 与所有非 NULL operation `request_ciphertext`；是否属于“已完成”状态不影响扫描。
原始 key 不进包。

当前 exact identity 是 `0.8.0` / revision 2 /
`c9dedb33dd7a5ad613e762eb135a7aa5184ce1df52166459bee7b3485b4b3be3`。manifest 中 external requirement 固定
记录 `kind=credentials-key`、key ID、key SHA-256、`algorithm=aes-256-gcm`、`envelope_version=1`；记录摘要
不等于拥有 key。

key ID 必须是 1..64 个 `[A-Za-z0-9_-]`；key file 不超过 4096 bytes、为单硬链接私有普通文件，读取前后
身份和 metadata 不变，Base64 解码后精确 32 bytes。verify 会扫描并认证上述全部非 NULL 密文，防止一个
“ID 相同但 bytes 不同”的 key 被误接受。当前 AES-256-GCM 解密使用 12-byte nonce；Host 凭据
绑定 Host ID 与 `secret` 字段域，operation request 绑定 operation ID、action 与
`request_ciphertext` 字段域的长度分帧 AAD。空 AAD 密文必须拒绝。

Sunshine 支持 current backup/verify/restore，但 `support` 的 recover 列表为空。恢复中断后保全现场并进入
人工事件流程；不能用 Host recover，也不能因为知道 key 就手工完成 journal。

## 6.6 Sentinel Monitor

SQLite + MediaMTX config/contract + recordings tree + external key 是一个组合。当前 `0.2.0` / revision 1 /
`f547ddc817d830d23b5305bb1f88b29898d6531568edd6eb194c2b629eb560c0` adapter 使用 `backup-current`、
`verify-current`、`restore-current` 和 `recover-current`，严格要求 `mediamtx.lock`、`mediamtx.yml`、
`sentinel.env` 三个配置、recordings 树与当前 credentials key。密文按当前 HKDF/AES-GCM/AAD 合同逐条认证。
任何缺失、额外、改名或内容不一致都会使完整 verify 失败；不能使用 generic SQLite 冒充完整备份。

## 6.7 Dufs

SQLite + protected YAML + shared root 构成状态。当前 `0.50.1` / revision 1 /
`3659ff0c703515f555af95f0f1c08c35fa0555a8978f5f0e5a658fd93d225423` adapter 使用同一组 `*-current` 命令，
并要求配置集恰好是 `dufs.yaml`。备份、恢复和 recovery 将数据库、shared root 与配置作为同一代处理。
Dufs 的 data-plane 用户/文件所有者规则、protected YAML 和 shared root 是产品语义，不能由管理数据库单独重建，
也不能使用 generic SQLite 命令替代。

## 6.8 Sarmg Foundation

Foundation 没有 runtime state；catalog 可说明这一事实，但不提供 backup/restore adapter。源码和 package
发布由 Git/registry 流程管理，不能伪装成数据库备份。

本工具依赖 Foundation `sarmg-contracts =0.4.0` 与 `sarmg-schema-identity =0.4.0`，两者 Git rev 都是
`0e1be10273fd6abf72e0d0eeb24cbb1120572486`。这说明共享当前线类型和算法来自 Foundation；不表示 Foundation
是运行时服务，也不表示它替产品验证数据库。不得改用 workspace sibling、Cargo path dependency、可变
branch 或本地副本，也不能在依赖不可用时复制一份旧类型作 fallback。

## 6.9 新名称边界

所有 adapter、manifest、CLI、测试和文档只使用当前产品名称。工具不接受另一 product slug，也不自动
重写旧 manifest 的名称。

canonical slugs 只有：`media-backup`、`host-monitoring`、`sunshine-manager`、`sentinel-monitor`、`dufs-ram`、
`sarmg-foundation`。`FromStr` 精确比较；大小写变化、旧仓库名、展示名或拼写相近值都失败。当前无受支持
旧名称 edge；未来稳定版本若必须迁移名称，只能在本 `sarmg-upgrade` 仓库新增精确 source/target 的独立
adapter、fixture 与 CLI，不得向 current parser 添加 serde alias 或 fallback。

## 6.10 选错命令

在 mutation 前通过 product、version、resource shape、Schema、external requirement 和 code allowlist 多重
拒绝。操作者不能用 generic command 绕过组合产品资源合同。

| 误用 | 拒绝原因 |
|---|---|
| `backup-sqlite --product media-backup` | catalog 不等于单 SQLite；缺 data tree |
| `backup-sqlite --product sentinel-monitor` | 缺 config/companion/recordings/key |
| `backup-sqlite --product dufs-ram` | 缺 data tree/config |
| Sunshine 不带两个 key 参数 | 无法证明外部要求和密文 |
| Host 带 key 参数 | key option 只属于 Sunshine |
| `recover-sqlite --product sunshine-manager` | 当前 recover allowlist 仅 Host |
| 修改 product slug 后 verify | 实际 Schema identity 与 official allowlist 不符 |

## 6.11 Adapter 能力闭包检查表

开发者评审一个新增 current adapter 时，应逐项回答：

1. support 是否只列实际已实现的精确 version/operation？
2. catalog 的所有持久资源是否一次性纳入 generation？
3. official identity 和 fixture 是否由产品 current Schema 生成并精确匹配？
4. 是否验证产品业务不变量，而不只验证 SQLite integrity？
5. source、output、target、key 的路径身份和容量边界在哪里？
6. backup 是否 pending + manifest last + fsync + no-clobber + 发布后复验？
7. restore 是否在 mutation 前全量验证并持久化 journal？
8. 每个资源是否同文件系统 stage、preserve、install、verify？
9. 是否需要 external key，raw bytes 能否进入任何输出？
10. recover 若不公开，support、CLI 和运维是否明确事件处置边界？
11. 是否有 tamper、链接、超限、错误产品/version/key 和每阶段 crash 测试？
12. 是否保持 `upgrade_edges=[]`，除非一条完整、可审计且在本仓库原子准入的历史 edge 真正落地？

## 6.12 本章检查

应能为六个产品分别说出 catalog 资源与 support 结论；能解释 Host/Sunshine 虽都 SQLite-only 却在 key 和
recover 上不同；能说明 Media 数据库与 tree 的双重完整性验证；能判断新增 enum/catalog entry 为什么不等于
新增 adapter。
