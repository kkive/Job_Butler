# python/AGENTS.md

## 目标
Python 侧 AI 能力集合。

## 子模块
- `planner`：langgraph 流程编排
- `vision`：OmniParser-master 识别
- `bridge`：与 Rust 通信

## 约束
- Python 只负责 AI/视觉能力，不替代 Rust 的主调度地位
- 涉及数据库读写必须通过 `crates/api` HTTP API

## 新增说明（planner）
- `python/planner` 提供可运行 langgraph
- 视觉解析通过 `python/OmniParser-master` 完成
- 模型 key/url/model_name 通过 `crates/api` 的 HTTP API 读取数据库配置

## Rust 与 Python 通信方案（重要）

本项目推荐采用：

- `Rust 作为主控进程`
- `Python 作为独立能力服务`
- `Rust 与 Python 通过本地 HTTP API 通信`

这是当前阶段最适合本项目的方案，原因如下：

1. 更适合真实用户使用
- 用户只需要启动一个主程序
- Rust 在启动时自动拉起 Python 服务
- 用户无需理解 Python 环境、命令行、模型依赖、解释器版本等细节

2. 更适合当前仓库架构
- Rust 已经承担 orchestrator、storage、api等核心职责
- Python 主要承担 planner、vision、bridge 、tools等 AI 能力
- 两者天然适合通过“进程边界 + API 协作”来解耦

1. 更适合维护和部署
- Python 侧依赖较重，包含 `torch`、`paddleocr`、`easyocr`、`gradio` 等库
- 如果强行将 Python 嵌入 Rust 进程，会显著增加打包、调试、升级、崩溃隔离的复杂度
- 独立进程后，即使 Python 能力异常，Rust 主控和 UI 仍可继续保持可控状态

1. 更利于后续扩展
- 后续可以将 Python 服务替换为远程服务、容器服务、GPU 服务
- Rust 调用层基本不需要大改
- 也可以逐步从 HTTP 演进到 SSE、WebSocket 或 gRPC

## 为什么当前不推荐直接嵌入 Python

当前不建议使用以下方式作为主链路：

- Rust 直接嵌入 Python 解释器
- Rust 通过 `pyo3` 或类似方案直接调用大段 Python 逻辑
- Rust 每次通过命令行临时启动 Python 脚本并等待完成

主要问题如下：

1. 环境脆弱
- Python 模型依赖复杂
- 解释器版本、动态库版本、CUDA/CPU 差异都可能引入不稳定因素

2. 崩溃隔离差
- Python 执行异常、模型初始化卡死、OCR 依赖崩溃时，容易直接影响 Rust 主进程

3. 难以拿到持续状态
- 单次调用更适合“函数式返回”
- 不适合长任务、多轮决策、截图识别、持续进度回报这类工作流

4. 不利于真实用户体验
- 后续你想做可安装、可交付、可维护的桌面工具时，这种方案成本很高

## 推荐通信模型

推荐使用“提交任务 + 查询状态 + 获取事件”的模型。

标准流程如下：

1. Rust 向 Python 提交任务
- Rust 将用户目标、上下文、截图路径、配置参数通过 HTTP 发给 Python
- Python 返回 `task_id`

2. Python 异步执行任务
- Python 在后台启动 planner/vision 任务
- 任务执行过程中不断更新任务状态

3. Rust 轮询或订阅任务状态
- Rust 使用 `task_id` 获取任务当前运行状态
- 用于 TUI 显示“正在识别页面”“正在调用模型”“正在点击按钮”等过程信息

4. Python 返回最终结果
- 任务结束后，Rust 获取最终结果、错误信息、执行摘要

## 推荐接口设计

建议在 Python 侧至少提供以下 HTTP API：

### 1. 健康检查

- `GET /health`

用途：
- Rust 启动时检测 Python 服务是否可用
- 用于 UI 显示“Python 服务在线/离线”

示例返回：

```json
{
  "status": "ok",
  "service": "job-agent-python",
  "version": "0.1.0"
}
```

### 2. 提交 Planner 任务

- `POST /planner/tasks`

用途：
- Rust 提交一个新的规划/执行任务

建议请求体：

```json
{
  "goal": "请自动完成该岗位投递流程",
  "max_steps": 12,
  "context": {
    "platform": "boss",
    "job_title": "Rust开发工程师",
  }
}
```

建议响应体：

```json
{
  "task_id": "planner_20260314_0001",
  "status": "accepted"
}
```

