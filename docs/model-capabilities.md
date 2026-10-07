# 官方模型推理能力核查

核查日期：**2026-10-03**。运行时数据位于 `src-tauri/resources/model-capabilities.json`；本文记录所查模型、API 支持档位与默认值，以及原生 Ultra 的客户端语义。抓取的官网页面及官方源码存放于本地验收目录 `artifacts/official-model-capabilities-2026-10-03/`、`artifacts/official-ultra-0.160/`，不包含账号或密钥。

## 本次纠正

上一版直接沿用了 Codex 客户端经过筛选的模型菜单。该菜单不是公开 API 的完整能力说明，也不能作为 API 服务端默认值：例如 `gpt-5.4` 的客户端菜单曾仅列四档、默认 medium，API 官网明确支持 none/low/medium/high/xhigh，默认 none。

- GPT-5、Mini、Nano 支持 minimal/low/medium/high；不能替换成带 none 或 xhigh 的通用模板。
- GPT-5.1 支持 none/low/medium/high，默认 none。
- GPT-5.2 与 GPT-5.4、Mini、Nano 支持 none/low/medium/high/xhigh，默认 none。
- GPT-5.5 同样支持五档，默认 medium。
- GPT-5 Pro 只有 high；5.4 Pro 有 medium/high/xhigh，默认 medium；5.5 Pro 默认 high。5.2 Pro 官网页面列出 medium/high/xhigh，但未注明服务端默认值。
- GPT-5.1-Codex Mini 的官方客户端只列 medium/high；不能沿用普通 Codex 的 low/medium/high，更不能因名称含 Mini 就猜测能力。
- GPT-5.6 Sol/Terra/Luna 与 GPT-6 Sol/Luna 支持 none/low/medium/high/xhigh/max，默认 medium。官网明确 `gpt-5.6` 是 Sol 的别名。
- GPT-6 Astra 与 GPT-6.1 Sol 不支持 none/minimal，支持 low/medium/high/xhigh/max。6.1 Sol 的 API 默认是 medium；Astra API 页面没有声明默认，原生目录采用该精确型号官方客户端的 low。
- 5.6 的 Pro 由 `reasoning.mode = "pro"` 控制，不是推理强度，也没有依据创建 `gpt-5.6-pro` 型号。官网推理指南同时说明 6.1 Sol、6 Sol、6 Luna 的默认强度为 medium。ahaX 本次管理 effort，没有增加 Pro mode 切换。
- 日期快照只登记官网实际列出的完整 ID；不根据名称前缀、日期形状或用户自定义后缀推断。
- 可用第三方模型不可能由一份 OpenAI 表穷举。渠道重命名、代理特有模型、其他厂商协议，以及下表“未找到完整声明”的型号仍需按该渠道的公开说明手动配置。

## API 档位与原生客户端兼容性

`max` 是公开 API 的真实 effort；完整目录保留该档位，不自动裁剪。公开 API 当前枚举没有 `ultra`。Codex 0.160 的 Ultra 是客户端编排选项，执行时映射为模型定义的多代理强度或 max 等；直接把 Ultra 当成第三方 API effort 发送会混淆两种含义，因此 ahaX 不把它登记为 API 档位。

