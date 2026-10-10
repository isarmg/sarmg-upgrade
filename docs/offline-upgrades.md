# 受签名发行物的离线升级与恢复

`apply-upgrade` 将确认已停服、独占维护、备份、校验、二进制切换、运行权交接和业务就绪检查组织为一个有持久记录的流程。xsos、xszs、xscs、xcos、xczs 和 xocs 通过相同流程接入，产品只提供自身普通只读校验与发行身份；升级定义及制品打包只在本工具维护。

仅支持本轮当前基线到未来版本的升级。当前实现要求源和目标 `SchemaIdentity` 完全相同，由当前程序和封存目标分别验证实际状态。未来真实发行改变数据结构时，再随该次发行增加明确转换；该受签名流程不登记历史结构、不制造虚拟未来结构。当前不提供历史结构适配。未知或损坏状态返回 `STATE_INCOMPATIBLE` 或状态校验错误，并保护原状态。

## 运行条件

- Linux AMD64、systemd 和支持 Ed25519 的 `/usr/bin/openssl`。操作者先停止主服务及所有相关持久数据 writer，并关闭其他管理器自动重启。工具不下载、不停服，也不修改 unit。未确认已停返回 `SERVER_MUST_BE_STOPPED`。
- 六个 Server 的当前程序与未来目标均提供标准 `config validate --config PATH --data-dir PATH --json`、`release-identity --json` 和普通 `state-contract --json`。配置校验必须只读、校验真实结构和业务约束，输出 `schema_identity` 与全部 `state_paths`。
- `state_paths` 包含所有不可重建的持久输入。SQLite 报告数据库父目录以保护 WAL/SHM；外部媒体、密钥或其他数据目录必须落在计划资源内。工具在校验前后对整组内容及属主进行核对，拒绝意外写入。
- 当前和目标 runtime 使用中立状态锁 `.state-maintenance.lock`、`.state-instance.lock`，遇任何 `.state-maintenance-pending.json` 对象均拒绝启动。工具确认已停、设置中性维护门并取得排他维护锁；配置校验不申请写入权。
- 服务的 `/readyz` 返回 HTTP 200、JSON `{"ready":true}`，带与产品身份一致的 `x-service`。关键业务依赖不可用时不得就绪。
- root 可管理独立服务 UID 的物理 0700 状态目录，普通运行者仍必须是目录实际属主。工具验证 systemd 固定 User/Group 与数据 UID/GID 一致，清 supplementary groups 后降权执行产品校验。配置父目录必须由该服务用户持有且可访问，JSON 文件为私有 0600。DynamicUser 暂不支持。
- 全部输入使用绝对规范路径。除下文唯一受控 `current` 入口外，链接、特殊文件、多链接文件、组或其他用户可写对象、重叠保护根均拒绝。恢复保留实际 UID/GID、权限和内容；xattr/ACL、稀疏布局和硬链接关系不在支持合同内。

## 制作受信任升级制品

升级发布者在本工具维护受控产品定义，并从普通产品发行诊断取得严格 `ReleaseIdentity` 及真实状态合同。产品仓库没有升级定义、专用命令或工具依赖。工具定义精确绑定当前软件的普通发行身份，并包含相同源/目标状态身份、必需资源、额外数据 writer 角色及可选完整发行目录：

```json
{
  "source_identity": {
    "product": "xocs",
    "version": "1.0.0",
    "source_revision": "由当前正式程序release-identity取得的40位小写Git提交",
    "target": "x86_64-unknown-linux-gnu",
    "state_contract_sha256": "由当前正式程序release-identity取得的64位小写SHA256"
  },
  "source_schema": {
    "application": "xocs",
    "application_version": "xocs-db-v1",
    "schema_revision": 1,
    "schema_sha256": "由产品实际结构定义取得的64位小写SHA256"
  },
  "target_schema": {
    "application": "xocs",
    "application_version": "xocs-db-v1",
    "schema_revision": 1,
    "schema_sha256": "与源结构相同的64位小写SHA256"
  },
  "resources": [
    {"name": "config", "kind": "file"},
    {"name": "data", "kind": "directory"}
  ]
}
```

定义仅接受 `source_identity`、`source_schema`、`target_schema`、`resources` 和可选 `artifact`、`additional_service_roles`。源/目标不同、额外字段、任意命令或 hook 均拒绝。稳定的数据格式标签可以低于软件版本；保持标签稳定不代表程序来自旧发行。

