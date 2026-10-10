# xcsc 输入校验

xssc 作为客户端工具只消费一个 `xcsc` Cargo 包，启用 `offline-maintenance` feature，固定官方 URL `https://github.com/isarmg/xcsc`、精确版本 `=1.0.0` 和完整 revision `00770c007912b276f5bb1075abfefe3c31026276`。`xcsc::contracts`、`xcsc::state_file`、`xcsc::fs_safety`、`xcsc::log` 和 `xcsc::sqlite` 是包内模块。Cargo 正常、build、dev 及锁文件依赖图不能包含 xcss 或旧拆分包。

`xcsc-client.toml`、`Cargo.toml` 与根 `Cargo.lock` 必须表达同一来源和客户端角色。`offline-maintenance` 仅为 Linux 离线管理提供状态锁、文件安全、日志和有界 SQLite 支持；工具拥有升级编排、签名策略、快照和恢复状态机。持久化数据中的 xcss 锁文件、表名和合同标识属于服务器当前数据协议，按原合同读取。

CI 和 Release 的客户端来源任务检出完整 revision 的官方 xcsc，运行其 `scripts/check-xcsc.py`。检查既验证客户端产品 ID，也验证实际锁图，不接受路径替代来源或客户端直接、间接消费 xcss。

取得上述完整 revision 的官方 xcsc 源码后，本地复核命令：

```sh
cargo metadata --locked --format-version 1
python3 /absolute/path/to/xcsc/scripts/check-xcsc.py --product-root /absolute/path/to/xssc
```

第一条命令读取真实 Cargo.lock 依赖图，不更新锁文件；第二条运行与 CI 相同的官方客户端角色和来源校验，确认产品描述、版本、revision 和实际依赖图一致。历史[0.4.1 输入记录](validation-logs/xcss-source-policy.json)只证明当时的 xcss 0.10.8 输入，不作为当前 xcsc 或本产品签名资产的验收证据。当前产品按最终源码独立通过原生测试、签名发行和解包验证。
