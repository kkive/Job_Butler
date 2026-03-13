# apps/daemon/AGENTS.md

## 目标
后台服务入口，负责常驻运行和任务生命周期管理。

## 职责
- 加载配置与服务依赖
- 启动 orchestrator
- 服务启动前初始化数据库
- 在用户数据异常时触发数据库重置流程
- 处理优雅退出和恢复

## 启动数据库约定
- 默认启动时执行：`init_or_recover_database`
- 管理命令可选参数：
  - `--db-path=<path>`：指定 sqlite 文件路径
  - `--force-reset`：备份后重建数据库

## 约束
- 调度入口唯一为 `crates/orchestrator`
- 错误统一上报日志系统