### 3. 获取任务状态

- `GET /planner/tasks/{task_id}`

用途：
- Rust 查询 Python 当前运行状态
- UI 展示当前阶段、当前步骤、是否失败、是否完成

建议响应体：

```json
{
  "task_id": "planner_20260314_0001",
  "status": "running",
  "stage": "vision_parse",
  "message": "正在识别页面中的可点击按钮",
  "progress": 42,
  "step_count": 5,
  "max_steps": 12,
  "started_at": "2026-03-14T16:00:00+08:00",
  "updated_at": "2026-03-14T16:00:08+08:00",
  "error": null,
  "result": null
}
```

其中 `status` 建议统一为：

- `pending`
- `running`
- `success`
- `failed`
- `cancelled`
- `timeout`

其中 `stage` 建议统一为：

- `queued`
- `capture_screen`
- `vision_parse`
- `planner_think`
- `planner_plan`
- `planner_decide`
- `tool_execute`
- `finalize`

### 4. 获取任务事件流

- `GET /planner/tasks/{task_id}/events`

用途：
- Rust 获取任务中间日志、动作建议、错误记录、模型输出摘要
- 用于“任务详情页”“调试日志页”“操作历史页”

建议响应体：

```json
{
  "task_id": "planner_20260314_0001",
  "events": [
    {
      "seq": 1,
      "type": "info",
      "stage": "vision_parse",
      "message": "已识别 7 个可点击元素",
      "timestamp": "2026-03-14T16:00:03+08:00"
    },
    {
      "seq": 2,
      "type": "decision",
      "stage": "planner_decide",
      "message": "模型建议点击：立即沟通",
      "timestamp": "2026-03-14T16:00:05+08:00"
    }
  ]
}
```

### 5. 取消任务

- `POST /planner/tasks/{task_id}/cancel`

用途：
- Rust 让 Python 停止当前任务
- 用于紧急停止机制、用户取消操作、超时中断

建议响应体：

```json
{
  "task_id": "planner_20260314_0001",
  "cancelled": true
}
```

### 6. Vision 单独接口

- `POST /vision/parse`

用途：
- 如果 Rust 或其他服务希望单独调用视觉解析能力，而不走完整 planner

建议请求体：

```json
{
  "image_path": "data/screenshots/current.png",
  "ocr_fallback": true
}
```

建议响应体：

```json
{
  "elements": [
    {
      "type": "button",
      "text": "立即沟通",
      "x": 1240,
      "y": 680,
      "confidence": 0.92
    }
  ]
}
```

## Rust 侧建议职责

Rust 应作为唯一主控，承担以下职责：

1. 服务生命周期管理
- 启动 Python 服务
- 检查 `GET /health`
- 检测服务异常退出并尝试重启

2. UI 交互与状态汇总
- 接收用户输入
- 调用 Python 接口
- 将 Python 状态映射为 TUI 可读文本

3. 工作流调度
- 统一从 orchestrator 进入
- 决定什么时候调用 planner
- 决定什么时候调用 tools

4. 数据持久化
- 将任务记录、结果、错误、日志写入 Rust 侧存储模块
- Python 不直接写数据库业务数据

5. 安全控制
- 超时控制
- 用户取消
- 紧急停止
- 坐标校验

## Python 侧建议职责

Python 只负责能力输出，不负责主流程所有权。

建议承担：

1. 视觉解析
- UI 元素检测
- OCR 兜底识别
- 坐标提取

2. Planner 推理
- 高层思考
- 规划步骤
- 动作建议
- 结构化 JSON 决策

3. 中间状态回报
- 持续更新任务阶段
- 记录每一步执行事件
- 提供最终结果摘要

不建议承担：

- 持久化主业务数据
- 自行控制整个应用生命周期
- 绕过 Rust 主控直接驱动整个系统的全局流程

## 状态存储建议

Python 服务必须维护内存中的任务状态表，至少包括：

- `task_id`
- `status`
- `stage`
- `message`
- `progress`
- `step_count`
- `max_steps`
- `created_at`
- `started_at`
- `updated_at`
- `finished_at`
- `error`
- `result`
- `events`

建议形式：

- 早期可先使用 Python 进程内字典
- 键为 `task_id`
- 值为任务状态对象

后续如需增强：

- 可接 Redis
- 可接 SQLite
- 可接消息队列

但当前阶段没有必要一开始就做复杂化。

