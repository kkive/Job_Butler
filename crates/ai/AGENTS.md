# crates/ai/AGENTS.md

## 目标
LLM 决策封装层。

## 职责
- 组装提示词与上下文
- 调用模型并返回结构化动作
- 校验输出 JSON 合法性

## 输出协议（建议）
- `action_type`
- `target`
- `x` / `y`
- `confidence`
- `reason`

## 约束
- 禁止直接调用鼠标执行，执行交给 `crates/tools`
