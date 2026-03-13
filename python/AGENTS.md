# python/AGENTS.md

## 目标
Python 侧 AI 能力集合。

## 子模块
- `planner`：AutoGen 流程编排
- `vision`：OmniParser 识别
- `bridge`：与 Rust 通信

## 约束
- Python 只负责 AI/视觉能力，不替代 Rust 的主调度地位

## 新增说明（planner）
- `python/planner` 提供可运行 AutoGen Planner
- 统一输入结构化 UI 元素，输出结构化动作 JSON
- AutoGen 不可用时必须可降级到启发式策略