以上是 xocs `1.0.0` 的字段模板，不能直接作为升级定义执行。`source_identity` 的完整五字段应逐字取自本实例当前正式程序的 `release-identity --json`；源码提交和状态合同摘要不可使用其他发行版本的值。当前状态身份为 `application=xocs`、`application_version=xocs-db-v1`、`schema_revision=1`，结构摘要应取自当前程序的 `config validate --json`。其他产品使用自己的真实当前身份。工具在任何协调锁、journal 或维护门改写之前，以当前程序的 `release-identity` 逐字段核验 `source_identity`。不符返回 `CURRENT_RELEASE_INCOMPATIBLE`，不会以结构碰巧一致接纳其他软件发行。定义与发行输入已经完成校验后，发布者使用集中脚本：

```sh
python3 scripts/stage-upgrade-release.py \
  --binary /absolute/release/xocs \
  --identity /absolute/release/release-identity.json \
  --definition /absolute/xssc/definitions/xocs.json \
  --private-key /absolute/private/signing.pem \
  --trusted-public-key /absolute/product/release-signing-public.pem \
  --output /absolute/new-upgrade-package
```

脚本拒绝既有输出目录、错误平台和与源码绑定公钥不匹配的私钥，生成精确字节签名的 `upgrade-release.json` 与 `upgrade-release.sig`。私钥不进入制品。发行标识、源码修订、目标、二进制 SHA-256、源/目标结构及必需资源定义都在签名范围内。

操作方从独立可信渠道取得并固定产品公钥的 DER SHA-256。不能把下载包自带的公钥或摘要当作信任来源。计划中的公钥与指纹必须来自该预先确定的信任锚。

## 执行升级

创建私有计划文件，填写已经确认的 unit、配置、状态范围、信任锚与全新恢复目录。以下路径是部署示例，必须替换为本实例实际路径；文件已存在、权限已满足、签名已由正式发布者制作才可执行：

```json
{
  "plan_version": 1,
  "service": "xocs.service",
  "installed_binary": "/opt/sarmg/bin/xocs",
  "target_binary": "/srv/xocs-release/xocs",
  "release_manifest": "/srv/xocs-release/upgrade-release.json",
  "release_signature": "/srv/xocs-release/upgrade-release.sig",
  "trusted_public_key": "/etc/sarmg/trust/xocs-release.pem",
  "trusted_public_key_sha256": "独立信任渠道确认的64位小写DER摘要",
  "config": "/etc/sarmg/xocs.json",
  "data_dir": "/var/lib/sarmg/xocs",
  "resources": [
    {"name": "config", "kind": "file", "path": "/etc/sarmg/xocs.json"},
    {"name": "data", "kind": "directory", "path": "/var/lib/sarmg/xocs"}
  ],
  "work_directory": "/srv/backups/xocs-upgrade-unique-id",
  "readiness_address": "127.0.0.1:8080",
  "timeout_seconds": 60,
  "max_backup_bytes": 1099511627776
}
```

额外 writer 使用本工具签名 `additional_service_roles`，按相同顺序映射计划 `additional_services`，例如：

```json
"additional_service_roles": ["mediamtx"]
```

```json
"additional_services": [{"role":"mediamtx","service":"xcos-mediamtx.service"}]
```

主 unit 使用计划 `service`。同名 role/unit、缺失或多余映射、超过 16 个辅助 writer 均拒绝。工具逐个只读确认 `LoadState=loaded`、`MainPID=0` 且 `ActiveState=inactive` 或 `failed`；not-found/error/unknown/activating/deactivating 不能当作停止。设置门、取得锁、快照完成及校验结束时复查全部 unit。xcos 的 MediaMTX 是独立录像 writer，不能只停主服务。systemd 以外的 writer 不在当前自动核验范围，须先停止并保全其输入；工具不运行任意 shell 检查。

资源顺序、名称和类型与签名定义完全一致。外部数据库、媒体或密钥要求对应产品发布定义列出另外的保护根，并在计划中明确映射实际路径；缺失会拒绝升级。`max_backup_bytes` 限制整组旧程序与状态备份，默认 1 TiB；目录深度上限 128、每资源最多 2,000,000 项。交接可能写入标记持久后，工具先启动额外 writer、再启动主 unit 并检查真实 ready。单个命令等待 1–600 秒，stdout/stderr 各限制 1 MiB。

```sh
xssc apply-upgrade --plan /absolute/private/upgrade-plan.json
xssc inspect-upgrade --work-directory /srv/backups/xocs-upgrade-unique-id
```

工具不自行下载程序；所取得的离线目标先验证签名、信任锚、发行身份、目标平台、ELF 架构和完整性，封存到受控执行目录后才调用目标校验器。单程序在已安装程序同一目录暂存并原子替换；完整 Root 使用下述完整目录协议。当前状态采用停服、独占锁下的完整原始快照，主库与 sidecar 同组保存，前后 inventory 与 SHA-256 一致后才标记可恢复。

就绪必须同时证明 unit 活跃、实际 MainPID 运行指定路径及目标 SHA-256、正确产品的 `/readyz` 业务检查通过。进程存在、端口可连或一条退出成功的启动命令都不足以完成升级。