- [Codex 0.137 协议](https://github.com/openai/codex/blob/rust-v0.137.0/codex-rs/protocol/src/openai_models.rs) 的 effort 是固定六项枚举，包含 max 的目录会被旧客户端拒绝。
- [Codex 0.138 协议](https://github.com/openai/codex/blob/rust-v0.138.0/codex-rs/protocol/src/openai_models.rs) 增加 `Custom(String)`，能够读取并保持 max 等模型定义的 effort。
- [Codex 0.143 协议](https://github.com/openai/codex/blob/rust-v0.143.0/codex-rs/protocol/src/openai_models.rs) 增加显式 Max/Ultra 枚举。
- [Codex 0.160 的努力值转换](https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/protocol/src/openai_models/reasoning_effort.rs) 说明 Ultra 的客户端语义。
- [公开 Responses API 定义](https://developers.openai.com/api/reference/resources/responses/methods/create) 和[官方 Python SDK](https://github.com/openai/openai-python/blob/main/src/openai/types/shared/reasoning.py) 列出 API effort 到 max，并要求按模型核对支持值。

使用含 max 的统一目录至少需要 Codex **0.138**；0.143 以后有明确枚举。ahaX 的完整 Ultra 接入以正式版 **0.160.0**（GitHub Release 发布于 2026-10-01）的原生运行时为验收基准；旧版本能读取 Ultra 枚举不代表具有同样的多代理行为。本次没有修改用户全局安装的 CLI。

## 原生 Ultra

ahaX 0.5.0 在原生模型目录中分别声明 `supported_reasoning_levels` 的 Ultra、`multi_agent_version = "v2"` 和 `multi_agent_reasoning_effort`。Codex 在用户选中 Ultra 后启用主动多代理提示，并在构建 Responses 请求时将 Ultra 转换为该模型的实际 API 强度。ahaX 网关不自行改写该强度。依据为[官方 0.160 模型目录](https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/models-manager/models.json)、[强度转换](https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/protocol/src/openai_models/reasoning_effort.rs)、[客户端请求构建](https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/core/src/client.rs)和[多代理模式选择](https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/core/src/session/multi_agents.rs)。

| 自动开启原生 Ultra 的精确 ID | Ultra 对应的 API effort | 依据 |
| --- | --- | --- |
| `gpt-6-astra` | `xhigh` | 官方模型目录显式声明 |
| `gpt-6.1-sol` | `xhigh` | 官方模型目录显式声明 |
| `gpt-6-sol` | `max` | 官方客户端的 max 回退规则 |
| `gpt-5.6-sol`、官方别名 `gpt-5.6` | `max` | 官方客户端的 max 回退规则 |
| `gpt-5.6-terra` | `max` | 官方客户端的 max 回退规则 |
| `gpt-daybreak-blue-latest` | `max` | 官方客户端的 max 回退规则 |
| `gpt-daybreak-red-latest` | `max` | 官方客户端精确型号声明，API 页面未完整声明档位 |

GPT-6 Luna、GPT-5.6 Luna、GPT-5.5 等未在该精确目录中开放 Ultra，不自动增加；不能仅凭型号系列或 `multi_agent_version` 推断。原生 Ultra 也不改变官网已确认的其他 API 档位和默认值。

Daybreak Red 的公开 API 档位仍记为未证实。其 `nativeEfforts` 单独保存官方客户端明确列出的 low/medium/high/xhigh/max，原生默认 medium；`nativeUltra` 保存编排映射。直接调用 API 的测试不会借用这些原生声明来认定服务端能力。

手动能力列表是显式覆盖：关闭推理或只选择部分档位时不自动增加 Ultra。手动开启 Ultra 必须同时选择至少一个 low/medium/high/xhigh/max；先使用仍在选择范围内的官方 Ultra 强度，否则使用已选范围内最高的有效强度。只提供 none/minimal/Ultra 的组合会被拒绝。手动选择 Ultra 为默认值时仍遵循同样的转换。

历史恢复将 Ultra 的实际强度和 v2 版本保存为 `nativeReasoning: { multiAgentVersion, ultraEffort }`，防止新版本注册表改变旧快照语义。编辑推理范围时重新计算映射；仅更换默认值保留历史映射。不含 Ultra 的旧目录恢复后仍按旧目录显示。

完整多代理运行时在新建任务时确定；已存在的旧任务可能沿用旧运行时，应用目录后应重新打开 Codex 并新建任务。用户显式设置的 `[agents] enabled = false` 保持有效，ahaX 不覆盖该偏好。依据见[官方运行时选择](https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/core/src/session/mod.rs)和[配置优先级](https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/core/src/config/mod.rs)。

能力评测直接调用渠道 API，只接受该模型确认的具体 API 档位；不选择档位则完全省略 `reasoning`，交由服务端处理默认值。`ultra` 不会作为 API 参数发送，也不会静默变成普通档位。

### 验收

`scripts/official-models-smoke.mjs` 使用独立 `CODEX_HOME`、本地凭据助手和本地 Responses 服务驱动官方 Codex 0.160.0 app-server。2026-10-03 验收覆盖 7 个模型、38 次请求，验证原生模型菜单、各档位与默认值、Ultra 实际映射，以及 Ultra 请求中的主动委派提示、`spawn_agent` 与 v2 `followup_task` 工具；普通档位没有主动委派提示。所有请求仅发往本机，不使用真实 Key，不产生远端调用。报告为 `artifacts/official-codex-smoke-1791015517867/report.json`。此脚本验证与 ahaX 同构的目录和 provider 配置；ahaX 自身目录生成及历史恢复另由 Rust 单元测试覆盖。

## 两种默认值必须分开

`apiDefault` 是官网或官方 API 定义明确记载的服务端默认值；`null` 表示未证实，不能解释成 none。

原生模型目录必须提供合法默认值，因为部分客户端将缺失默认值显示为 none，即使模型不支持 none。ahaX 使用下面的明确优先顺序：

1. 用户显式选择的默认强度。
2. 官网明确记载、且在该模型可选集合中的 API 默认值。
3. 同一精确型号的官方 Codex 客户端默认值，并单独标明来源。
4. ahaX 兼容默认：存在 medium 时选 medium，否则选该模型实际支持的最低一档。

第三和第四项仅为原生界面的初始化选择，不声称是服务端默认值；不改写已发送请求中的 effort，不降级或静默重试。用户关闭档位或模型没有已证实的能力列表时不生成推理选项。

## 逐模型记录

共核对登记 51 个型号组、83 个精确 ID（包含公开快照）。历史 API 文档中已删除的能力细节，以具体版本的官方 SDK 或该精确型号官方客户端为补充来源，不能用全局 SDK 枚举推断所有变体。

| 精确型号 / 公开快照 | 自动登记的 API 档位 | 官网 API 默认 | 原生目录默认及来源 | 官方出处 |
| --- | --- | --- | --- | --- |
| `gpt-5`<br>`gpt-5-2025-08-07` | `minimal`, `low`, `medium`, `high` | `medium` | `medium` · 官网 API 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5>) [依据1](<https://developers.openai.com/api/docs/guides/gpt-5>) [依据2](<https://github.com/openai/openai-python/blob/v2.8.0/src/openai/types/shared/reasoning.py>) |
| `gpt-5-mini`<br>`gpt-5-mini-2025-08-07` | `minimal`, `low`, `medium`, `high` | `medium` | `medium` · 官网 API 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5-mini>) [依据1](<https://developers.openai.com/api/docs/guides/gpt-5>) [依据2](<https://github.com/openai/openai-python/blob/v2.8.0/src/openai/types/shared/reasoning.py>) |
| `gpt-5-nano`<br>`gpt-5-nano-2025-08-07` | `minimal`, `low`, `medium`, `high` | `medium` | `medium` · 官网 API 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5-nano>) [依据1](<https://developers.openai.com/api/docs/guides/gpt-5>) [依据2](<https://github.com/openai/openai-python/blob/v2.8.0/src/openai/types/shared/reasoning.py>) |
| `gpt-5-pro`<br>`gpt-5-pro-2025-10-06` | `high` | `high` | `high` · 官网 API 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5-pro>) |
| `gpt-5.1`<br>`gpt-5.1-2025-11-13` | `none`, `low`, `medium`, `high` | `none` | `none` · 官网 API 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5.1>) |
| `gpt-5.2`<br>`gpt-5.2-2025-12-11` | `none`, `low`, `medium`, `high`, `xhigh` | `none` | `none` · 官网 API 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5.2>) |
| `gpt-5.2-pro`<br>`gpt-5.2-pro-2025-12-11` | `medium`, `high`, `xhigh` | 未声明 / 不适用 | `medium` · ahaX 兼容默认（非 API 声明） | [模型页](<https://developers.openai.com/api/docs/models/gpt-5.2-pro>) |
| `gpt-5.4`<br>`gpt-5.4-2026-03-05` | `none`, `low`, `medium`, `high`, `xhigh` | `none` | `none` · 官网 API 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5.4>) |
| `gpt-5.4-mini`<br>`gpt-5.4-mini-2026-03-17` | `none`, `low`, `medium`, `high`, `xhigh` | `none` | `none` · 官网 API 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5.4-mini>) |
| `gpt-5.4-nano`<br>`gpt-5.4-nano-2026-03-17` | `none`, `low`, `medium`, `high`, `xhigh` | `none` | `none` · 官网 API 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5.4-nano>) |
| `gpt-5.4-pro`<br>`gpt-5.4-pro-2026-03-05` | `medium`, `high`, `xhigh` | `medium` | `medium` · 官网 API 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5.4-pro>) |
| `gpt-5.5`<br>`gpt-5.5-2026-04-23` | `none`, `low`, `medium`, `high`, `xhigh` | `medium` | `medium` · 官网 API 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5.5>) |
| `gpt-5.5-pro`<br>`gpt-5.5-pro-2026-04-23` | `medium`, `high`, `xhigh` | `high` | `high` · 官网 API 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5.5-pro>) |
| `gpt-5-codex` | `low`, `medium`, `high` | 未声明 / 不适用 | `medium` · 该精确型号官方 Codex 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5-codex>) [依据1](<https://github.com/openai/codex/blob/rust-v0.70.0/codex-rs/core/src/openai_models/model_presets.rs>) |
| `gpt-5.1-codex` | `low`, `medium`, `high` | 未声明 / 不适用 | `medium` · 该精确型号官方 Codex 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5.1-codex>) [依据1](<https://github.com/openai/codex/blob/rust-v0.70.0/codex-rs/core/src/openai_models/model_presets.rs>) |
| `gpt-5.1-codex-max` | `low`, `medium`, `high`, `xhigh` | 未声明 / 不适用 | `medium` · 该精确型号官方 Codex 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5.1-codex-max>) [依据1](<https://github.com/openai/codex/blob/rust-v0.70.0/codex-rs/core/src/openai_models/model_presets.rs>) |
| `gpt-5.1-codex-mini` | `medium`, `high` | 未声明 / 不适用 | `medium` · 该精确型号官方 Codex 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5.1-codex-mini>) [依据1](<https://github.com/openai/codex/blob/rust-v0.70.0/codex-rs/core/src/openai_models/model_presets.rs>) |
| `gpt-5.2-codex` | `low`, `medium`, `high`, `xhigh` | 未声明 / 不适用 | `medium` · ahaX 兼容默认（非 API 声明） | [模型页](<https://developers.openai.com/api/docs/models/gpt-5.2-codex>) |
| `gpt-5.3-codex` | `low`, `medium`, `high`, `xhigh` | 未声明 / 不适用 | `medium` · 该精确型号官方 Codex 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5.3-codex>) [依据1](<https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/models-manager/models.json>) |
| `gpt-5.6-sol`<br>`gpt-5.6` | `none`, `low`, `medium`, `high`, `xhigh`, `max` | `medium` | `medium` · 官网 API 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5.6-sol>) |
| `gpt-5.6-terra` | `none`, `low`, `medium`, `high`, `xhigh`, `max` | `medium` | `medium` · 官网 API 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5.6-terra>) |
| `gpt-5.6-luna` | `none`, `low`, `medium`, `high`, `xhigh`, `max` | `medium` | `medium` · 官网 API 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5.6-luna>) |
| `gpt-6-sol` | `none`, `low`, `medium`, `high`, `xhigh`, `max` | `medium` | `medium` · 官网 API 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-6-sol>) |
| `gpt-6-luna` | `none`, `low`, `medium`, `high`, `xhigh`, `max` | `medium` | `medium` · 官网 API 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-6-luna>) |
| `gpt-6.1-sol` | `low`, `medium`, `high`, `xhigh`, `max` | `medium` | `medium` · 官网 API 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-6.1-sol>) |
| `gpt-6-astra` | `low`, `medium`, `high`, `xhigh`, `max` | 未声明 / 不适用 | `low` · 该精确型号官方 Codex 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-6-astra>) [依据1](<https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/models-manager/models.json>) |
| `o1`<br>`o1-2024-12-17` | `low`, `medium`, `high` | `medium` | `medium` · 官网 API 默认 | [模型页](<https://developers.openai.com/api/docs/models/o1>) [依据1](<https://github.com/openai/openai-python/blob/v1.93.0/src/openai/types/shared/reasoning.py>) [依据2](<https://github.com/openai/openai-python/blob/v2.8.0/src/openai/types/shared/reasoning.py>) [依据3](<https://developers.openai.com/api/docs/guides/latest-model?model=gpt-5.2>) |
| `o3`<br>`o3-2025-04-16` | `low`, `medium`, `high` | `medium` | `medium` · 官网 API 默认 | [模型页](<https://developers.openai.com/api/docs/models/o3>) [依据1](<https://github.com/openai/openai-python/blob/v1.93.0/src/openai/types/shared/reasoning.py>) [依据2](<https://github.com/openai/openai-python/blob/v2.8.0/src/openai/types/shared/reasoning.py>) [依据3](<https://developers.openai.com/api/docs/guides/latest-model?model=gpt-5.2>) |
| `o3-mini`<br>`o3-mini-2025-01-31` | `low`, `medium`, `high` | `medium` | `medium` · 官网 API 默认 | [模型页](<https://developers.openai.com/api/docs/models/o3-mini>) [依据1](<https://github.com/openai/openai-python/blob/v1.93.0/src/openai/types/shared/reasoning.py>) [依据2](<https://github.com/openai/openai-python/blob/v2.8.0/src/openai/types/shared/reasoning.py>) [依据3](<https://developers.openai.com/api/docs/guides/latest-model?model=gpt-5.2>) |
| `o4-mini`<br>`o4-mini-2025-04-16` | `low`, `medium`, `high` | `medium` | `medium` · 官网 API 默认 | [模型页](<https://developers.openai.com/api/docs/models/o4-mini>) [依据1](<https://github.com/openai/openai-python/blob/v1.93.0/src/openai/types/shared/reasoning.py>) [依据2](<https://github.com/openai/openai-python/blob/v2.8.0/src/openai/types/shared/reasoning.py>) [依据3](<https://developers.openai.com/api/docs/guides/latest-model?model=gpt-5.2>) |
| `o1-mini`<br>`o1-mini-2024-09-12` | 未找到完整声明 | 未声明 / 不适用 | 不生成可选档位 | [模型页](<https://developers.openai.com/api/docs/models/o1-mini>) |
| `o1-preview`<br>`o1-preview-2024-09-12` | 未找到完整声明 | 未声明 / 不适用 | 不生成可选档位 | [模型页](<https://developers.openai.com/api/docs/models/o1-preview>) |
| `o1-pro`<br>`o1-pro-2025-03-19` | 未找到完整声明 | 未声明 / 不适用 | 不生成可选档位 | [模型页](<https://developers.openai.com/api/docs/models/o1-pro>) |
| `o3-pro`<br>`o3-pro-2025-06-10` | 未找到完整声明 | 未声明 / 不适用 | 不生成可选档位 | [模型页](<https://developers.openai.com/api/docs/models/o3-pro>) |
| `o3-deep-research`<br>`o3-deep-research-2025-06-26` | 未找到完整声明 | 未声明 / 不适用 | 不生成可选档位 | [模型页](<https://developers.openai.com/api/docs/models/o3-deep-research>) |
| `o4-mini-deep-research`<br>`o4-mini-deep-research-2025-06-26` | 未找到完整声明 | 未声明 / 不适用 | 不生成可选档位 | [模型页](<https://developers.openai.com/api/docs/models/o4-mini-deep-research>) |
| `codex-mini-latest` | 未找到完整声明 | 未声明 / 不适用 | 不生成可选档位 | [模型页](<https://developers.openai.com/api/docs/models/codex-mini-latest>) |
| `gpt-5-chat-latest` | 未找到完整声明 | 未声明 / 不适用 | 不生成可选档位 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5-chat-latest>) |
| `gpt-5.1-chat-latest` | 未找到完整声明 | 未声明 / 不适用 | 不生成可选档位 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5.1-chat-latest>) |
| `gpt-5.2-chat-latest` | 未找到完整声明 | 未声明 / 不适用 | 不生成可选档位 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5.2-chat-latest>) |
| `gpt-5.3-chat-latest` | 未找到完整声明 | 未声明 / 不适用 | 不生成可选档位 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5.3-chat-latest>) |
| `gpt-5.6-cyber` | 未找到完整声明 | 未声明 / 不适用 | 不生成可选档位 | [模型页](<https://developers.openai.com/api/docs/models/gpt-5.6-cyber>) |
| `gpt-daybreak-red-latest` | 未找到完整声明 | 未声明 / 不适用 | `medium` · 精确型号的官方原生目录，另有 Ultra | [模型页](<https://developers.openai.com/api/docs/models/gpt-daybreak-red-latest>) [原生目录](<https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/models-manager/models.json>) |
| `gpt-daybreak-blue-latest` | `none`, `low`, `medium`, `high`, `xhigh`, `max` | `medium` | `medium` · 官网 API 默认 | [模型页](<https://developers.openai.com/api/docs/models/gpt-daybreak-blue-latest>) [依据1](<https://developers.openai.com/api/docs/models/gpt-5.6-sol>) |
| `gpt-4.1`<br>`gpt-4.1-2025-04-14` | 无可配置推理档位 | 未声明 / 不适用 | 不生成可选档位 | [模型页](<https://developers.openai.com/api/docs/models/gpt-4.1>) |
| `gpt-4.1-mini`<br>`gpt-4.1-mini-2025-04-14` | 无可配置推理档位 | 未声明 / 不适用 | 不生成可选档位 | [模型页](<https://developers.openai.com/api/docs/models/gpt-4.1-mini>) |
| `gpt-4.1-nano`<br>`gpt-4.1-nano-2025-04-14` | 无可配置推理档位 | 未声明 / 不适用 | 不生成可选档位 | [模型页](<https://developers.openai.com/api/docs/models/gpt-4.1-nano>) |
| `gpt-4o`<br>`gpt-4o-2024-05-13`<br>`gpt-4o-2024-08-06`<br>`gpt-4o-2024-11-20` | 无可配置推理档位 | 未声明 / 不适用 | 不生成可选档位 | [模型页](<https://developers.openai.com/api/docs/models/gpt-4o>) |
| `gpt-4o-mini`<br>`gpt-4o-mini-2024-07-18` | 无可配置推理档位 | 未声明 / 不适用 | 不生成可选档位 | [模型页](<https://developers.openai.com/api/docs/models/gpt-4o-mini>) |
| `gpt-4-turbo`<br>`gpt-4-turbo-2024-04-09` | 无可配置推理档位 | 未声明 / 不适用 | 不生成可选档位 | [模型页](<https://developers.openai.com/api/docs/models/gpt-4-turbo>) |
| `gpt-3.5-turbo` | 无可配置推理档位 | 未声明 / 不适用 | 不生成可选档位 | [模型页](<https://developers.openai.com/api/docs/models/gpt-3.5-turbo>) |

## 未证实的范围与来源限制

`o1-mini`、`o1-preview`、`o1-pro`、`o3-pro`、两个 Deep Research 型号、部分 Chat Latest 型号、`codex-mini-latest` 和 Cyber/Daybreak Red 页面没有列出完整的可配置 effort 集合。不能因为“支持 reasoning tokens”就断言支持某三个可选档位，也不能把 pro 变体自动视作基础型号。表中保留记录、来源和“未找到完整声明”，不虚构 API 能力。Daybreak Red 另有精确型号的官方原生目录声明，按上文分开保存。早期 o1/o3/o3-mini/o4-mini 的基础型号则由[官方历史 API SDK](https://github.com/openai/openai-python/blob/v1.93.0/src/openai/types/shared/reasoning.py)、[默认值定义](https://github.com/openai/openai-python/blob/v2.8.0/src/openai/types/shared/reasoning.py)和[GPT-5.2 指南对 o3 的明确对比](https://developers.openai.com/api/docs/guides/latest-model?model=gpt-5.2)交叉佐证。

`gpt-5.1-codex`、`gpt-5.1-codex-max`、`gpt-5.1-codex-mini` 与 `gpt-5-codex` 的模型页未逐项公开 effort；仅用[官方 0.70 精确型号预设](https://github.com/openai/codex/blob/rust-v0.70.0/codex-rs/core/src/openai_models/model_presets.rs)补充可选集合，API 默认保持未声明。这种补充不覆盖有明确 API 页面声明的型号。

本次核查中 `gpt-5.3`、`gpt-5.3-pro`、`gpt-5.3-codex-spark`、`gpt-5.5-mini`、`gpt-5.5-nano`、`gpt-5.2-mini`、`gpt-5.2-nano`、`gpt-6`、`gpt-6.1`、`gpt-6-pro`、`gpt-6.1-pro`、`gpt-6-terra`、`gpt-6.1-terra`、`gpt-6.1-luna` 和 `gpt-6.1-astra` 的直接 API 模型页返回 404。这表示此次未取得该完整 ID 的官方 API 页面证据，不表示渠道一定不能提供自己的同名模型；不将这些名称自动登记为官方 API 别名。

最新 5.6/6/6.1 页面未列日期快照，因此仅登记其明确命名的 slug。Daybreak Blue 页面明确列当前快照为 `gpt-5.6-sol`，依该页面登记，后续别名切换需重新核查。旧型号出现在资料中不保证账号当前仍有调用权限。
