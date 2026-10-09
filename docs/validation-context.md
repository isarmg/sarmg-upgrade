# 集中迁入的复核快照

这是迁入时已有的复核记录，包含当时的未完成项与撤回候选事实，不代表当前版本验收完成。最新实际升级结果由 validation.md 单独记录；历史转换不计入当前支持或通过证据。以下引用的工作区制品路径均相对工作区根目录。

## 实施验收.md中的相关复核快照

验收依据为本目录唯一权威文本 `发展.md`，复核日期为 2026-10-07。范围包括两个 Foundation、六个业务 Server、四个配套 Client 和 `xssc`；`maple-font` 仅检查工作树未修改，不参与改造。此记录由公共基础实现负责人独立阅读各仓库源码、配置、流程和文档形成，产品负责人仍在并行修改，因此它记录的是本次复核快照，不能代替最后冻结源码后的发行验收。

**整体尚未完成验收。** 公共配置、CLI/HTTP 错误、诊断快照、维护锁和日志已有实际实现和风险测试；产品已开始消费。两个 Foundation 的本地提交/tag/源码 bundle 已冻结并有真实发行制品；产品 Cargo/npm 的相邻 path 和旧公共版本已从当前实际消费图消除；远端发布、当前版本到未来版本的真实升级/恢复路径、真实发行物和原生平台验证仍需完成。用户后续明确只考虑xssc当前版本到未来版本，历史版本转换已从本轮交付和验收要求撤销；保留旧数据拒绝、当前格式、原子程序切换、备份恢复与UID隔离要求。用户明确“无真实摄像头，跳过这个”，本轮真实摄像头验收按要求跳过，不作为阻塞，也不冒充支持证据。未执行的验证明确列为未验证，不把源码存在、测试通过、CI 通过和用户完成真实任务混为一谈。

| 仓库 | HEAD | 本轮主要事实锚点 |
|---|---|---|
| xcss | 六产品输入`c0dfab08e54ad9df9c884fa31207f19b07a8e501`/`v0.10.6`；Upgrade行政API输入`2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4`/`v0.10.7` | `rust/crates/xcss-config`、`xcss-server-cli`、`xcss-log`、`xcss-state-file`、`xcss-sqlite` |
| xcsc | `630bb16fe317c41c169cbfa7442cd06cce77257f`，本地最终候选 `v0.9.18` | `tools/client_policy.py`、`rust/crates/xcsc-runtime` |
| xczs | `8484e36ace4b5baec2e5ba29e880dd1776dbe878`，本地候选已冻结 | `src/main.rs`、`src/args.rs`、`src/logger.rs`、`src/server/tagging/db.rs` |
| xsos | `cd463e4ab681d4ab36a2192edf1a09a4e50d997f`，本地候选已冻结 | `xsos/src/main.rs`、`config.rs`、`retention.rs` |
| xszs | `535263637773ea77787f50f30cc20f4843b09f43`，本地候选已冻结 | `crates/server/src/main.rs`、`config.rs`、`database.rs` |
| xscs | `4df50e373a5a6ce1eed629ca9e38236f2e9f9db5`，本地候选已冻结 | `src/main.rs`、`config.rs`、`operations.rs` |
| xcos | 原`42f38ae8dbe78d5fe0a4cafac81095f97f1c60b9`/v0.4.0封存撤回未发布；0.4.1修改待新冻结 | `src/main.rs`、`runtime_lock.rs`、`routes.rs`、`lifecycle.rs` |
| xocs | 原`61fe129eccdcea7d14913c5eae921295338402e0`/v3.1.0封存撤回未发布；3.1.1修改待新冻结 | `xocs/src/main.rs`、`config.rs`、`schema.rs`、`state-contract` |
| xsoc | `3538971834b68af82eb8f6dab0a4e7b36105c27d`，本地候选已冻结 | `src/monitor_app`、`src/cli.rs`、`Cargo.toml` |
| xszc | `da7205639124f35052705f9c1752fe45ae1c2455`，本地候选已冻结 | `crates/client-core`、`xcsc-client.toml` |
| xscc | `6067e1317cbd367adfb12aa5e7a6f30332e8c233`，本地候选已冻结 | `src/main.rs`、`windows_service.rs` |
| xcoc | `03094971bef08db6c0c2ffeef79d48a2da507dd0`，工作树仍有待冻结变更 | `src/main.rs`、`device.rs`、`onvif.rs` |
| xssc | `27ad9f303dd72f9287283a423108e93f615f8f52`，工作树仍有待冻结变更 | `src/upgrade/mod.rs`、`src/upgrade/snapshot.rs`、`src/current.rs` |
| maple-font | `c08fda97` | `git status --short` 为空；未修改 |