## 阶段与中断

| 持久阶段 | 已证明的完成条件 |
|---|---|
| `prechecked` | 已确认服务停止、验证签名目标并封存，计划与原二进制身份已记录 |
| `maintenance-intent` | 维护意图已记录，维护门保护后续正常启动 |
| `maintenance-acquired` | 所有声明的 unit 仍停止，持有数据目录独占维护权 |
| `backup-started` | 正在备份；部分备份不可用于恢复 |
| `backup-complete` | 旧程序、配置和数据整组 inventory/hash 已验证 |
| `validated` | 源/目标结构与资源范围验证通过，最终只读验证未发生意外写入 |
| `switch-intent` | 切换意图已持久化；检查实际程序 SHA 判断是否已完成原子替换 |
| `switched` | 目标二进制实际 hash 通过 |
| `start-intent` | 交接已持久化；无论后续命令/观察结果如何，均视为可能产生新写入 |
| `ready` | 指定目标进程与产品业务就绪均已验证 |
| `recovery-maintenance-intent` / `recovery-restoring` | 确认所有 writer 已停，设置维护门并持有独占权后，正在恢复原整组状态；允许在门保护下续接 |
| `recovery-start-intent` | 原程序取得运行权，亦可能产生恢复后的新写入 |
| `rolled-back` | 原程序、配置和数据对应，原程序业务就绪通过；或未变更的预检查记录已关闭 |

每次记录写入先同步私有 journal，再进入下一阶段。门和目录均同步。`inspect-upgrade` 重新校验已完成备份，并报告实际已安装程序属于原版、目标版、未知或不可读，维护门是否存在，以及当前状态是否仍与备份相符；未能读取或验证不等于未修改。

## 失败恢复与数据损失授权

出现失败先保留输出和恢复目录。不要手工删除维护门、修改 journal、换回旧二进制或重跑同一升级计划。

```sh
xssc inspect-upgrade --work-directory /absolute/recovery-directory
xssc recover-upgrade --work-directory /absolute/recovery-directory
```

尚未交接运行权时，先要求所有 writer 已由操作者停止，再在独占维护下验证备份并恢复原程序、配置和数据，恢复后用原程序普通只读校验核对结构/范围/无写入，再交接、启动并检查原程序业务就绪。部分备份不会被当作已完成快照使用。未知的现行程序、损坏备份或错误记录均停止恢复，不覆盖现状。

进入 `start-intent` 后，即使启动失败或观察超时，也不能证明目标没有写入。默认恢复返回 `RECOVERY_AUTHORIZATION_REQUIRED`，保留新增数据。仅在操作方明确接受**丢弃备份时刻之后的业务写入**后执行：

```sh
xssc recover-upgrade \
  --work-directory /absolute/recovery-directory \
  --allow-data-loss
```

恢复自己的启动也采用同样规则：若 `recovery-start-intent` 中断，下一次重新恢复不能默认丢弃原程序随后产生的写入。完成的恢复再次执行只报告既有结果，不重复恢复。

## 完整发行目录与权限分工

本工具的受控定义与签名 manifest 提供部署权威。`artifact` 缺省为单 ELF；完整 Root 使用严格 `immutable-release-root-v1` 对象，声明 `root_layout`、`entrypoint` 和树摘要。定义必须来自真实当前产品正式交付方式：包含 Web/MediaMTX 等完整目录的产品不能用 binary-only 制品通过其普通当前状态校验。产品只提供自己的普通诊断和运行入口。额外字段和未知协议拒绝；工具不根据软件版本猜数据格式。


完整 Root 的签名定义新增 `artifact`，由本工具受控定义声明 `protocol`、`root_layout`、`entrypoint`，打包工具使用 `--release-root` 计算并补入 `tree_sha256`。目录和文件模式逐项参与签名，允许产品原有 0755/0644 或 0555/0444 模式；必须同一可信属主，拒绝组/其他用户写入、特殊位、特殊对象、硬链接和任意 symlink。不能把产品 mode 清单改成另一组值来满足升级工具。树 hash 是 `immutable-release-root-v1\n` 后接 UTF-8 紧凑 JSON：DFS preorder、每目录兄弟项按文件名排序、根相对路径为空字符串，每项字段精确顺序 `path,directory,mode,bytes,sha256`。目录 bytes=0/hash空，文件 hash 是原字节 SHA256。单棵树上限 200 万项、128 层及计划 byte budget。

完整目录可选签名字段 `diagnostic_release_root_env`，例如 xcos 已有的普通 `XCOS_RELEASE_ROOT`。只接受长度不超过64的大写 `*_RELEASE_ROOT` 名称。工具清环境后，仅在 `config validate` 注入这个变量；值由已固定执行文件和签名 `entrypoint` 导出对应物理 Root，操作者不能另填值。没有任意环境或 shell hook。xsos、xszs、xscs 的普通配置校验不需要该字段。

