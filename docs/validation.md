# 1.0.0 离线文档闭包验证

0.6.0 正式发行 [37650833489](https://github.com/isarmg/xssc/actions/runs/37650833489) 已实际成功，Source `2baff9f0ee6c17269aab4968f1f2b89044481edc`。两项公开资产回下载后与 GitHub SHA-256/size 一致；解包全文件清单、独立源公钥 Ed25519、DER 指纹、catalog/release/provenance 的版本/源码/target、SBOM 及源脚本/文档逐字节核验通过。macOS 没有执行 Linux 二进制；实际 binary support/version 的源绑定由 Linux 正式发行门执行。回下载检查发现准备说明未封装及根相对链接错误，因此由 1.0.0 修复，原 0.6.0 资产封存。

1.0.0 的本机回归实际通过 6 项文档封装测试及 4 项签名前身份/文档拒绝测试。测试运行真正的文档封装器，从当前源文档生成包、归档再安全解包，检查准备说明、链接和锚点，同时检查嵌套引用/资源、循环及破坏输入。使用隔离 Bash 5 补齐系统 Bash 3 的平台限制；Linux AMD64 全 targets/features Clippy、fmt、官方 verify-source、工作流供应链策略及 Bash 语法检查通过。交叉编译没有执行 Linux 二进制；正式 Linux CI、签名后解包和回下载结果须对应本补丁的真实 Source 记录。

以下 0.6.0 候选与更早证据保留原版本范围，不计为本补丁的新验证。

# 0.6.0 当前候选验证

本轮在 macOS 上使用官方 Rust 1.99.0 与固定 Foundation 1.0.0 / `d58b9ef0822984ee0d29fb8b8139cfd2787374fb`：`cargo fmt --all -- --check`、官方 `verify-source`、workflow supply-chain 两个工作流与负向策略检查、Linux AMD64 目标 `cargo clippy --locked --all-targets --all-features -- -D warnings` 已通过。交叉编译只验证 Linux 目标静态代码；没有将其当作 Linux 原生锁或业务运行证据。

新 Sunshine 离线准备新增六项风险测试，包含精确旧观察形状、真实 SQLite 事务和业务保留、坏结构拒绝、完整备份、源接口/摘要/身份拒绝、common/DB 锁冲突及写入意图记录。它们由现有 Ubuntu CI 的全部 targets/features 测试运行；当前原生结果等待该源码 CI，源接口和停服夹具不代表真实 systemd 或设备。

本机 Apple LibreSSL 不支持 Ed25519，且系统 Bash 3 无 `readarray`，签名打包 Python 测试不能在这里完成：stage 测试停在 Ed25519 密钥生成，finalize 的 3 项中 2 项通过、1 项停在 Bash 平台要求。保留 Linux/OpenSSL 执行条件及正式质量门，在原生 CI 复验，不为本机放宽签名或平台要求。

以下历史结果保持其原版本事实，不计入 0.6.0 的新测试计数。

# 当前离线升级验证

本记录只计入最新范围：操作者先停止当前 Server 和全部数据 writer，辅助工具完成同结构发行物切换及原完整组恢复。历史转换、撤回候选、源码审阅和尚不存在的未来制品不计为通过证据；迁入的旧事实单独封存在 `validation-context.md` / `legacy-location-notes.md`。

## 固定输入

原始机制及真实产品验收使用 Tool `0.4.0`。四个直接 Foundation crate 全部固定官方 Git URL、`=0.10.8`、完整 revision `6c6206cf7df047fb8b4c72689fbba6495e2e7968`。Cargo.lock 中102个registry包的名称/版本/source与上一精确锁图逐项相同。Tool自身通过std-only build.rs记录实际编译target及严格40位源码修订，开发态明确unbound；正式stage必须与冻结HEAD一致，finalize在签名前逐项核对实际binary支持/版本及provenance。早期本地校验使用受控缓存解析；随后官方签名发行实际成功，见下节的发行证据。

## 已执行的机制验证（2026-10-07）

| 验证 | 实际结果与范围 |
|---|---|
| 全targets/features Rust tests | 28 passed、3 ignored；2真实产品opt-in及1UID子入口分别标明 |
| 实际 UID65534 行政夹具 | 包含在27中；root持有完整Root/current，serviceuid700/600数据、清补充组/降giduid、固定ELF FD4、不能写Root/selector、完整切换与授权恢复保留业务UID/GID/内容 |
| 已停服务检查 | LoadState=loaded、MainPID0、inactive/failed；缺失/重复/未知/not-found/error/transitional/非零PID拒绝，活动主服务/额外writer在journal/gate之前拒绝 |
| 当前软件准入 | 签名 source_identity 的 version/fullrev/state合同逐字段与当前程序核对；不符在协调锁、journal、gate之前拒绝，真实绑定Xocs亦核验通过 |
| 完整目录诊断覆盖 | 签名变量仅接受大写 *_RELEASE_ROOT；Root从固定ELF和entrypoint导出，实际子进程只收到PATH/LANG/唯一Root变量；任意环境字段拒绝 |
| writer角色签名 | 额外roles与实际unit映射不可缺失、多余、重复；签名包含roles，停止后多阶段复查 |
| 完整Root | 真文件/树/签名、Python与Rust摘要一致、已知原inode损坏修复、中断续接、未知selector/dangling link拒绝 |
| 恢复与新增写入 | 部分快照不使用、损坏快照拒绝、配置/数据/程序整组恢复、原结构只读核验、交接后需明确data-loss授权、恢复启动后亦保护新写入 |
| 有界子进程 | stdout/stderr双管道并行上限；超限/timeout实际kill+wait且PID不存在，秘密stderr不进入普通错误 |
| HTTP就绪身份 | 实际临时TCPlistener覆盖唯一正确头、大小写、缺失、错误、重复相同/冲突身份；任何重复均拒绝 |
| Clippy | alltargets/allfeatures、-D warnings通过 |
| 签名打包Python | 8 passed；完整artifact、模式/链接/额外字段拒绝、identity/trust、writerroles界限 |
| 工具发行identity Python | 3 passed；实际字段/hash/编译source/target/provenance错在打开签名私钥前拒绝 |
| workflow策略 | 固定action/runner/权限/credentials正向及负向通过 |

完整日志：[Rust机制测试](validation-logs/final-0.10.8-tests.log)、[Clippy](validation-logs/final-0.10.8-clippy.log)、[真实当前Xocs](validation-logs/actual-current-product-tests.log)。

测试使用模拟服务管理器观察及故障注入，同时真正执行签名、文件、权限、锁和恢复。它们不修改系统服务、不创建系统用户，不当作真实systemd验收。

## 真实当前产品

演练所用正式 Xocs `3.1.2` 的 full source SHA 为 `217df0807c2f265f4cf5613fa102c41a2dbb5ce3`，source-bound ELF SHA256 为 `0e6b282107cc5edd44ecb146e32406272fbf4f159f8fe2486e9abe0dd4c6d4c1`，状态合同摘要为 `d5e51d08e80bdb84bcfa10c3561f33920e1f0bcc6d98f0ef4e5d134fa471bbad`，目标为 `x86_64-unknown-linux-gnu`。受控制品路径为 `/mnt/sarmg.org/发行准备/poetize-3.1.2/xocs-source-bound`。

两个真实当前产品opt-in已经执行：**2 passed**。它们从当前 `init` 新建数据库，保存会员、文章、管理员、审计及其他当前普通表的全部行和媒体字节；以UID65534普通校验/运行，核对真实PID、exe路径/SHA、UID和HTTP业务ready。一次完成当前同结构重装；一次在目标校验注入配置与媒体损坏后，恢复原程序/配置/完整数据库/媒体及属主，再由真实当前原程序提供ready。测试拒绝unbound身份，没有伪造identity fallback、历史DDL或历史程序。

该演练使用相同实际当前发行物作为源和目标；未把尚不存在的未来软件版本当成通过证据。完整Root机制已通过文件/权限/中断恢复测试，未来不同完整Root版本需要在真实发行时验证。测试仅替换systemd管理器观察；程序、数据、服务UID和业务HTTP检查实际运行。

## 未执行边界

真实systemd的停服/启动与companion进程生命周期、六产品不同未来版本的正式完整制品切换仍需相应受控环境和实际发行资产。Source审阅、当前同版本重装和本地缓存解析不能替代这些证据。

## 官方签名发行与 0.4.1 输入修正

Tool `0.4.0` 官方签名发行 [run 37596210367](https://github.com/isarmg/xssc/actions/runs/37596210367) 实际成功，Source `b15df0a0f83c8135635188bfe0744f322e798952`。官方制品下载后以仓库既有独立 Ed25519 信任锚核签，并验证所有清单/摘要和实际 binary support 的 source/target/version。封存`官方验收收据`记录 archive SHA256 `c62bf726998647f6895657663ca12804842c64849443ff2fd3e0b290c3bb6a63` 与 ELF SHA256 `f3554b9a4364f7e0ea49765d0a074d6d103d875843d648cba46111e845d73210`。本地未签名制品保留且与官方字节身份分开。

`0.4.1` 仅修正声明和发行 CI：旧 `xcss-product.toml` 的 0.10.3 声明漂移到实际唯一 0.10.8，并删未消费的 secret-envelope 能力；新增官方精确策略的 verify-source 必需门。Cargo registry 包和生产业务机制均未改动，先前 Xocs 3.1.2 E2E 保持原 Source 事实，不冒充后来 Xocs 3.1.3 或新 Tool Source 的 E2E。当前来源正向/负向记录见[输入验证](foundation-input-validation.md)。0.4.1 新正式 Source/CI/资产在成功后另存其版本目录。


## 0.4.1 正式发行与最新当前产品实测

`0.4.1` 冻结运行源码为 `ec147584f99edca6e9333298b707066a09778dd3`，annotated tag object 为 `2ced9246552c6b9486439cb2d8a8c855d457d6cc`。主 CI [37605345982](https://github.com/isarmg/xssc/actions/runs/37605345982) 与正式发行 [37606281761](https://github.com/isarmg/xssc/actions/runs/37606281761) 全部成功，必需的官方精确 Foundation 输入门实际通过。下载官方资产后，使用冻结源码中的原有 Ed25519 公钥独立核签，逐项核验包内清单及 ELF 实际编译 source/target/version、目录 catalog 与 provenance；未更换信任锚。archive SHA256 为 `139940430ce51579c544bea07e6f016e8c213ed3a36f52d76b269943c5094ba5`，ELF SHA256 为 `da5aa45b3d0567452729b6982d80af745821f02ca9aeeb7ba9eac671dfc1505b`。完整`官方收据`和全部资产封存在本仓库版本目录，0.4.0 旧证据未覆写。

同一 0.4.1 运行源码还重新执行了两条实际当前产品 opt-in，**2 passed**。产品使用官方 Xocs `3.1.3` 归档，Source 为 `2d0f2d26e2c95af6cfe44375e0de2befb20f24c0`，ELF SHA256 为 `ab59db6ee4845e834677e1465cfdb6eb47b176dec7537b89bc7778131933e891`。从该版本的当前 init 建立数据，再以实际 UID/GID 65534 验证正常同合同重装的业务 ready、全部业务表/媒体及属主保留，以及目标校验失败后原程序/配置/数据整组恢复与真实 ready。`实测收据`明确记录 Tool 和产品实际 Source，以及受控测试仅替换服务管理器、使用临时夹具签名的边界。它不代表不同未来版本或真实 systemd companion 生命周期已经验收。

Tool 的产品目录不硬编码这些软件版本；签名 source_identity 才逐项绑定操作者实际当前软件、完整源码 revision、target 和状态合同。未来 schema 改动必须在真实发行时新增明确转换，当前不能绕过同合同拒绝规则。

1.0.0 使用当前源码及 GitHub Actions 的验证结果。文中旧版验收路径是历史记录，未随独立仓库发布；当前发布说明见 [1.0.0](releases/1.0.0.md)。