| 验证 | 实际结果与覆盖范围 |
|---|---|
| `python3 scripts/check-foundation.py` | 通过；workspace/package/license/版本/依赖边界规则通过，新增三 crate 纳入清单 |
| `python3 -m unittest discover -s tools/tests`（Server Foundation） | 54 项通过；包括配置策略、发布清单、供应链流程、契约生成等工具行为 |
| 同一命令（Client Foundation） | 0.9.18 的21项通过；中立日志精确官方 git/rev/version，漂浮来源与 Server runtime 依赖仍拒绝；真实mobile ABI/helper重复拒绝，普通SQLite私有回调和风险测试不再误报 |
| `CARGO_NET_OFFLINE=true python3 scripts/check-rust-package-licenses.py` | 24/24 crate 的真实 `cargo package --list` 包含唯一 LICENSE；不表示已经发布 crates |
| `cargo +1.98.0 test --locked --offline -p xcss-config --all-features --lib` | 9 项通过；来源优先级、低层坏值不可被高层掩盖、结构/未知/重复/类型、秘密不回显、有界私有读取、真实 FIFO 拒绝 |
| `cargo +1.98.0 test --locked --offline -p xcss-log --all-features --lib` | 8 项通过；统一字段/UTC/template、秘密脱敏、精确筛选、预算、真实轮转、tracing 公共模板、动态 sink 切换及 writer 失败计数 |
| `cargo +1.98.0 test --locked --offline -p xcss-state-file --all-features --lib` | 7 项通过；真实锁互斥、目录/inode/链接、维护门、共享诊断写锁、显式 unlock、当前迁移会话和可执行 SHA 授权 |
| `cargo +1.98.0 test --locked --offline -p xcss-admin-static --all-features --lib` | 6 项通过；账户/会话、重启持久化、只读准确 ID 检查、不创建/改写账户文件、链接拒绝 |
| `cargo +1.98.0 test --locked --offline -p xcss-sqlite --all-features --lib` | 10 项通过；当前 schema/integrity、WAL-only 提交快照、源主库/WAL/SHM/journal 字节不变、活跃 writer busy、query-only与pool clone guard、先捕获再into_pool同代且不重新读源 |
| `cargo +1.98.0 test --locked --offline -p xcss-server-cli --lib` | 7 项通过；机器 stdout 单记录、未知数据不重建、日志目录校验不创建/修权限、JSON缺/未知字段/类型/结构及Query/Path拒绝、413/no-store、统一typed请求ID与安全state/snapshot机器错误、临时 loopback HTTP 查询真实 readiness 和服务身份 |
| `cargo +1.98.0 clippy --locked --offline --workspace --all-targets --all-features -- -D warnings` | Foundation Server 0.10.6冻结源码全workspace实际通过 |
| `cargo +1.98.0 fmt --all -- --check` | Foundation Server 全workspace通过；所有仓库最终冻结后的格式检查仍由对应产品完成 |
| `RUSTDOCFLAGS=-Dwarnings cargo +1.98.0 doc --locked --offline --workspace --all-features --no-deps` | Foundation Server 24 crate 文档通过；public API 文档构建无警告 |
| `cargo +1.98.0 clippy --locked --offline -p xcss-log --no-default-features -- -D warnings` | 通过；portable 无 tracing 使用不会因 unused API 产生警告 |

