# crates/storage/AGENTS.md

## 目标
SQLite 持久化与查询访问层。

## 职责
- 数据表迁移与版本管理
- 任务/职位/投递记录读写
- 服务商（service）增删查
- 日志落库（如启用）
- 启动时数据库初始化与异常恢复

## 启动初始化与恢复
- 对外提供：`init_or_recover_database(db_path)`
- 行为：
  - 数据库不存在：自动创建并初始化 schema
  - 数据库存在且健康：只做 schema 对齐（`CREATE TABLE IF NOT EXISTS`）
  - 数据库损坏或不可读：将旧文件备份为 `*.corrupt.<timestamp>` 后重建
- 用户数据异常可调用：`reset_database(db_path)`（备份后重置）

## 约束
- 数据库访问必须通过本模块
- 对外暴露 repository 风格接口
- 初始化逻辑必须幂等，可重复执行