## Rust 获取 Python 运行状态的推荐方式

对于当前项目，推荐使用“HTTP 轮询”为第一阶段方案。

原因：

1. 实现简单
- Python 好写
- Rust 好接
- 日志与调试直观

2. 足够稳定
- 对 TUI 而言，每 300ms 到 1000ms 拉一次状态已经足够
- 不需要立即上 WebSocket

3. 出错容易排查
- 每个请求、响应都可记录日志
- 对开发期最友好

推荐 Rust 轮询频率：

- 正常任务页：500ms
- 高实时场景：300ms
- 后台低频状态：1000ms

## 是否需要 SSE 或 WebSocket

当前阶段：

- 不强制
- 普通 HTTP 足够

后续如果你要做更实时的“步骤流展示”，可以升级为：

### SSE

优点：
- 服务端持续推送事件
- 实现比 WebSocket 简单
- 很适合单向状态流

适合场景：
- 日志流
- 任务步骤流
- 进度推送

### WebSocket

优点：
- 双向通信
- 实时性更高

缺点：
- 管理复杂度更高
- 当前阶段性价比不高

适合场景：
- 未来需要双向实时控制 Python 执行过程
- 未来需要浏览器端实时联动

## 错误处理建议

Rust 与 Python 的接口必须统一错误语义。

推荐错误类型：

- `bad_request`
- `service_unavailable`
- `model_not_configured`
- `vision_failed`
- `planner_failed`
- `tool_failed`
- `timeout`
- `cancelled`
- `internal_error`

建议错误响应：

```json
{
  "error": {
    "code": "vision_failed",
    "message": "OCR 识别失败",
    "detail": "paddleocr initialization failed"
  }
}
```

要求：

- Python 返回的错误必须结构化
- Rust 不能只显示原始 traceback
- UI 需要将错误转换为用户可理解文本

## 超时与取消建议

必须从设计上支持：

1. 单任务超时
- Rust 发起任务时可传最大执行时长
- Python 到时自动置为 `timeout`

2. 用户手动取消
- Rust 调用取消接口
- Python 结束当前任务并回写状态

3. 紧急停止
- Rust 保留全局停止入口
- Python 遇到停止标记后尽快退出当前工作流

## 日志建议

Python 服务建议输出两类日志：

1. 结构化运行日志
- 任务开始
- 阶段切换
- 工具调用
- 模型调用耗时
- 异常原因

2. 面向 Rust 的事件日志
- 用于 `/planner/tasks/{task_id}/events`
- 方便 UI 展示中间过程

建议每条事件至少包含：

- `seq`
- `task_id`
- `type`
- `stage`
- `message`
- `timestamp`

## 部署与用户体验建议

为了让系统不是“只给开发者自己用”，建议采用以下启动模型：

1. 用户只启动 Rust 主程序
- TUI 或桌面前端作为唯一入口

2. Rust 自动启动 Python 服务
- 启动后检查 `/health`
- 如果未启动则自动拉起

3. Rust 自动检测服务状态
- 如果 Python 异常退出，Rust 可以提示“AI 服务已断开”
- 视情况尝试自动重启

4. UI 统一展示状态
- 用户看到的是“任务正在执行”
- 而不是“某个 Python 脚本正在跑”

这对于真实用户至关重要。

## 当前阶段推荐结论

本项目当前推荐方案如下：

- 主控：Rust
- AI/视觉能力：Python
- 通信方式：本地 HTTP API
- 状态获取：Rust 轮询 Python 状态接口
- 中间日志：事件接口
- 停止控制：取消接口

这是当前最稳、最容易落地、最适合真实用户使用的方案。

## 后续演进建议

第一阶段：
- 先实现 `health`
- 先实现 `planner task submit/status/events/cancel`
- Rust 通过轮询拿状态

第二阶段：
- 拆分 `vision` 与 `planner`
- 将任务状态统一结构化
- 增强错误码体系

第三阶段：
- 视需要增加 SSE
- 视需要增加远程部署能力
- 视需要增加多任务并发调度

## 实施原则

在本仓库中，Python 侧新增通信逻辑时必须遵守：

- 只暴露能力接口，不抢夺 Rust 主控权
- 所有输出优先结构化 JSON
- 所有状态必须可查询
- 所有错误必须结构化返回
- 所有长任务必须支持取消
- 所有设计以“普通用户可用、可维护、可交付”为优先目标
