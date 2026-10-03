# 评测模块结构

0.6.0 继续使用 Tauri 2、React 19 和 Rust，增加 TanStack Query 与 Radix Primitives。浏览器的 iframe sandbox 承担生成页面的执行隔离，不把模型代码放进 React DOM，不引入第二套应用框架。

## 视图和数据

- `useEvaluations` 通过 TanStack Query 共享 dashboard、activity 和 run 缓存。读请求可在活跃时刷新，写操作禁止自动重试，取消旧读取后提交新状态。
- `EvaluationPage` 只组合视图和页面选择。Gallery、Timeline、Toolbar、Report、PlanDrawer 各自负责单一交互。
- `EvaluationDialog` 使用 Radix 的焦点恢复、Esc 与滚动锁；Tabs、Tooltip 复用 Radix 键盘与无障碍行为。
- 活动索引只包含渠道、模型、推理档位、时间与结果摘要。完整 HTML 和回答只在可见卡片或报告中按轮次加载，相同轮次共享缓存。

## 执行和存储

- `evaluation/types.rs` 定义持久化和 IPC 契约；旧超时、小时调度和 SVG 字段保留迁移规则。
- `cases.rs` 管理题目与文本题评分，`artifact.rs` 只提取生成文档，`runner.rs` 负责串行执行、取消和复评。
- `client.rs` 处理 Responses 请求、单次截止时间、有限内存和凭据脱敏；网络失败不自动重试。
- `scheduler.rs` 认领到期计划，`storage.rs` 原子保存报告、紧凑索引、历史轮转和退出恢复，导出文件独立保留。

## 动画预览

`ArtifactPreview` 只负责可见性、视口和播放命令。文档执行使用无同源权限的 sandbox iframe；应用与子框架的播放消息绑定来源和随机 token。播放桥控制 CSS、SMIL、requestAnimationFrame 与计时器，不改变原始报告内容。

安装版在主窗口创建前准备独立只读本机预览源，并将主文档 CSP 限定到该进程的精确预览路径。模型 HTML 的原生权限不随主窗口授予，预览源没有模型转发、文件访问或凭据接口。开发浏览器使用同样受限的 srcdoc。默认策略不开放任意本机端口，预览服务不可用时显示错误。

所有作品保留原始代码；运行环境隔离与题目评分分开维护。外链资源不能在应用内预览时，用户仍可在原文中查看完整输出。