计划新增唯一部署选择对象，示例：

```json
"native_release": {
  "current_link": "/opt/isarmg/xcos/current",
  "source_root": "/opt/isarmg/xcos/releases/当前实际版本",
  "install_root": "/opt/isarmg/xcos/releases/未来实际目标版本"
}
```

`installed_binary` 必须为 `current_link/entrypoint`，输入 Root 与安装 Root 都必须保留签名 `root_layout` 的物理后缀。只有名为 `current` 的受控单跳绝对 symlink 可选择其同级 `releases` 中声明的两棵物理树；重复斜线、`./`、`..`、任意别名和未知对象拒绝。原 unit 在初始化部署时必须已经使用该受支持入口；工具不改 unit，不给未知旧 CLI 包装壳。封存和验证整棵目标后，工具将完整目录以 NOREPLACE 发布，然后原子替换 `current` 并同步目录。已有目标目录必须逐项一致。当前父目录身份与原 Root inode 记录在 journal 中，另一数据目录也不能并行切换同一部署。

原完整发行 Root 和全部持久资源属于同一个已完成备份组。恢复保留原 Root；若原 inode 仍在但资产损坏，先持久化修复意图，将损坏树移到 `.upgrade-displaced-original-操作ID` 留证，重建原树并切回 selector。发生在移开原 Root 后的中断，可凭固定原 inode/持久意图/完整备份继续；未知替换 inode、dangling link 和损坏备份均拒绝覆盖。未完成发布的暂存树保留，不猜测删除。

对于 root 管理程序目录、独立服务 UID 持有数据的部署，工具采用 xcsc 1.0.0 的 Client 离线维护与文件保护入口；被维护产品仍实施既定运行属主与权限规则。行政入口仅允许实际属主或 root，数据目录必须物理 0700，root 创建的维护锁/门继承目录实际 UID/GID 和 0600。主 systemd 固定 User/Group 必须与数据目录属主一致，DynamicUser 暂不支持。产品 config JSON 也必须供实际服务 UID 读取；工具不把管理员环境或 root-only EnvironmentFile 内容偷偷注入验证进程。

journal 和私密备份保持操作者 0700。为独立服务 UID 执行只读验证，公开发行闭包封存在安装父目录下 root 持有的 `.xssc-execution-操作ID`；它及祖先可遍历但不能由服务 UID、组或其他用户修改。工具先 NOFOLLOW 打开固定 ELF FD，再继承 FD4执行；清 supplementary groups、切实际 GID/UID，并清环境。父进程保留排他维护锁，子进程只校验当前状态；没有迁移授权通道。持久资源另外记录逐项 UID/GID，恢复保留原属主、权限和内容；协议锁/门保持原 inode。辅助命令 stdout/stderr 同时非阻塞读取，各自最多 1MiB，超限/超时 kill+wait，stderr不进入普通错误说明。

## 验证边界

机制测试实际执行 Ed25519 验签、完整二进制/Root/配置/数据复制、资源覆盖、模式/属主/链接限制、排他锁、持久阶段及恢复。覆盖完整 Root 的中断修复、验证器意外写入、超限子进程回收、启动后新写入保护和损坏备份拒绝。实际 UID65534 夹具证明私有 0700/0600 状态可降权校验，root 控制的程序和 selector 不可被服务用户修改，整组恢复仍保留服务属主。

按需运行真实当前产品测试：

```sh
XSSC_TEST_XOCS_BINARY=/absolute/controlled/current/xocs \
  cargo test --locked --lib upgrade::product_tests -- --ignored --test-threads=1
```

测试使用真实当前 `init` 建库、当前结构校验、当前 `run` 和 HTTP 业务就绪，核对会员、文章、管理记录、审计和媒体保留；验证故障后原程序/配置/数据库/媒体整组恢复及真实原程序就绪。此演练使用同一实际当前发行物进行同结构重装，不把尚不存在的未来版本当成已验证制品。

产品测试替换 systemd 生命周期管理，实际程序、进程 UID、exe 路径/hash 与 HTTP 检查不替换。真实产品测试和生产命令均要求真实 source-bound 编译发行身份，拒绝 unbound 开发程序；纯机制测试的观察替身另行标明。真实 systemd、六产品的正式完整资产和各平台验证另行记录，不能用测试夹具代替。

历史实现曾删除预基线的独立 `backup-current`、xszs 和 keyed SQLite adapters：它们硬绑定旧配置文件名/结构，不能验证本轮当前私有 JSON 与产品合同。删除依据是合同不匹配，不是数据版本数字小于软件版本。当前 `1.0.0` 的整组快照和中断恢复由以上统一流程完成；当前唯一发布标签为 `v1.0.0`，历史验收记录仅供追溯。
