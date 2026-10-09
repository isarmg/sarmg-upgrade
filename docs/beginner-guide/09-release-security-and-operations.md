# 9. 发行信任与操作

依赖、源码 full revision、软件版、target 和状态合同共同绑定发行身份。Cargo 使用官方 URL/full rev/exact version 与根 lockfile。工具四个直接 Foundation crates 固定 0.10.8；仅工具使用行政权限桥，服务普通严格属主行为保持不变。

发布者用 `stage-upgrade-release.py` 对真实目标、产品定义、独立受信任公钥和受控私钥制作离线签名包。manifest 的精确字节签名，ELF与完整树 hash在签名范围内。接收者从独立渠道固定公钥 DER 指纹。

工具自身 `stage-release.sh` 只在 clean source/软件tag/locked release构建后暂存；finalize 核对实际身份和源公钥，签名、解包并运行 support 自验证。私钥不进入制品。

本地 commit/tag、离线缓存验收和远端公开发布分别记录。尚未推送的完整 Git对象不证明正式独立CI可从远端fetch。本轮不重写已发布tag或资产。

操作失败先保留恢复目录并 inspect；交接后需明确授权丢弃新增写入才恢复，最后实际原程序业务 ready。详见[操作文档](../operations.md)。
