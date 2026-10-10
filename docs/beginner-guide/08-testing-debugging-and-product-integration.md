# 8. 测试、排错和产品接入

机制测试以真实签名、文件复制、权限、锁和故障为证据，生命周期管理器可替换。产品集成测试运行真实当前 init/validate/run/ready，核对业务行和媒体，不能使用虚构迁移替代。

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
python3 -m unittest discover -s scripts/tests -v
```

实际 UID65534 的 root 夹具验证私有状态、清 supplementary groups、固定 root-owned ELF、selector 保护及逐项属主恢复。普通非root CI 跳过该特定身份分支；本地真实root测试结果另行记录。

排错先读取 `inspect-upgrade`，核对 phase、完整备份、实际已装程序/Root、门和修改状态。错误码稳定，内部错误链和敏感 stderr 不进入普通输出。不能删门、手改 journal 或强制重跑。

本工具维护普通产品诊断接口、当前真实状态与完整 state_paths 的受控接入定义，并按实际发行目录制作签名制品；不复制公共锁/日志实现，不让 xcss 识别产品业务。真实结构变化只有到那次发行时才实施转换并验证失败恢复。
