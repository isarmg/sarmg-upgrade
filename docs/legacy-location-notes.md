# 历史说明位置整理

以下为本轮从其他仓库当前文档移出的原位置事实，供追溯。它们记录当时的文本，不是本工具当前支持范围、执行步骤或验收要求；当前支持和行为以本仓库现行说明及真实代码为准。原不可变tag与已封存资产没有修改。

### xcos/README.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
本轮升级支持从当前版本到未来版本；普通运行只接受当前格式，不提供历史版本转换入口。
```

### xcos/docs/README.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
构建、bootstrap、配置、锁、doctor、备份和事件响应
```

### xcos/docs/project-workflow.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
   ├─ lifecycle + relocated smoke tests
   └─ 外部升级仓处理未来代际数据操作
```

### xcos/docs/project-workflow.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
产品运行时只理解当前合同。数据库转换、历史数据导入、备份和恢复不进入 Sentinel 运行路径；这些能力
必须由单独仓库在停机、排他锁和明确输入输出合同下完成。
```

### xcos/docs/project-workflow.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
未来备份、恢复或升级必须在两进程停止后，由外部升级仓固定按 database maintenance、runtime、MediaMTX
顺序取得排他锁，把 SQLite、MediaMTX config/contract、recordings 和 external key 身份作为组合状态处理。
普通运行不扫描其他代路径、不解析非当前Schema/密文，也不通过fallback修补数据。本轮只支持当前版本到未来版本的升级，备份、程序原子切换和恢复由xssc负责；历史数据库转换不属于支持范围，产品没有历史state转换入口。
```

### xcos/docs/operations.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
下面的 systemd 方案使用核心 `current/bin` 入口与独立私有 JSON，供统一升级工具停止服务、验证完整新 bundle 并原子切换 `current`。
```

### xcos/docs/operations.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
已有当前数据必须只读校验；升级只考虑当前版本到未来版本，由统一升级工具执行，不重置管理员。
```

### xcos/docs/operations.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
数据库、录像、私有 JSON 和其他实际状态按产品当前 state contract 与升级请求备份。统一升级工具负责 bundle 验证和指针原子切换，shell 和 unit 不实现第二套升级逻辑。
```

### xcos/docs/operations.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
| doctor Schema 失败 | 停止服务，保全 generation，交给升级工具 |
```

### xcos/docs/beginner-guide/07-current-contracts-and-cryptography.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
本轮只验收当前版本到未来版本的程序升级与备份恢复；旧格式只读拒绝，不提供历史转换入口。
```

### xcos/docs/beginner-guide/07-current-contracts-and-cryptography.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
运行时只接受一个当前 key，没有 previous-key keyring。轮换、全量 re-encryption 和验证由停机的
`xssc` 完成，成功安装新 generation 后产品只看新 key。
```

### xcos/docs/beginner-guide/07-current-contracts-and-cryptography.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
在升级仓实现精确转换；删除旧
reader和alias。
```

### xcos/docs/beginner-guide/06-mediamtx-recording-and-playback.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
## 6.7 组合备份

使用 `xssc` 同时取得数据库、MediaMTX config/contract 和完整 recordings tree，并由升级仓生成
recordings inventory。Sentinel doctor 本身不建立逐文件 inventory。external key 仅以 ID/要求写 manifest，
原始 key 独立保管。恢复后先验证 key 与所有密文，再启动 companion。
```

### xcos/docs/beginner-guide/06-mediamtx-recording-and-playback.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
升级 MediaMTX 必须同步 binary、SHA、config、start/doctor、release manifest、真实 smoke 和升级仓资源合同；
不能只替换 executable。
```

### xcos/docs/beginner-guide/06-mediamtx-recording-and-playback.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
升级 stage 和备份
```

### xcos/docs/beginner-guide/06-mediamtx-recording-and-playback.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
升级协议语义
```

### xcos/docs/beginner-guide/09-deployment-security-and-operations.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
## 9.6 备份恢复

