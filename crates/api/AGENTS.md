# crates/api/AGENTS.md

## 目标
统一外部 API 调用入口，并提供 HTTP 服务作为数据库访问唯一通道。

## 职责
- LLM / OCR / 视觉服务调用
- 请求重试、超时、熔断策略
- 统一响应结构与错误映射
- 对内提供服务商 CRUD 的 HTTP API（供 TUI/Planner/其他模块调用）
- 提供基于服务商配置的 OpenAI 兼容大模型调用 API（当前先实现硅基流动）

## HTTP 服务
- 监听地址：`127.0.0.1:54001`
- 启动时自动执行数据库初始化/恢复
- `/health`
- `GET /services`
- `GET /services/:provider`
- `POST /services`
- `DELETE /services/id/:id`

## 约束
- 所有数据库读写必须通过本模块的 HTTP API 完成
- 其他模块不得直接依赖 `crates/storage`
- 业务模块不得绕过本模块直接请求第三方


