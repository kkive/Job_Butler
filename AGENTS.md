# AGENTS.md

This file defines how AI agents should operate within this repository.

Agents must follow these rules when reading, modifying, or generating
code.

The purpose of this project is to build an AI-powered autonomous job
application system.

The system automatically interacts with job websites using: - LLM
decision making - UI recognition - automated mouse interaction

Agents must respect the architecture and constraints described in this
document.

------------------------------------------------------------------------

# 1. 项目定位

项目名称

Job-Agent

这是一个基于 AI Agent
的自动求职系统，核心目标是自动完成招聘网站投递流程。

系统核心流程

1 捕获屏幕 2 识别 UI 元素 3 使用 LLM 决策下一步动作 4 自动执行鼠标操作 5
记录投递结果

系统架构

User ↓ Orchestrator ↓ Vision Parsing ↓ LLM Decision ↓ Tool Execution ↓
Storage

------------------------------------------------------------------------

# 2. 技术栈

主要语言

Rust

辅助语言

Python

Rust 库

-   tokio
-   ratatui
-   serde
-   reqwest
-   sqlx

Python 组件

-   AutoGen
-   OmniParser
-   OCR

数据库

SQLite

------------------------------------------------------------------------

# 3. Repository Structure

job-agent/

apps/ - tui - daemon

crates/ - common - core - config - storage - api - ai - tools -
orchestrator - recruiter - resume

python/ - planner - vision - bridge

prompts/ configs/ migrations/ data/ logs/ docs/

Agents must not change this structure unless absolutely necessary.

------------------------------------------------------------------------

# 4. 核心模块说明

Orchestrator

位置

crates/orchestrator

职责

-   任务调度
-   状态机执行
-   重试机制
-   超时控制
-   协调 AI 与工具模块

这是整个系统唯一的工作流入口。

------------------------------------------------------------------------

AI Decision Layer

位置

crates/ai

职责

-   构建提示词
-   调用 LLM
-   生成结构化动作

示例输出

{ "action":"click", "target":"立即沟通", "confidence":0.92,
"reason":"button visible" }

AI 输出必须始终为 JSON。

------------------------------------------------------------------------

Vision System

位置

python/vision

职责

-   UI 元素检测
-   屏幕结构解析
-   坐标提取
-   OCR 兜底识别

该模块禁止包含业务逻辑。

------------------------------------------------------------------------

Planner

位置

python/planner

使用 AutoGen 实现

-   多 Agent 推理
-   任务拆分
-   动作建议

------------------------------------------------------------------------

Tools Layer

位置

crates/tools

职责

-   截图
-   鼠标移动
-   鼠标点击
-   键盘输入
-   窗口控制

Tools 只负责执行动作，不允许包含决策逻辑。

------------------------------------------------------------------------

Storage

位置

crates/storage

职责

-   数据库访问
-   投递记录
-   日志

SQLite 访问必须通过该模块。

------------------------------------------------------------------------

# 5. 执行流程

Start Task 
↓ 
Screenshot
 ↓ 
Vision Parse
  ↓ 
Planner Analyze 
  ↓ 
LLM Decide 
  ↓
JSON Action
 ↓
Tool Execute 
 ↓ 
Store Result 
 ↓
Repeat

------------------------------------------------------------------------

# 6. Coding Rules

1 Orchestrator 控制全部工作流\
2 Tools 不允许包含业务逻辑\
3 AI 输出必须为 JSON\
4 外部 API 统一通过 crates/api 调用\
5 数据库访问必须通过 crates/storage\
6 Vision 模块禁止决策逻辑\
7 鼠标点击必须验证坐标有效性\
8 所有错误必须记录日志

------------------------------------------------------------------------

# 7. 代码规范（新增）

1 所有代码注释必须使用 **中文**\
2 注释必须清晰说明模块作用与逻辑\
3 关键算法必须写完整中文注释\
4 对复杂流程必须写步骤说明

------------------------------------------------------------------------

# 8. 用户与语言规则（新增）

本项目优先面向 **中国开发者和中国用户**。

因此所有 Agent 在生成内容时必须遵循以下规则：

1 优先使用 **中文** 进行说明\
2 所有提示词默认使用中文\
3 所有文档优先保证中国开发者可以理解\
4 如需使用英文必须附带中文解释

------------------------------------------------------------------------

# 9. Logging

日志目录

logs/

日志文件

app.log\
error.log\
task.log\
action.log

------------------------------------------------------------------------

# 10. Security

安全规则

-   API Key 不允许写入日志
-   密钥必须存储在 .env
-   凭证通过环境变量读取
-   自动操作必须提供紧急停止机制

------------------------------------------------------------------------

# 11. Agent Behavior

Agent 在本仓库运行必须遵守

1 修改代码前必须阅读 AGENTS.md\
2 必须保持模块边界\
3 禁止修改无关模块\
4 代码修改必须可预测\
5 架构修改必须说明原因

------------------------------------------------------------------------

# 12. Roadmap

未来功能

-   浏览器自动化
-   AI 岗位匹配
-   简历优化
-   面试提醒
-   多平台并行投递

------------------------------------------------------------------------

# 13. License

推荐许可证

AGPL-3.0