只使用 Sentinel 专用组合命令。发行 manifest 验证发行树全部文件 Hash；升级/备份仓还必须自行生成并
验证 recordings inventory 与 external key 认证，并在隔离主机 restore + doctor + 播放。当前 Sentinel
doctor 只验证录像根安全属性和可清理写探针，不会逐个核对录像文件 Hash；只验证目录存在同样不充分。
```

### xcos/docs/beginner-guide/09-deployment-security-and-operations.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
## 9.9 回滚

保持两个服务停止，按 `xssc` recovery journal 选择 commit 或 rollback，验证完整组合状态和 key，
再安装与其精确匹配的 release。不能只回滚 binary 或数据库的一半。
```

### xcos/docs/beginner-guide/09-deployment-security-and-operations.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
| Schema drift | 停止并保全 generation，交给升级工具 |
```

### xcos/docs/beginner-guide/01-project-overview.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
只复制 `app.db` 不能形成可恢复备份；原始 key bytes 又不能塞进备份包。
```

### xcos/docs/beginner-guide/01-project-overview.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
旧状态兼容、产品内备份恢复、自动猜测
```

### xcos/docs/beginner-guide/01-project-overview.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
增加升级时的联合验证工作
```

### xcos/docs/beginner-guide/README.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
- 凭据变化：定义新的完整当前 envelope；历史转换只能放在升级仓库。
```

### xcos/docs/beginner-guide/README.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
- MediaMTX 升级：同一变更更新版本、SHA-256、lock、配置和 lifecycle 测试。
```

### xcos/docs/beginner-guide/10-reading-roadmap-and-glossary.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
升级是组合变更、为何 current Schema 不能现场修补，并能在临时环境完成一次恢复演练。
```

### xcos/docs/account-settings.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
账号设置不执行数据库升级或迁移。
```

### xcos/docs/feature-inventory-and-tradeoffs.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
启动路径只证明当前 Schema、当前 credential envelope 和当前 external key。当前版本到未来版本的程序切换、组合备份或
恢复都需要停机排他锁和独立审计，由 `xssc` 单独完成。本轮不提供历史版本转换。Sentinel 本身不扫描其他目录、不猜测
格式、不尝试“尽量启动”。
```

### xcos/docs/releases/1.0.0.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
- 核心CLI改为`init`、`run`、`config validate`、`status`、help/version。初始化显式读取stdin密码，只接受空目标；普通运行验证当前数据，不自动建库或创建管理员。正式source-bound binary必须使用`run --release-root`并验证物理发行树、target、源码、Web、MediaMTX及全清单。
- 原生`bootstrap`生成配置，`bootstrap --confirm-config`审阅后调用显式初始化/只读验证，成功才移除临时密码；`start`只运行已初始化数据。临时发行树生命周期测试和shellcheck已实际通过，包含重复确认不重建、锁/PID身份、启动失败回滚、发布不可覆盖及链接负例。
- 当前数据格式是`sentinel-db-v2`/Schema8；Client配对与设备快照使用`sentinel-edge-v4`，能力值为`supported`、`unsupported`、`unknown`。浏览器契约改为`sentinel-wire-v3`，HTTP路径前缀仍为`/api/v1`；媒体JWT v2保持独立身份。
- 归档bool能力只由`xssc`通过固定`sentinel-capabilities-v1-to-v2`离线委派转换：true变supported，false变unknown。写入必须持当前exclusive维护锁、继承FD3并匹配session、阶段、数据目录身份、迁移ID与目标binarySHA；普通运行不读旧格式。
- 公共配置/来源优先级、安全HTTP错误和请求编号、维护锁、只读SQLite当前代快照、运行任务和有界结构化日志复用Foundation0.10.6候选输入。官方来源远端发布、独立正式制品与产品升级用户路径必须分别验收，不能借旧消费者CI记录宣称本次发行已通过。
- 本轮真实摄像头验收按用户“无真实摄像头，跳过这个”要求跳过，不扩大厂商、型号或固件支持声明。Windows/macOS原生Client验收没有被该请求免除。
```