HTTP 状态测试使用真实临时本机监听；其服务行为是明确构造的测试 Router。升级服务管理器、摄像头以及 Windows/macOS 原生行为没有因此得到实测。SQLite 临时副本只用于独立诊断进程；不能作为同进程持有源 SQLite 连接时任意关闭原始 FD 的安全保证，也不是可恢复备份声明。

| 章节与条款 | 当前状态和具体证据 | 剩余事项 / 验收边界 |
|---|---|---|
| 一、1.1–1.4 总则 | 部分完成。已按真实通用需求实现上游机制；本记录明确区分目标与事实；Maple 工作树无修改 | 必须继续处理下文真实缺口，不能把清单或文档完成当成所有工程目标完成 |
| 二、2.1–2.3 定位 | 源码已实现主要边界。两 Foundation 为独立库；产品组合自身能力；Xocs 公共前台保留；Sentinel 设备适配在 Client；升级工具离线 | Sentinel MediaMTX、客户端 ffmpeg 等外部前提仍需实际制品路径验证；不应把缺失业务代码重新归类为外部依赖 |
| 三、3.1–3.5 依赖与复用 | 公共配置/日志/锁/错误/诊断均有实际上游代码，多产品 `Cargo.toml` 与调用点已消费，未增加中心服务或产品名单分支。Foundation 自身检查通过 | 当前11产品Cargo manifest/lock统一精确Server0.10.6或Client0.9.18；六Web各8包URL/SRI与真实归档一致，来源记录见发行准备。官方新输入尚未发布，独立官方fetch仍待执行；业务PID/目录规则与公共锁分别核对 |
| 四、4.1–4.4 工程与清理 | 源码使用 Rust1.98、统一 fmt/clippy、显式资源释放；Sentinel 旧重复 flock 与快照已由共享实现替换；Xocs 公共 DDL 直接取上游定义 | 最终所有仓库 fmt/check/test 尚未统一冻结复验；需检查旧脚本、旧环境样例和旧部署入口是否仍作为当前推荐路径 |
| 五、5.1–5.4 定义与身份 | `xcss-error`、schema identity、公共日志 registry 为明确事实源；typed 配置依产品结构校验，每层 hook 检查语义，当前 schema 同时检查实际 fingerprint | 新配置/CLI、升级定义和 Sentinel 能力三态变更的产品契约、Web guard、schema与发布身份需同步，禁止只改标识；全部跨语言定义来源尚未逐字段验收 |
| 六、6.1–6.5 错误 | 公共 CLI 单记录/非零退出、文件失败分类、HTTP extractor 安全 reason/真实400或413/415/no-store 已测；Xocs/Sentinel 有共享 wrapper 调用 | 四个其他 Server 接入共享 extractor 的全部入口、业务拒绝及未知路由仍需产品实测；老新 ErrorEnvelope 类型最终须统一依赖身份。错误边界不得因隐去诊断而让失败不可区分 |
| 七、7.1–7.5 当前契约 | 六 Server 正常运行将只读当前结构验证与显式 init 分离；统一工具使用 product-state-v1，产品历史state转换模块/入口按最新范围移除 | 各产品正常路径所有 `serde(alias)`、双解析、自动 schema 重建和旧入口必须最终检查；旧格式必须明确拒绝，不能假成功。历史版本转换不属于本轮交付范围；本轮只验当前版本到未来版本 |
| 八、8.1–8.5 安全 | 上游认证/会话/权限机制已有实际实现；init要求本地凭据；private 配置、管理员文件、密钥和日志限制已测，Server 业务授权仍属产品 | Client服务错误已改为公共安全事件的源代码，Windows原生实际日志仍未本机验证；全部管理/设备接口权限回归及真实 HTTPS/外部设备例外未全执行 |
| 九、9.1–9.6 CLI配置 | 六 Server 源码具有 init/run/config validate/status/help/version 核心路径；配置JSON、明确环境映射与来源、禁止低层坏值掩盖已接。正常运行不默认bootstrap的路径已修改 | 六个真实二进制的首次init、缺凭据、未知数据、只读validate、重启、status失败、机器输出、实际来源全矩阵仍待冻结发行物验收；Sentinel native当前推荐正同步0.4.1显式init/run/status与当前Schema/wire；真实source-bound/fullbundle测试仍待冻结制品 |
| 十、10.1–10.4 单二进制 | 各 Web Server build.rs 消费 `xcss-web-assets`，Xocs `static_assets.rs` include生成表；升级工具独立，不作为在线Foundation中心 | 这证明源码嵌入路径，不能证明最新正式制品。须以无开发目录、断网、重定位的实际二进制验证必要静态资源、业务代码及已声明外部依赖 |
| 十一、11.1–11.4 生命周期 | Foundation Runtime健康/任务/退出机制及状态查询实现；Xocs/Sentinel 接入 WorkScope/Lifecycle，四 Server 复用实际Runtime transport；升级门阻止中断后错误启动 | 所有业务任务的停止期限、任务未结束时的状态、DB/文件/锁关闭次序、关键依赖故障readyz与降级仍需产品真实运行和故障注入 |
| 十二、12.1–12.6 数据一致性 | private目录/文件、稳定身份、原子保存、共用maintenance/instance锁与pending门已有实际测试；只读诊断副本包含当前WAL；升级快照维护下保护程序/配置/数据 | 正式跨资源备份完整范围须按产品 `state_paths` 与签名资源比对；journal中断、磁盘不足、目录替换、sidecar/live writer等需实际产品演练，临时诊断副本不可称可靠备份 |
| 十三、13.1–13.5 任务 | `xcss-operations` 的持久意图/Unknown/outbox，Sunshine `operations.rs`、Sentinel reconciliation 实际业务判定存在；普通查询未强制复杂队列 | 必须复核每种不可重复设备控制超时后的 Unknown/重试策略及取消完成；xcoc已实现全局inflight256/单camera8/command records4096的实际预算，满容量保留已知结果并限制新领取；仍需结合业务任务和崩溃恢复的产品实测 |
| 十四、14.1–14.4 上限与隔离 | config1MiB/256覆盖、snapshot4GiB/10秒/3次、status4KiB/3秒、日志16KiB记录/轮转40MiB；Host retention批次/时间/保留有明确代码 | Sentinel ffprobe已消费公共capture_bounded，ONVIF discovery256/xaddr8/scope32与队列累计预算源码已落实；多实例负载、磁盘边界和容量默认值的真实验证未执行 |
| 十五、15.1–15.5 日志 | 中立 `xcss-log` 实现UTC共同字段/固定common模板/server-instance作用域/稳定ID/精确字段查询/有界rotation；tracing继承及动态sink failure计数实测 | 各 Server Run须在锁与完整preflight后切到文件，Clients外部服务管理器保留须明确；旧 raw日志/错误输出清理、产品日志关联、日志访问授权与各平台原生sink未全验证 |
| 十六、16.1–16.4 界面 | 多产品 Web package消费design-tokens/admin-shell/admin-ui/web-fonts，Xocs公开实现边界保留；Foundation有组件状态与错误定义 | 实际浏览器字体/间距/反馈、未知/失败/未应用配置/取消状态、更新前后发行契约以及服务端权限回归未在本次全量执行。引用相同package不证明交互已一致 |
| 十七、17.1–17.6 摄像头 | xcoc `device.rs/onvif.rs` 有RTSP、ONVIF、identity/streams/PTZ适配；Server保留业务视图而不持设备secret。现有多厂商适配仍基于协议和具体代码 | 当前Client `CapabilityStatus` 与Server `CapabilitySupport` 为supported/unsupported/unknown三态，Client设备边界实际保留未知与不支持区别；配对/快照edge-v4与浏览器wire-v3、Schema8分别维护；历史能力转换不属于本轮范围。用户明确“无真实摄像头，跳过这个”：本轮真实厂商/型号/固件设备验收跳过且不阻塞。源码/模拟能力与真实支持证据保持区别，不扩大品牌支持声明 |
| 十八、18.1–18.8 升级 | `xssc/src/upgrade/mod.rs` 实现阶段journal、停服、共同独占锁、持久门、快照、签名目标、原子切换和业务就绪；start-intent后默认拒绝丢失新写入的恢复 | 本轮仅要求当前版本到未来版本的升级；产品真实签名制品、current程序原子切换、完整state_paths备份恢复和root→service UID路径仍需实际执行。历史Source adapter/Archive schema转换不再作为交付要求；旧输入仍明确拒绝 |
| 十九、19.1–19.5 构建发行 | 精确工具链、Cargo.lock、旧发布rev、不可变Web tarball、releaseidentity与签名机制在仓库存在；Foundation package LICENSE24/24实测 | 本地输入已经真实commit/tag/bundle/8tgz闭合，六产品Server输入0.10.6、Upgrade行政桥输入0.10.7、Client输入0.9.18，产品实际Cargo/NPM无相邻path或旧公共版本混合。正式输入仍需官方新revision/tag/URL可取、远端CI与独立正式发行物检查。产品manifest应与锁统一；Xocs已补真实profile/build recipe并整理根workspace，不能用双锁副本绕过检查。不得改旧tag或用本地成功冒充发行 |
| 二十、20.1–20.5 CI | 现有CI固定toolchain/actionrevision/runner/timeout，Foundation policy与54工具tests实际过；公共流程由上游实现，产品independence workflow存在 | Foundation44浏览器场景已验证；四Client完整policy均通过，六Server source policy按最终manifest复核。Xocs CI已固定完整action SHA、精确工具链与官方固定rev conformance，release绑定真实HEAD。本轮未运行远程CI、全部产品完整浏览器矩阵或官方independence；最终流程必须消费新的不可变输入，不能放宽path/release检查来制造绿色。需要确认release与用户实际二进制闭包一致 |
| 二十一、21.1–21.5 测试 | 本次公共测试针对结构/秘密/FIFO/flock/WAL/HTTP/rotation等具体风险，代码行为实际执行而非仅字符串检查；产品接入由产品另测 | Foundation tests不替代六产品全部路径；模拟systemd、测试Router、模拟设备与真实平台区分。Windows/macOS/移动端、实际发行制品未执行部分不能标完成；真实设备本轮按用户要求跳过 |
| 二十二、22.1–22.4 文档 | Foundation新机制说明与本记录标明未发布和支持边界；四Server新增CLI说明、XocsREADME改为真实官方固定候选输入及根workspace，Sentinel当前native/docs同步0.4.1与显式初始化 | Xczs说明已区分SQLite与静态账户；原path混合事实已被当前不可变pins替代，文档仍明确候选未远端发布。Sentinel新current/systemd私有JSON部署与旧native平面env入口范围明确；所有源码修改后的最终文档/实际制品核对仍不可省略 |
| 二十三、23.1–23.4 交付 | 本记录按源码/公共测试/产品接入/实际用户路径分层，明确尚未交付全部目标；没有因缺设备/发布证据而删目标 | **整体未完成**。必须冻结所有源码并通过真实init→run→state保持→错误→升级/恢复用户路径、单binary资源闭包、受控pins、支持声明证据；不得把表格已填写当完成 |
| 二十四、24.1–24.5 管理 | `发展.md`保留集中权威；上游先实现公共机制再产品接入；本次记录差距与owner，不创建Foundation产品白名单；已有重复机制开始删除 | 尚未完成的官方来源发布、原生平台与支持声明须给明确退出条件；持续修复新的风险，不用无限“临时”保留第二套配置/日志/升级或历史runtime兼容 |

