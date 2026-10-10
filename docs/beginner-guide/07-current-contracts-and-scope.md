# 7. 当前支持范围

| 产品 | 统一发行物升级与整组恢复 | 当前状态权威 |
|---|---|---|
| xsos | 支持 generic 同合同流程 | 当前产品 config validate |
| xszs | 支持 generic 同合同流程 | 当前产品 config validate |
| xscs | 支持 generic 同合同流程 | 当前产品 config validate |
| xcos | 支持 generic 同合同流程 | 当前产品 config validate |
| Xczs | 支持 generic 同合同流程 | 当前产品 config validate |
| Xocs | 支持 generic 同合同流程 | 当前产品 config validate |

仅当前基线到未来版本。相同结构先校验再切换；实际未来结构改变随对应发行增加明确实现。本轮不解析缺可信身份的数据、不登记历史软件版本转换、不构造虚拟未来 schema。

完整 Root、外置密钥和数据必须按产品标准接口和签名定义覆盖。机制支持不等于所有产品正式资产已完成实机验收；自动/真实产品/真实systemd/远端CI证据分别记录在[验证台账](../validation.md)。

旧独立备份 adapters 因硬路径/输入和结构不匹配当前合同删除；不是因为状态标签数字小于软件版。统一升级里的完整快照、中断恢复和新写入保护继续保留。
