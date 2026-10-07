# 评测模块结构

ahaX 0.7.0 继续使用 Tauri 2、React 19、Rust，以及已有 TanStack Query 与 Radix Primitives。单次与定时视图共享同一执行服务、查询缓存和记录格式，通过运行来源区分。浏览器的 iframe sandbox 承担生成页面的执行隔离，不把模型代码放进 React DOM，不引入第二套应用框架。

## 视图和数据

- `useEvaluations` 通过 TanStack Query 共享 dashboard、activity 和 run 缓存。读请求可在活跃时刷新，写操作禁止自动重试，取消旧读取后提交新状态。删除成功时移除对应报告缓存与活动摘要，避免旧查询让已删除的作品再次出现。
- `EvaluationPage` 组合子导航、页面选择和单次草稿。`EvaluationResults` 按 `trigger = manual | scheduled` 分流：单次糖果与判题展示逐条回答，定时结果展示 48 格时间线；两类鹈鹕都复用 Gallery。
- Gallery、Timeline、Toolbar、Report、History、DeleteDialog、PlanDrawer 分别负责作品、时段、筛选、报告、记录管理、删除确认和设置。单次弹窗只调用 start，定时弹窗只调用 save，不再把两类操作放在同一表单里。
- `EvaluationDialog` 使用 Radix 的焦点恢复、Esc 与滚动锁；Tabs、Tooltip 复用 Radix 键盘与无障碍行为。
- 活动索引只包含渠道、模型、推理档位、时间与结果摘要。完整 HTML 和回答只在可见卡片或报告中按轮次加载，相同轮次共享缓存。
- 主题使用应用统一的语义颜色变量；浅色、深色和跟随系统不另建评测专用主题实现。作品文档自己的颜色不随宿主主题改写。

## 执行和存储

- `evaluation/types.rs` 定义持久化和 IPC 契约；旧超时、小时调度和 SVG 字段保留迁移规则。
- `cases.rs` 管理题目与文本题评分，`artifact.rs` 只提取生成文档，`runner.rs` 负责串行执行、取消和复评。
- `client.rs` 处理 Responses 请求、单次截止时间、有限内存和凭据脱敏；网络失败不自动重试。
- `scheduler.rs` 认领到期计划，`storage.rs` 原子保存报告、紧凑索引、历史轮转和退出恢复，导出文件独立保留。
- `deletion.rs` 管理整轮删除与中断恢复，薄 IPC `delete_evaluation_runs` 接收 `runIds`，返回最新 dashboard；前端通过 `evaluationApi.remove` 调用。单次与定时共用后端计划及运行名额，不复制执行器或存储结构。

## 删除一致性

删除与开始、进度写入和完成提交共用运行状态锁，磁盘变更再获取应用文件锁。批量 ID 先去重并全量校验，空列表、超过 100 个不同 ID、非法或不存在的 ID、运行中记录均拒绝。校验失败不部分删除；选择其他历史也不会发送取消信号或调整计划。

有效报告先移动到 `evaluations/.deleting/`，随后在一次索引替换中同步移除 history 和 records，最后清理暂存文件。原子提交失败时依据当前索引恢复文件；启动恢复使用同一规则判断应还原还是清理，不需要另一套任务队列。索引已提交但文件暂时占用时返回清理提示，下一次打开再处理。`evaluations/exports/` 不参与这项事务。

## 动画预览

`ArtifactPreview` 只负责可见性、视口和播放命令。产品界面自动播放并提供重播，不再显示暂停按钮；内部仍保留可见性相关的播放控制以释放后台开销。文档执行使用无同源权限的 sandbox iframe；应用与子框架的播放消息绑定来源和随机 token。播放桥控制 CSS、SMIL、requestAnimationFrame 与计时器，不改变原始报告内容。

安装版在主窗口创建前准备独立只读本机预览源，并将主文档 CSP 限定到该进程的精确预览路径。模型 HTML 的原生权限不随主窗口授予，预览源没有模型转发、文件访问或凭据接口。开发浏览器使用同样受限的 srcdoc。默认策略不开放任意本机端口，预览服务不可用时显示错误。

所有作品保留原始代码；运行环境隔离与题目评分分开维护。外链资源不能在应用内预览时，用户仍可在原文中查看完整输出。

## 与本机诊断的边界

模型评测会调用 Responses API，本地诊断默认不调用模型。诊断的字段检查、受管目录修复与备份恢复预检继续使用既有 core 服务，修复提交成功后重新诊断；不把评测执行器用于配置修复。详见 [本地诊断与修复](DIAGNOSTICS.md)。产品改名保留原数据路径、凭据和题库兼容标识，避免用重命名迁移混入评测结果维护。