- [x] Foundation本地受控输入：Server0.10.6产品输入/0.10.7行政输入与Client0.9.18真实commit/tag/bundle与新版本锁、资产和校验记录闭合；旧候选未冒充正式发布。
- [ ] 官方发布与产品闭包：新revision/tag/URL远端可取并通过远程CI；全部产品独立官方checkout/fetch构建、最终Cargo/npm锁与manifest/releaseidentity一致，验证真实发行物。
- [ ] 六Server实际二进制：help/version不修改环境；init只接受明确授权和空目标；run坏配置/错误schema/未知数据不重建；config validate源字节不变且读当前WAL；status校验正确服务真实ready；机器stdout仅一个记录。
- [ ] 所有HTTP拒绝入口：JSON/Query/Path、413、unknownroute及业务权限错误遵循共享语义、request_id与no-store，不泄露提交值或内部链。
- [ ] 运行权与退出：运行、在线写诊断、升级共用同目录身份/锁/pending；正常退出先关闭业务任务/DB/文件再交接；root行政操作新文件保持实际service UID/GID，目标执行树由root持有、服务不可写，工作进程实际降权。
- [ ] 升级与恢复：六产品支持范围清楚；当前版本到未来版本的实际签名制品程序升级、阶段中断、坏备份、错release、启动后新写入保护、显式授权数据损失恢复均有对应证据。
- [ ] Sentinel三态能力：identity、连接状态和confirmed/unsupported/unknown分开；Client/Server/协议/Web/schema一致，无旧runtime兼容fallback。
- [ ] Sentinel资源边界：ffprobe双输出cap并发读取+timeout/越限kill/wait、发现数量和累计设备命令/结果队列上限、单实例阻塞不扩散，达到上限真实表达状态。
- [ ] 日志：所有普通运行日志采用共同结构，公共template不能改写；实例/时间/等级/事件/关联精确查询；文件与外部服务管理器保留明确；writer失败可观察；Windows服务链不明文输出。
- [ ] 实际制品单二进制：嵌入必要前端资源、无开发目录/补下载业务包依赖；Xocs公开实现保留；Sentinel外部媒体依赖声明和就绪真实验证。
- [ ] 浏览器与平台：用户界面权限/错误/配置应用/取消状态真实；当前声明的Windows/macOS/移动端原生验证及Linux服务路径都有相应证据，不把交叉编译当原生验收。
- [x] 本轮真实摄像头验收按用户“无真实摄像头，跳过这个”要求跳过；无真实支持证据，不把模拟覆盖作为品牌/型号/固件已验证，也不将该项作为交付阻塞。
- [ ] 文档闭环：当前quickstart/config/deploy/upgrade用法与最终代码/发行物一致；过时入口不继续推荐；说明未完成项，Maple仍未修改。