### xcos/docs/releases/1.0.0.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
发布制品绑定精确 Git 提交和当前版本；以附件校验和及制品内身份为准。历史标签和 Release 保持不可变。
版本身份、摄像头凭据绑定及 Media JWT issuer 随当前版本更新；旧状态不自动迁移。迁移与恢复仍由 xssc 的独立审定 adapter 负责。
```

### xcos/docs/releases/1.0.0.md @ `970cd57b6ad7edc6c814bf46597993ca4d5a8fab`

```text
- 修复正式发行树重定位后的录像目录诊断：仅完整校验的执行树内固定MediaMTX模板可使用launcher从唯一`recordings_directory`构造的覆盖值。外部或开发YAML仍严格检查实际路径；受管模板改写、重复字段、目录别名或宽松权限均拒绝。
- 在线Doctor读取固定loopback路径默认配置API，核对MediaMTX真正生效的`recordPath`。读取限时3秒、响应16KiB并禁重定向，不把HTTP可达等同于配置正确。
- Native构建器将Cargo生成的ELF复制到私有stage，再检查单链接、版本、Source、静态资源和完整发行身份。发行包校验与原子不可覆盖发布保持严格。
- 本轮仅支持当前版本到未来版本的程序升级和备份恢复，删除历史状态转换入口。当前`sentinel-db-v2`/Schema8、`sentinel-edge-v4`、`sentinel-wire-v3`以及独立media JWT v2保持不变，旧数据仍拒绝。
- Foundation公共输入仍为固定0.10.6；不借旧CI证据宣称本次实际发行验证已完成。真实摄像头按用户要求跳过，其他原生平台没有被该请求免除。
```

## Foundation原位置（历史记录，非当前运行说明）

### xcss/docs/server-client-repositories.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
`consumers/repositories.toml` 只记录 Server 消费者及离线维护工具。Client SDK 基线和测试由
`xcsc` 维护。产品历史与发布标签不重写，旧发布记录不作为新拆分提交的验收证据。
`xssc` 不因仓库改名自动增加版本或客户端状态支持；第三方字体仓库不按产品端拆分。
```

### xcss/docs/feature-inventory-and-tradeoffs.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
| FND-005 | 当前状态离线备份/恢复归 `xssc` 与产品 adapter | README、合同边界、空 Foundation state | 核心 | 高 | 在线 runtime 被非当前解析器和高权限修改逻辑污染 | Foundation 不含 migration edge 或产品 DDL |
```

### xcss/docs/feature-inventory-and-tradeoffs.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
| FND-136 | Release不含升级工具版本/edge | exact contract | 核心 | 中 | Foundation release被历史迁移矩阵耦合 | unknown字段拒绝 |
```

### xcss/docs/feature-inventory-and-tradeoffs.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
| FND-390 | backup/restore journal与crash recovery | 升级工具/产品adapter | 核心 | 高 | 资源组合/Secret/原子替换语义被错误泛化 | 共享只提供manifest contract |
```

### xcss/docs/feature-inventory-and-tradeoffs.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
执行删除时同步移除 member、dependency、export、Schema、fixture、test、lock、package/release inventory、
CI、文档和消费者调用，不留下 alias。若改变持久格式，在线产品直接只接受新当前格式；只有明确的稳定
source/target状态才在独立 `xssc` 创建转换。
```

### xcss/docs/project-workflow.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
本仓坚持 current-only：每个版本只存在一套公开名称、格式和行为。任何历史读取、字段 alias、双写、旧
散列降级或运行时版本分支都不进入该流程；确需处理已发布状态时，由 `xssc` 建立独立、精确、
离线的转换流程。
```

### xcss/docs/project-workflow.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
结论只有四种：由 Foundation 拥有；表达为通用 Profile/Capability；保留为产品 Adapter/业务语义；由
`xssc` 处理服务端当前状态的离线维护。单一参考消费者不是拒绝平台责任的理由，但能力必须完成规范、产品无关实现、
参考消费者迁移和旧实现删除这一整条纵向切片。Foundation 是构建期中央平台，不是中央运行服务。
```

