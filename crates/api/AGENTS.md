# crates/api/AGENTS.md

## 目标
统一外部 API 调用入口。

## 职责
- LLM / OCR / 视觉服务调用
- 请求重试、超时、熔断策略
- 统一响应结构与错误映射
- 对内提供服务商 CRUD API（供 TUI/其他模块调用）
- 提供基于服务商配置的 OpenAI 兼容大模型调用 API（当前先实现硅基流动）

## 当前内部 API（服务商）
- `view_service_providers(db_path)`
- `add_service_provider_via_api(db_path, input)`
- `delete_service_provider_via_api(db_path, id)`
- `get_service_provider_via_api(db_path, provider_name)`

## 当前 LLM API（硅基流动）
- `call_siliconflow_chat_completion(db_path, input)`
- 调用流程：先读数据库服务商配置 -> 再发起 OpenAI 兼容 `/v1/chat/completions` 请求

## 约束
- 业务模块不得绕过本模块直接请求第三方
- 业务模块访问数据库时，优先通过 `crates/api` 暴露的接口