1. 公共配置只用 NOFOLLOW 打开可能在FIFO上无界等待，已加NONBLOCK并以真实mkfifo回归通过；缺文件、unsafe文件、读取失败、读取中改变使用不同稳定code，诊断不回显敏感值。
2. 原私有只读SQLite读取有忽略当前WAL或在原库创建SHM/恢复journal风险，现公共临时副本明确锁、身份、摘要和预算，SQLx仅打开副本。必须坚持独立诊断进程及“原始FD全部先关闭、随后打开SQLx”的锁约束。
3. 仅Drop close在fork短时继承情况下可能延长维护锁持有，已提供显式release并以重复FD验证交接；该已测的旧子迁移委派行为仅保留为公共机制历史证据，不再是当前产品交付门；当前升级仍必须独占维护锁并严守pending门。
4. tracing公共event曾经过产品event构造而被拒绝，现统一registry模板，未知common拒绝；动态typed sink切换、定制log filename轮转与身份变化失败计数已实测。
5. Sentinel原bool能力和探测/累计队列上限不满足17.3/14.1，现三态、ffprobe双管道预算、ONVIF数量和累计任务预算已有真实源码；真实多厂商验收按用户要求跳过；仍如实限制支持声明。
6. 多个Client Windows服务错误直接打印raw链，已交负责人统一portable结构化事件；当前Linux公共测试不能证明Windows ACL、服务停止或日志保留已正确。
7. 新机制的源码开发路径不能复用已发布v0.10.4的发行证据，也不能把已执行公共54/21工具tests、48风险tests计数当作新制品与六产品用户任务已完成。