### xcss/docs/project-workflow.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
Server target 统一不等于所有代码只能 AMD64。xsoc Client、Android/iOS 客户端、移动 FFI 和其他非
Server binary 不依赖该 crate，继续按自己的平台矩阵构建。Foundation 和 Upgrade 没有在线业务 Server；
Foundation 发布 identity 使用 `source-any`，Upgrade 的 CLI 发行目标由其自身合同约束。
```

### xcss/docs/configuration-cli-logging.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
本文描述 0.10.8 发行候选的公共实现。已发布基线仍为不可变 `v0.10.4`；新增的 `xcss-config`、`xcss-server-cli`、`xcss-log` 及本次锁/快照 API 正按新的唯一版本 0.10.8 准备本地受控源码与制品，尚未推送或发布。六个产品与 Client 的中立日志继续固定已封存的 0.10.6 完整源码 revision 与不可变 Web 资产；只有离线 Upgrade 因行政维护 API 采用 0.10.8。两者差异可追溯，不要求无变更产品锁步更新。官方远端发布与独立正式发行仍需完成。
```

### xcss/docs/configuration-cli-logging.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
`state_error` 和 `snapshot_error` 保存公共文件/锁/快照错误的机器语义；锁忙、升级待恢复、缺失、权限、身份变化、离线迁移委派和快照繁忙等使用稳定码，路径、owner、权限位和内部 I/O/SQL 链不进入响应。
```

### xcss/docs/configuration-cli-logging.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
`xcss-state-file` 使用数据目录中 `.xcss-maintenance.lock` 与 `.xcss-instance.lock`。正常运行持有共享维护锁和独占实例锁；升级维护持有独占维护锁；显式在线诊断写探针可持 `try_shared_maintenance_lock`。身份检查覆盖目录和 lock 文件的 owner、700/600 权限、普通单链接及 device/inode。
```

### xcss/docs/configuration-cli-logging.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
`.xssc-pending.json` 的任何目录项都阻止正常运行和在线写探针，包括损坏文件或悬空链接。升级独占维护不受该门阻止，门的写入、同步、阶段与移除由升级工具管理。`release(self)` 在业务资源全部关闭后显式 unlock，处理短时继承描述符的运行权交接；异常退出仍依靠描述符关闭释放。
```

### xcss/docs/configuration-cli-logging.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
行政数据目录接口不用于执行发行目录。高权限工具的 journal/备份保持 root-owned 0700；服务 worker 读取的 sealed 程序、内嵌 Web 及 companion 闭包位于另一个 root-owned、服务不可写的实体树。服务只获得 read/execute，所有父目录必须受控可遍历；不能将可置换工作目录 chown 给服务后依赖一次 SHA 校验。执行描述符和产品完整发行校验仍由 Upgrade 控制。非协作的同 uid writer 不属于维护锁支持边界。
```

### xcss/docs/operations.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
Foundation 没有生产 daemon、监听端口、业务数据库、用户表、Session 表、systemd unit 或运行时配置文件。
本文件中的“运维”专指源码仓库、固定工具链、依赖锁、CI 权限、npm package、release asset、消费者采用
证据和共享安全事件。产品数据库备份、恢复、迁移、Secret 轮换和服务启停应查各产品及 `xssc`
的运维文档。
```

### xcss/docs/README.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
四个 package 目录中的 `README.md` 是发布包随附的必要 README，仍属于“必要 README”分类；它们只解释
各自公开入口和边界，不另建教程体系。Foundation 无生产 daemon，所以没有启动、systemd、业务数据备份
或在线告警 runbook；相关工作分别属于各产品及 `xssc`。
```

### xcss/docs/beginner-guide/04-contracts-types-guards-and-json-schema.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
若字段变化涉及已发布持久manifest，由`xssc`建立精确离线转换；Foundation当前parser不同时接受
两个版本。
```

### xcss/docs/beginner-guide/07-versioning-publishing-and-breaking-changes.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
持久状态必须符合运行版本的当前 Schema 和身份；在线产品拒绝不匹配的状态。
`xssc` 负责当前状态的离线维护，不要求实现历史版本转换。
```

### xcss/docs/beginner-guide/01-project-overview.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
稳定发布后未来需要状态转换
 -> 在线产品只接受target current
 -> xssc离线识别精确source
 -> 原子转换/验证到精确target
