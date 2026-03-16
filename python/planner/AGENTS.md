# python/planner/AGENTS.md

## 目标
实现基于状态图（LangGraph）的流程控制 Agent：
- 接收用户目标
- 规划并决策工具调用
- 执行屏幕自动化动作
- 在有限步数内产出最终结论

## 当前实现概览
核心文件：`python/planner/main.py`

当前是一个单文件流程控制器，包含：
- LLM 初始化（硅基流动 OpenAI 兼容接口）通过 `crates/api` 的 HTTP API 读取数据库中的模型名称、key、url 等参数。
- 工具函数（包括但不限于，截图、识别按钮、点击、滚动、键盘操作）
- 状态图节点（think/plan/decide/execute_tool/finalize）
- CLI 启动入口
- `tool_detect_clickable_buttons` 依赖 `python/OmniParser` 实现

## 状态结构（GraphState）
主要字段：
- `goal`：用户目标
- `thought`：思考结果
- `plan`：规划结果
- `decision`：`tool` 或 `finish`
- `tool_name` / `tool_input` / `tool_output`：工具调用上下文
- `screenshot_path`：截图路径
- `latest_buttons`：最新按钮候选
- `error`：错误信息
- `final_answer`：最终输出
- `step_count` / `max_steps`：步数控制

## 状态图流程
节点：
1. `think`：生成高层思考
2. `plan`：生成执行规划
3. `decide`：产出结构化决策 JSON（继续调用工具/结束）
4. `execute_tool`：执行具体工具并更新状态
5. `finalize`：汇总最终回答

路由：
- `decide -> execute_tool/finalize`
- `execute_tool -> decide/finalize`
- 达到 `max_steps` 会强制进入 `finalize`

## 工具清单
由 `main.py` 内函数实现：
- `tool_capture_screen`
- `tool_detect_clickable_buttons`
- `tool_click_screen`
- `tool_scroll_wheel`
- `tool_input_text`

## LLM 与服务商约定
- 通过 `crates/api` HTTP API 读取数据库中的模型名称、key、url 等参数。
- 默认 API 地址：`http://127.0.0.1:54001`（可用 `JOB_AGENT_API_BASE` 覆盖）。

## 输入输出约定
输入：
- CLI 参数 `--goal`（用户目标）
- `--max-steps`（最大步数）

输出：
- 标准输出打印：最终答案、规划、最后工具名/工具输出、错误
- 可选 `--show-state` 打印完整状态

## 运行方式
通过项目的 Rust 前端调用这个代码

## 开发约束
- 决策节点输出必须为可解析 JSON（严禁自然语言直接驱动执行）
- 工具函数只执行动作，不嵌入业务策略
- 所有异常应写入 `state.error`，不中断状态图主流程
- 必须受 `max_steps` 保护，防止无限循环
- 涉及真实鼠标键盘操作前必须保留人工可中断能力

## 后续建议
- 将工具实现拆分到 `python/planner/tools/`，降低 `main.py` 复杂度
- 引入结构化日志（每步决策、工具耗时、失败原因）
- 将 `decide` 的 JSON schema 抽成常量并集中校验
- 对接 `python/bridge`，作为 Rust orchestrator 可调用子模块