- Xocs当前debug程序的`10组CLI验收`全部通过，报告记录实际binary SHA。覆盖help/version零写、坏密码与未知数据拒绝、重复init全树身份与字节不变、readonly config/provenance、错误低层不可CLI遮盖且不回显、WAL-only坏管理员hash拒绝且原main/WAL/SHM全字节/mode/inode不变、真实wrong-service status拒绝、pending拒绝、第二进程互斥与正常SIGTERM/重启数据保持。这与公开/会员/管理员业务browser smoke是不同验证，不重复计数；正式source-bound制品须另验。
- Sentinel修复根目录单叶暂存后，真实debug二进制`unbound relocation烟测`通过，显式init与全部嵌入资产/MIME/HEAD/ETag/缺失404/开发覆盖拒绝均实际执行。`另7组CLI验收`使用真实官方pinned MediaMTX（版本/hash校验），空摄像头实例应用达到实际ready，status、第二进程锁、HTTP401请求ID/header/body/no-store与结构化UTC/common退出生命周期/按request_id日志关联均通过。没有真实摄像头或原生平台支持声明。
- `native/lifecycle-test.sh`是模拟fixture，通过仅证明shell生命周期协议，不算真实binary初始化或正式发行。原生包producer的成功、已有archive/SHA保持、symlink拒绝、link/fsync失败只rollback本次inode与8进程竞争唯一赢家已实际验证；正式native/fullroot源绑定包正在构建。
- Foundation0.10.7行政桥新增明确owner/root入口和实际服务uid/gid的新锁/pending，保持ordinary exactuid。新21项直接测试加父测试实际执行65534/65533helper、两crate alltargetsClippy/doc与54Python、8真实新tgz/41exports独立安装通过；`本地发行记录`封存。六产品与Client中立log保留0.10.6，只有Upgrade采用0.10.7；这是19.2要求的可追溯适用差异，不是混乱版本或漂浮path。真实Root→服务uid完整升级演练由Upgrade负责人执行，尚未以公共单测代替。