```
```

### xcss/docs/beginner-guide/10-reading-roadmap-and-glossary.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
| State/Release/Backup | contracts Rust/TS/Schema | release builder、xssc |
```

### xcss/docs/beginner-guide/README.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
离线/发布工具
├─ contracts/Schema identity供xssc复用
├─ package-artifacts审计真实tgz
└─ release-tree验证本地普通文件树
```
```

### xcss/docs/beginner-guide/README.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
当前产品只接受当前字段、Schema、密码散列和发行树，发现不匹配就失败。若未来确实有稳定版本之间的状态
转换，放到 `xssc` 离线执行；在线产品不同时支持两个版本。
```

### xcss/docs/platform-specifications/platform-migration-roadmap.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
```text
第三方库 -> xcss -> 产品 Adapter -> 产品业务
当前状态离线维护 ---------------------------> xssc
```
```

### xcss/docs/platform-specifications/platform-migration-roadmap.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
在线产品只定义、创建和读取唯一当前格式。持久格式变化时同步更新当前 Schema、fingerprint、fixture 和验证；
不匹配的状态在启动时拒绝，不携带历史 reader、双读写或兼容 fallback。
`xssc` 负责服务端当前状态的离线维护，不要求实现历史版本转换。
Foundation 不保存指向其私有目录的 baseline，也不根据下游 fixture 布局决定构建或发布结果。
```

### xcss/docs/architecture/ADR-0008-no-product-branches.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
Foundation 源码、Feature、Profile 和 Capability 中不得按产品 ID 分支或使用产品名称。允许的差异只有
Profile、Capability、Adapter/Trait 和产品业务 Schema。当前状态离线维护的产品 Adapter 由 `xssc` 拥有。
```

### xcss/docs/architecture/ADR-0004-current-only-offline-upgrades.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
Foundation 和在线产品只定义、创建、读取唯一当前格式，不实现历史 parser、兼容别名、双读写或隐式降级。
当前规则不要求任何仓库实现历史版本转换。`xssc` 仅承担服务端当前状态的离线维护。
发行制品和消费者验收证据保持不可变。持久格式变化时同步更新当前 Schema、身份、fixture 和验证，
在线启动严格拒绝不匹配的状态，不进行隐式转换。
```

### xcss/docs/architecture/README.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
| `0004：current-only-offline-upgrades（历史 ADR）` | current-only 与离线维护所有权 |
```

### xcss/docs/releases/1.0.0.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
0.10.7 新增离线维护的明确权限桥：高权限 Upgrade 能锁住服务 uid 拥有的私有数据目录，生成服务 uid/gid 的锁和 pending 文件，再把固定的维护 lease 委派给降权 worker。普通运行时的精确 uid 检查保持不变。
```

### xcss/docs/releases/1.0.0.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
这不是产品运行时的权限放宽，也不拥有系统用户、systemd 或产品迁移。Upgrade 保持 root-owned 私有 journal/备份与另一个 root-owned、服务不可写但可读执行的 sealed 发行闭包；固定 descriptor 执行和完整产品身份校验由工具负责。不能把可替换工作目录交给服务 uid。
```

### xcss/docs/releases/1.0.0.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
六个 Server 及 Client 的中立日志仍采用封存的 0.10.6 / `c0dfab08e54ad9df9c884fa31207f19b07a8e501` 和原八个 Web tgz。仅 Upgrade 因新增行政 API 更新到 0.10.7，差异有明确原因，不要求无变更产品重复更新。两个不可变 tag 和资产均保留。
```

