# apps/AGENTS.md

## 目标
承载终端应用与后台守护进程入口。

## 子模块
- `tui`：终端交互界面（ratatui）
- `daemon`：后台任务执行入口

## UI 约束补充
- `apps/tui` 默认主题：强调色 `rgb(110,228,149)` + 黑色背景 + 白色文字
- `apps/tui` 顶部菜单包含：首页、设置
- `apps/tui` 必须支持键盘和鼠标输入

## 约束
- 不直接访问数据库，统一通过 `crates/storage`
- 不直接调用外部 API，统一通过 `crates/api`
- 任务启动统一交给 `crates/orchestrator`
