# 构建与测试

开发环境使用 Linux x86_64 GNU、Rust `1.99.0`、Python 3、Bash 和 C 编译工具链。签名脚本测试需要支持 Ed25519 的 `/usr/bin/openssl`；完整封装还使用 Zstandard 和 GNU tar。

## 构建开发程序

在仓库根目录执行：

```sh
cargo build --release --locked
./target/release/xssc support --json
./target/release/xssc apply-upgrade --help
```

普通构建明确标识为未绑定正式源码身份，用于开发验证。正式操作程序由[发行流程](operations.md#发行)绑定源码、封装和签名。

## 运行检查

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
python3 -m unittest discover -s scripts/tests -v
python3 tests/test_finalize_release.py
python3 tests/test_release_docs.py
python3 scripts/check-workflow-supply-chain.py
```

Rust 测试验证签名、文件复制、权限、锁与中断状态；Python 测试覆盖制品制作、定稿和离线文档封装。root 环境另覆盖临时 UID 65534 的真实权限分支，普通用户结果单独记录。

## 真实产品演练

机制测试实际执行 Ed25519 验签、完整二进制/发行根目录/配置/数据复制、资源覆盖、模式/属主/链接限制、排他锁、持久阶段及恢复。覆盖完整发行根目录的中断修复、验证器意外写入、超限子进程回收、启动后新写入保护和损坏备份拒绝。实际 UID65534 夹具证明私有 0700/0600 状态可降权校验，root 控制的程序和选择链接不可被服务用户修改，整组恢复仍保留服务属主。

按需运行真实当前产品测试：

```sh
XSSC_TEST_XOCS_BINARY=/absolute/controlled/current/xocs \
  cargo test --locked --lib upgrade::product_tests -- --ignored --test-threads=1
```

测试使用真实当前 `init` 建库、当前结构校验、当前 `run` 和 HTTP 业务就绪，核对会员、文章、管理记录、审计和媒体保留；验证故障后原程序/配置/数据库/媒体整组恢复及真实原程序就绪。此演练使用同一实际当前发行物进行同结构重装，不把尚不存在的未来版本当成已验证制品。

产品测试替换 systemd 生命周期管理，实际程序、进程 UID、exe 路径/摘要与 HTTP 检查不替换。真实产品测试和生产命令均要求真实绑定源码编译发行身份，拒绝未绑定源码的开发程序；纯机制测试的观察替身另行标明。真实 systemd、六产品的正式完整资产和各平台验证另行记录，不能用测试夹具代替。



## 修改文档和接口

文档使用仓库内同一套相对路径。封装器从 `operations.md`、`offline-upgrades.md` 和 `platform-setup.md` 递归收集引用，签名前与解包后检查文件、锚点和路径；增删链接后运行 `tests/test_release_docs.py`。

开发职责见[设计约束](development-contract.md)，公共依赖与来源检查见[公共支撑](common-support.md)和[输入验证](xcsc-input-validation.md)。实现导航见[流程说明](project-workflow.md)。