用户明确只考虑xssc当前版本→未来版本。历史Source适配、Archive schema转换、旧版升级样例和历史state迁移入口不再属于本轮要求，也不得留在当前推荐路径。已执行的旧转换测试仅作原候选事实记录，不计入新版本交付通过；当前数据拒旧、备份完整性、坏备份拒绝、start-intent后新写入保护、原子程序/current切换和UID隔离仍须实测。

## 发行准备/README.md中的相关复核快照

| 仓库 | 候选版本/tag | 完整源码提交 |
|---|---|---|
| Foundation Server（六产品/Client中立日志） | 0.10.6 / v0.10.6 | `c0dfab08e54ad9df9c884fa31207f19b07a8e501` |
| Foundation Server（Upgrade行政权限桥） | 0.10.7 / v0.10.7 | `2e46ac5c7660db30fbf6b89a1922fb9ce7fe8fc4` |
| Foundation Client | 0.9.18 / v0.9.18 | `630bb16fe317c41c169cbfa7442cd06cce77257f` |

0.10.7为Upgrade新增明确行政uid桥，普通运行时exact-owner检查保持；六产品无需更新到此版。21项实际风险测试和真实65534/65533子进程、两crate Clippy/doc、54项Python与8个新包/41exports独立安装通过。Web代码/第三方图未改变，明确继承0.10.6浏览器证据，没有再次跑全套。新的`验证记录`、`真实发行树`、`bundle`和`SHA256`均封存，旧0.10.6资产保持原样。

发布成功后，各产品必须从官方来源独立checkout/fetch这些精确revision和新归档、更新锁及发行identity，并重新执行实际二进制/升级用户路径。Windows/macOS/移动端原生运行未在本机执行；真实摄像头验收本轮按用户“无真实摄像头，跳过这个”要求跳过，不扩大支持声明。

## 当前产品升级支持范围

按用户最新指示，本轮xssc只考虑当前版本到未来版本。历史Source适配、旧Archive转换和旧版本升级验收不再是交付要求；当前产品仍严格拒绝旧数据，并保留签名输入、停服/独占锁、当前状态完整备份、程序/current原子切换、失败恢复和root/service UID安全边界。

1.0.0 使用当前源码及 GitHub Actions 的验证结果。文中旧版验收路径是历史记录，未随独立仓库发布；当前发布说明见 [1.0.0](releases/1.0.0.md)。