### xcss/docs/releases/1.0.0.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
- 新 `xcss-config` 统一typed严格JSON、逐层校验、显式环境映射、命令行优先级和字段来源。私有配置读取有1MiB上限，拒绝FIFO/链接/多链接/坏权限，不回显输入值。
- 新 `xcss-server-cli` 统一机器错误、HTTP JSON/Query/Path安全分类拒绝、413/no-store、统一有界请求编号/业务错误关联、状态文件与快照稳定错误映射、服务身份和实际readiness查询、只读日志目录/账户检查。
- 新中立叶模块 `xcss-log` 可被Server与Client按需消费：统一UTC记录和公共模板、Server/instance关联、脱敏、精确字段查询、容量受控轮转与typed tracing sink切换，writer失败可观察。
- `xcss-state-file` 增加持久维护门、共享诊断写锁、显式锁交接和fixed FD3离线迁移授权；session/phase/data-dir/inode/migration-id/目标可执行SHA全部核验。
- `xcss-sqlite` 提供当前WAL/journal代的只读临时验证副本和同代into_pool；诊断必须在不持有源SQLite连接的独立进程执行，不声明为可恢复备份。
- 静态管理员检查复用唯一当前文件结构；既有持久账户加载不再无条件改写文件。
- 24个Rust crate和8个Web package及内部依赖/锁文件统一1.0.0。版本冻结后完整workspace 175项Rust测试、73项Web单元测试、44项Chromium/Firefox浏览器测试、54项Python工具测试全部通过；Clippy/fmt/doc及24个Cargo许可证清单通过。8个实际npm tarball的独立临时消费者安装、41个exports、TypeScript与Vite烟测通过。不可变源码和正式制品清单另记录。
- 六产品真实发行物、统一升级/恢复、设备与Windows/macOS等原生验证由产品分别负责，不能借用公共测试宣称完成。正式发布前还需完成远端CI和新不可变输入的产品独立构建。
```

### xcss/docs/releases/2026-09-05-closeout.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
- 原 Foundation 的 GitHub 仓库更名为 `xcss`，保留原 Git 历史；独立创建 `xcsc` 仓库和历史。两库互不依赖。
- Server 管理 Web 归 Server；Client 本机管理 Web 归 Client/client。Host Windows 托盘本地页面没有迁入 Server。五个产品的主 `web` 管理 Server。
- 六个消费者的 Server Rust 依赖锁定 `0.6.0` 和完整提交 `1e889d08fa69fcf2b5fffe45e8cc42b68218f4f1`；Web 使用 GitHub v0.6.0 tarball 和 lockfile integrity，不使用同级源码路径。
- Host/Media 的 Client Rust 依赖锁定 `0.6.0` 和完整提交 `a5b3158aa62af8189dd909500758cee649898e5c`。该源码版本后来确认 Windows Spool 阻断，见下表。
- 本次修复了发行指纹、当前 readiness、字体归档校验、原生测试夹具、独立构建缺失 Web 资源和特殊字符路径等真实失败。没有关闭安全门禁、降低覆盖率要求、跳过失败测试或改写已发布标签。
- `consumers/repositories.toml` 和生成矩阵记录本轮真实提交及直接使用的 Server 包。既有 `local-platform-stack` 例外仍保留为未完成项，不能解释为全部符合手册。旧的 baseline 记录不是新产品当前 Schema 的验收证据；Upgrade 当前 Schema 适配尚未完成，矩阵改记 `migration-in-progress`。
```

