# 本地诊断与修复

诊断默认仅检查本地配置，不调用模型 API。配置检查、受管文件检查和连接协议探测分开实现；结果不包含原始配置、凭据、私有目录、备份内容或远端错误正文。

## 检查范围

- TOML 语法、模型与服务商引用、当前配置方案、认证助手和认证冲突。
- `profiles` / `model_providers` 表结构、有效配置中的模型目录路径与推理强度字段类型，以及 `forced_login_method` 的合法取值。
- 配置文件只读属性。诊断不会自动改变权限；未发现只读属性也不等于验证了所有 Windows ACL 或占用情况。
- 已启用的受管网关、凭据读取程序、渠道 Key，以及统一模型目录与保存记录的一致性。
- 仅核对 AhaX 管理的模型目录文件，按 4 MiB 上限读取，不跟随任意外部目录配置进行修复。
- 对当前配置目录最近最多 20 份备份执行既有恢复预检，检查可读取性、解密、TOML 和连接兼容性。不会写回备份，也不会在诊断阶段恢复。

页面优先显示需处理问题，已通过项目可以折叠，配置备份入口直接连接已有恢复流程。导出的报告为此脱敏结果，不是配置文件的副本。

## 修复边界

受管模型目录缺失或损坏通过已有的预览、冲突检查、自动备份和原子写入流程重建。只读配置、缺失凭据或助手、没有启用模型时，不会显示可直接应用的修复入口。备份仍通过现有的完整恢复预览处理，不能跨 `CODEX_HOME` 恢复。

不终止其他进程、不修改历史会话的 provider、不切换官方登录、不移除虚拟账号、不清理用户认证或任意系统目录。现有内部 provider、凭据标识和数据目录保持兼容。

## 参考与核对

- OpenAI 配置参考：<https://developers.openai.com/codex/config-reference>。核对 `model_catalog_json` 和 `model_reasoning_effort` 的字符串类型，以及 `forced_login_method = chatgpt | api`。推理强度具体选项由模型和客户端决定，本次检查不新增硬编码档位限制。
- AiMaMi 公开源码：<https://github.com/borawong/AiMaMi>，核对 main `add37271e29ba81ee17f15f444a24925af50bb87` 与 v1.2.1 `297c7af56f10fb371b77bc9b6b65aa320afcbe7e`。`src/components/maintenance/maintenance-page.tsx` 将诊断、清理、注册表重建与重启分成独立动作；`src-tauri/src/core/repository.rs` 的 `diagnose` 返回平台、版本、账号与 API 状态。
- AiMaMi 发行说明：<https://github.com/borawong/AiMaMi/releases/tag/v1.2.1>，说明了路由/线程恢复、虚拟账号残留处理与脱敏诊断报告。上述公开源码快照未找到对应路由诊断实现，因此仅借鉴有记录的问题分类、分离操作与脱敏反馈，不声称移植或验证其未公开实现。

## 验证

新增隔离 fixture 覆盖错误字段值脱敏、受管目录损坏检测且外部文件保持不变、只读配置字节保持不变、DPAPI 恢复点预检与损坏识别。网络诊断继续使用本机合成服务验证 Responses、流式与工具调用，不访问真实模型或真实 Key。
