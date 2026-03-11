## 项目名称
**Job-Agent**

基于 AI Agent 的自动求职系统。

该系统通过 **大语言模型决策 + 计算机自动化操作** 的方式，自动识别招聘网站界面并完成简历投递。
系统主要使用 **Rust 实现核心框架**，并在必要时使用 **Python 作为 AI 规划与视觉辅助模块**。

---

# 一、项目概述

Job-Agent 是一个自动化求职工具，目标是帮助用户自动完成招聘网站的投递流程。

系统主要能力包括：

- 自动识别招聘网站界面
- 分析屏幕内容
- 定位可交互按钮
- 自动控制鼠标与键盘
- 通过 LLM 决策下一步操作
- 自动投递简历
- 记录投递进度与日志

系统采用 **Agent 架构设计**：

用户界面
   ↓
任务编排模块
   ↓
AI 决策模块
   ↓
工具执行模块
   ↓
招聘平台逻辑
   ↓
数据存储

---

# 二、技术栈

## 核心语言
Rust

## 辅助语言
Python

## 主要库

### Rust
- ratatui
- tokio
- serde
- reqwest
- sqlx

### Python
- LangChain 或自定义 Planner
- YOLO
- OCR

数据库：SQLite

---

# 三、项目目录结构

job-agent/
│
├─ apps/
│  ├─ tui/
│  └─ daemon/
│
├─ crates/
│  ├─ common/
│  ├─ core/
│  ├─ config/
│  ├─ storage/
│  ├─ api/
│  ├─ ai/
│  ├─ tools/
│  ├─ orchestrator/
│  ├─ recruiter/
│  └─ resume/
│
├─ python/
│  ├─ planner/
│  ├─ vision/
│  └─ bridge/
│
├─ prompts/
├─ configs/
├─ migrations/
├─ data/
├─ logs/
└─ docs/

---

# 四、模块职责

## apps/tui
终端 UI 界面，负责：
- 首页展示
- 任务进度
- 服务商管理
- 系统配置
- 启动任务

## orchestrator
任务编排模块，负责：
- 任务调度
- 执行流程
- 状态管理
- 失败重试

## ai
AI 决策模块，负责：
- 构建 LLM 上下文
- 管理提示词
- 调用模型
- 输出结构化动作

示例输出：

{
  "action": "click",
  "target": "立即沟通",
  "confidence": 0.92
}

## tools
自动化工具模块：
- 截图
- OCR
- UI识别
- 鼠标控制
- 键盘控制
- 窗口控制

## recruiter
招聘网站适配模块：
- Boss直聘
- 智联招聘
- 猎聘

负责页面解析与投递逻辑。

## resume
简历管理模块：
- 多版本简历
- 附件管理
- 模板选择
- 求职信生成

## storage
数据持久化模块：
- SQLite数据库
- 投递记录
- 日志存储

## api
外部服务接口模块：
- LLM API
- OCR API
- 视觉模型
- 通知服务

---

# 五、核心数据表

user
id
first_use_time

prompt
id
content
created_at

service
provider_name
model_name
api_url
api_key

task
id
type
status
retry_count
created_at

job
id
platform
company
title
url
salary
location

application
id
job_id
resume_id
status
apply_time

denylist
company
blocked_time

---

# 六、Agent 执行流程

用户点击开始
↓
创建任务
↓
任务编排模块启动
↓
截图
↓
视觉识别
↓
AI 判断下一步
↓
执行工具操作
↓
更新数据库
↓
循环直到流程结束

---

# 七、开发规则

1. 工具模块不允许包含业务逻辑
2. AI 输出必须为 JSON
3. 外部 API 必须通过 api 模块调用
4. orchestrator 是唯一调度入口
5. 数据库访问必须通过 storage
6. 所有错误必须统一处理

---

# 八、日志

日志目录：

logs/

日志类型：

app.log
error.log
task.log

---

# 九、安全

- API Key 必须加密存储
- 日志禁止输出敏感信息
- 使用 .env 管理密钥

---

# 十、未来规划

- 浏览器自动化
- AI岗位匹配
- 简历优化
- 面试提醒
- 多平台求职

---

# 十一、许可证

推荐使用：

AGPL-3.0