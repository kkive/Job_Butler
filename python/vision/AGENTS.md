# python/vision/AGENTS.md

## 目标
使用 OmniParser 完成界面理解与元素定位。

## 职责
- 解析按钮/输入框/文本区域
- 生成坐标与置信度
- OCR 兜底识别

## 输出字段（建议）
- `element_id`
- `bbox`
- `center`
- `text`
- `class`
- `confidence`
- `interactable`

## 约束
- 仅识别与定位，不做业务决策
