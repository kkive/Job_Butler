# crates/orchestrator/AGENTS.md

## 目标
系统唯一调度入口。

## 职责
- 管理任务状态机
- 编排 AutoGen -> OmniParser -> Action 执行循环
- 失败重试、回退与终止判定

## 关键流程
1. 截图
2. 识别（OmniParser）
3. 决策（AutoGen/LLM）
4. 执行（tools）
5. 状态写入（storage）

## 约束
- 禁止在其他模块复制调度逻辑