### xcss/docs/releases/2026-09-05-closeout.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
| xssc | 0.3.0 | [CI](https://github.com/isarmg/xssc/actions/runs/33978912762) 通过；当前产品 Schema 适配和正式签名仍阻断，未发布 |
```

### xcss/docs/releases/2026-09-05-closeout.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
1. **Windows Client 文件后端**：`PrivateDirectory::create` 对目录使用普通文件打开返回 AccessDenied，Spool 无法打开；还需原生句柄、reparse-point/ACL 和持久化合同的实现及验收。不能把目录同步改为成功空操作。修复必须发布新的 Client 版本，更新消费者完整 revision，不能覆盖 0.6.0。
2. **Media Android 签名**：`android-signing` 环境需既有身份的 `MEDIA_BACKUP_ANDROID_SIGNING_PKCS12_BASE64`、`MEDIA_BACKUP_ANDROID_SIGNING_PKCS12_PASSWORD`。证书 SHA-256 必须为 `0cfc2811d48cdeab3e6d857029d879e001ab9531c06784b4d48d15a847771421`。不得另生成签名身份或替代为 debug APK。
3. **Upgrade 当前状态合同**：Host、Sunshine、Media、Sentinel 的数据库 Schema 均已变化，当前适配器尚未完成同步和真实产品验收。只改版本号不能证明可验证/备份/恢复；仍只实现当前合同，不增加历史读取器。
4. **Upgrade 签名**：需已有 `release/xssc-release-signing-public.pem` 对应的 `RELEASE_SIGNING_KEY_PEM`。不得换公钥、生成替代密钥或发布未签名正式包。
```

### xcss/docs/releases/1.0.0.md @ `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`

```text
- `xcss-platform-db` 新增事务内平台元数据初始化与严格 Profile 验证入口，补齐公共平台表从建表、写入到读取验证的完整生命周期。
- 合规工具现在拒绝所有未识别或非精确的 Foundation Rust/Web 依赖写法，并通过 Cargo/npm 锁文件复核实际来源、revision、版本与 integrity。
- 源码接入报告与正式发布门禁分离：缺少 release 清单明确标记为 `not-checked`，发布门禁可要求清单必须存在。
- 删除 Foundation 对 Upgrade 私有 fixture 目录和产品历史 baseline 的认知；具体产品状态与迁移支持关系由 `xssc` 自己维护。
- 反向依赖检查按实际 Git/path 来源执行，不再依赖 `xcss-` 包名前缀猜测所有权。
```


### xcss/docs/configuration-cli-logging.md （原维护协议说明）

```text
Linux `MaintenanceLock::as_fd` 允许维护工具只为迁移子进程继承同一个 open file description。`verify_delegated_migration` 固定验证 FD3：必须已持独占 flock、属于当前私有数据目录的同一 lock inode，且有 schema 1 的严格有界迁移门。session、`migration-intent`、目录 identity、迁移 ID 和 `/proc/self/exe` 的目标 SHA-256 全部匹配才返回持有描述符的 guard。guard 仅关闭子引用，不能 unlock 父维护锁。该入口只用于显式离线转换，不扩大正常运行格式支持范围。
```

### xcss/docs/configuration-cli-logging.md （原维护协议说明）

```text
维护工具用 `PrivateDirectory::open_for_administration(data_dir)` 配合 `AtomicFile::replace` 保存 pending，用 `remove_file` 删除并 fsync。`PrivateStateDirectory::owner_uid/owner_gid` 给工具明确降权信息；产品 worker 以服务 uid 运行，继承同一个 exclusive lease description，原有 fd3/session/hash/phase 协议不变。普通运行入口不使用行政 API。
中性维护门常量为`MAINTENANCE_PENDING_FILE`，只读检查为`verify_no_pending_maintenance`，对应`Error::MaintenancePending`及安全机器码`state.maintenance_pending`。运行期不接受另一种门名称；持有独占锁不自动消除已有门。
```

### xocs/deploy/linux-x86_64/README.md（旧位置事实）

```text
数据库与媒体目录组成完整持久状态，升级和恢复由独立 `xssc` 负责。不要手动覆盖数据库、在维护中启动服务或将诊断临时副本当作恢复备份。当前 `poetize-db-v1` 正常运行不识别历史格式；工具仅能执行正式支持声明中的同结构更新或精确 `poetize-unversioned-v1-to-db-v1` 迁移。

旧版 Java/MySQL 数据不会自动转换为 SQLite；升级前请保留旧版数据和部署包。

```

### xcss/docs/consumers/axum-0.7.0-evidence.md @ `6c6206cf7df047fb8b4c72689fbba6495e2e7968`

以下为原位置历史说明，不能作为本轮当前支持声明：

```text
| xssc | 单独提交支持边界，明确不支持 Xczs 0.51.0 状态备份/验证/恢复/升级 | 不冒充已实现新版本适配 |
```

### xcss/docs/consumers/react-filesystem-0.7.1-evidence.md @ `6c6206cf7df047fb8b4c72689fbba6495e2e7968`

```text
xssc 仍明确不支持本版的备份、验证、恢复或升级，这是手册允许的明确支持边界，不是隐含适配。不允许修改旧 metadata 或把空库与旧共享树拼接；没有触碰真实实例数据。
```
