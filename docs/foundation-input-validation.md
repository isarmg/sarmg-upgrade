# Foundation 输入校验

1.0.0 的五个直接 Foundation 依赖为 `xcss-contracts`、`xcss-state-file`、`xcss-fs-safety` 、`xcss-log` 和 `xcss-sqlite`，统一固定官方 Git URL、精确 `=1.0.0` 和完整修订 `d58b9ef0822984ee0d29fb8b8139cfd2787374fb`。`xcss-product.toml`、`Cargo.toml` 和根 `Cargo.lock` 必须表达同一输入。Foundation v1.0.0 已正式发布；发行归档、校验和、发布资产树与源码身份均已核验。当前工具仍通过自身最终源码的正式 CI 和签名发行门禁后发布。

本轮依赖选择配合 Rust 1.99.0 与 SQLx 0.9 生产及测试图，使用同一 Foundation 修订的完整依赖闭包。`offline-tool` 声明显式路径、私有状态、恢复日志和 Linux openat2；生产工具没有消费 secret-envelope 实现，不声明该能力。

CI 的 `foundation-source` job 将工具源码与精确 Foundation 策略分别检出到两个同级目录，运行官方 `verify-source`。这样策略源码不会被当作产品输入扫描。正式发行的 `build` job 必须等待该检查通过，随后既有签名及独立验证流程保持。

本地复核命令：

```sh
python3 /absolute/path/to/foundation-1.0.0/scripts/xcss-conformance.py \
  verify-source --product-root /absolute/path/to/xssc
```

检查实际 manifest Profile、直接依赖的 URL/revision/version，以及 Cargo.lock 的完整来源身份。[0.4.1 输入验证记录](validation-logs/foundation-source-policy.json) 保留其当时 0.10.8 输入和负向回归事实，不作为 0.6.0 当前输入验证或正式签名资产验收证据。既有 tag 和资产不修改。
