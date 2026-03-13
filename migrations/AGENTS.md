# migrations/AGENTS.md

## 目标
数据库迁移脚本管理。

## 职责
- 表结构演进
- 回滚脚本维护
- 维护初始化 schema（如 `0001_init.sql`）

## 约束
- 迁移脚本需幂等或可安全重试
- 新表结构变更需同步 storage 初始化逻辑
