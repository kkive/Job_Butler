use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
    MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use job_agent_api::{
    add_service_provider_via_api, delete_service_provider_via_api, view_service_providers,
};
use job_agent_storage::{
    default_db_path, init_or_recover_database, NewServiceProvider, ServiceProvider,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::{Frame, Terminal};

const COLOR_ACCENT: Color = Color::Rgb(110, 228, 149);
const COLOR_BG: Color = Color::Black;
const COLOR_TEXT: Color = Color::White;
const COLOR_POPUP_THEME: Color = Color::Rgb(244, 209, 180); // #F4D1B4
const COLOR_POPUP_TEXT: Color = Color::Black;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Home,
    Task,
    Settings,
}

#[derive(Debug, Clone)]
struct UiRegions {
    home_tab: Rect,
    task_tab: Rect,
    settings_tab: Rect,
    start_button: Option<Rect>,
    setting_rows: Vec<Rect>,
}

impl Default for UiRegions {
    fn default() -> Self {
        Self {
            home_tab: Rect::default(),
            task_tab: Rect::default(),
            settings_tab: Rect::default(),
            start_button: None,
            setting_rows: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct ServiceListPopup {
    items: Vec<ServiceProvider>,
    selected: usize,
}

#[derive(Debug, Clone)]
struct AddServicePopup {
    provider_name: String,
    model_name: String,
    api_url: String,
    api_key: String,
    active_field: usize,
}

#[derive(Debug, Clone)]
enum Popup {
    None,
    ViewServices(ServiceListPopup),
    AddService(AddServicePopup),
    DeleteService(ServiceListPopup),
}

struct App {
    running: bool,
    active_tab: Tab,
    status: String,
    settings_items: Vec<&'static str>,
    selected_setting: usize,
    ui_regions: UiRegions,
    popup: Popup,
}

impl App {
    fn new() -> Self {
        Self {
            running: true,
            active_tab: Tab::Home,
            status: "就绪：可使用键盘或鼠标操作".to_string(),
            settings_items: vec!["查看服务商", "添加服务商", "删除服务商", "反馈问题", "配置"],
            selected_setting: 0,
            ui_regions: UiRegions::default(),
            popup: Popup::None,
        }
    }

    fn next_tab(&mut self) {
        self.active_tab = match self.active_tab {
            Tab::Home => Tab::Task,
            Tab::Task => Tab::Settings,
            Tab::Settings => Tab::Home,
        };
    }

    fn prev_tab(&mut self) {
        self.active_tab = match self.active_tab {
            Tab::Home => Tab::Settings,
            Tab::Task => Tab::Home,
            Tab::Settings => Tab::Task,
        };
    }

    fn prev_setting(&mut self) {
        if self.settings_items.is_empty() {
            return;
        }
        self.selected_setting = if self.selected_setting == 0 {
            self.settings_items.len() - 1
        } else {
            self.selected_setting - 1
        };
    }

    fn next_setting(&mut self) {
        if self.settings_items.is_empty() {
            return;
        }
        self.selected_setting = (self.selected_setting + 1) % self.settings_items.len();
    }

    fn activate_current(&mut self) {
        match self.active_tab {
            Tab::Home => {
                self.status = "已触发：开始任务（示例动作）".to_string();
            }
            Tab::Task => {
                self.status = "任务页：待接入任务列表与执行进度".to_string();
            }
            Tab::Settings => {
                self.handle_settings_action();
            }
        }
    }

    fn handle_settings_action(&mut self) {
        let item = self.settings_items[self.selected_setting];
        match item {
            "查看服务商" => self.open_view_services_popup(),
            "添加服务商" => self.open_add_service_popup(),
            "删除服务商" => self.open_delete_service_popup(),
            _ => {
                self.status = format!("设置动作：{item}");
            }
        }
    }

    fn open_view_services_popup(&mut self) {
        match fetch_service_providers() {
            Ok(items) => {
                self.popup = Popup::ViewServices(ServiceListPopup { items, selected: 0 });
                self.status = "已打开：查看服务商".to_string();
            }
            Err(e) => {
                self.status = format!("查看服务商失败: {e}");
            }
        }
    }

    fn open_add_service_popup(&mut self) {
        self.popup = Popup::AddService(AddServicePopup {
            provider_name: String::new(),
            model_name: "gpt-4o-mini".to_string(),
            api_url: "https://api.example.com/v1".to_string(),
            api_key: String::new(),
            active_field: 0,
        });
        self.status = "已打开：添加服务商（请填写表单）".to_string();
    }

    fn open_delete_service_popup(&mut self) {
        match fetch_service_providers() {
            Ok(items) => {
                self.popup = Popup::DeleteService(ServiceListPopup { items, selected: 0 });
                self.status = "已打开：删除服务商".to_string();
            }
            Err(e) => {
                self.status = format!("读取服务商失败: {e}");
            }
        }
    }

    fn handle_popup_key(&mut self, code: KeyCode) -> bool {
        match &mut self.popup {
            Popup::None => false,
            Popup::ViewServices(state) => {
                match code {
                    KeyCode::Esc => self.popup = Popup::None,
                    KeyCode::Up => {
                        if !state.items.is_empty() {
                            state.selected = state.selected.saturating_sub(1);
                        }
                    }
                    KeyCode::Down => {
                        if !state.items.is_empty() {
                            state.selected = (state.selected + 1).min(state.items.len() - 1);
                        }
                    }
                    KeyCode::Enter => self.popup = Popup::None,
                    _ => {}
                }
                true
            }
            Popup::DeleteService(state) => {
                let mut delete_target = None;
                match code {
                    KeyCode::Esc => self.popup = Popup::None,
                    KeyCode::Up => {
                        if !state.items.is_empty() {
                            state.selected = state.selected.saturating_sub(1);
                        }
                    }
                    KeyCode::Down => {
                        if !state.items.is_empty() {
                            state.selected = (state.selected + 1).min(state.items.len() - 1);
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(item) = state.items.get(state.selected) {
                            delete_target = Some(item.id);
                        }
                    }
                    _ => {}
                }
                if let Some(id) = delete_target {
                    self.delete_service_by_id(id);
                }
                true
            }
            Popup::AddService(form) => {
                match code {
                    KeyCode::Esc => self.popup = Popup::None,
                    KeyCode::Tab | KeyCode::Down => {
                        form.active_field = (form.active_field + 1) % 4;
                    }
                    KeyCode::Up => {
                        form.active_field = if form.active_field == 0 {
                            3
                        } else {
                            form.active_field - 1
                        };
                    }
                    KeyCode::Backspace => {
                        active_form_field_mut(form).pop();
                    }
                    KeyCode::Enter => {
                        if form.active_field < 3 {
                            form.active_field += 1;
                        } else {
                            self.submit_add_service();
                        }
                    }
                    KeyCode::Char(c) => {
                        if !c.is_control() {
                            active_form_field_mut(form).push(c);
                        }
                    }
                    _ => {}
                }
                true
            }
        }
    }

    fn submit_add_service(&mut self) {
        let Popup::AddService(form) = &self.popup else {
            return;
        };

        if form.provider_name.trim().is_empty()
            || form.model_name.trim().is_empty()
            || form.api_url.trim().is_empty()
            || form.api_key.trim().is_empty()
        {
            self.status = "请完整填写：服务商、模型、API URL、API Key".to_string();
            return;
        }

        let input = NewServiceProvider {
            provider_name: form.provider_name.trim().to_string(),
            model_name: form.model_name.trim().to_string(),
            api_url: form.api_url.trim().to_string(),
            api_key: form.api_key.trim().to_string(),
        };

        match add_service(input) {
            Ok(id) => {
                self.popup = Popup::None;
                self.status = format!("已添加服务商，id={id}");
            }
            Err(e) => {
                self.status = format!("添加服务商失败: {e}");
            }
        }
    }

    fn delete_service_by_id(&mut self, id: i64) {
        match delete_service(id) {
            Ok(true) => {
                self.status = format!("已删除服务商，id={id}");
                self.open_delete_service_popup();
            }
            Ok(false) => {
                self.status = format!("删除失败，未找到 id={id}");
            }
            Err(e) => {
                self.status = format!("删除服务商失败: {e}");
            }
        }
    }

    fn on_key(&mut self, code: KeyCode) {
        if self.handle_popup_key(code) {
            return;
        }

        match code {
            KeyCode::Char('q') | KeyCode::Esc => self.running = false,
            KeyCode::Left => self.prev_tab(),
            KeyCode::Right => self.next_tab(),
            KeyCode::Tab => self.next_tab(),
            KeyCode::Up => {
                if self.active_tab == Tab::Settings {
                    self.prev_setting();
                }
            }
            KeyCode::Down => {
                if self.active_tab == Tab::Settings {
                    self.next_setting();
                }
            }
            KeyCode::Enter => self.activate_current(),
            _ => {}
        }
    }

    fn on_mouse_click(&mut self, x: u16, y: u16) {
        if !matches!(self.popup, Popup::None) {
            return;
        }

        if in_rect(self.ui_regions.home_tab, x, y) {
            self.active_tab = Tab::Home;
            return;
        }
        if in_rect(self.ui_regions.task_tab, x, y) {
            self.active_tab = Tab::Task;
            return;
        }
        if in_rect(self.ui_regions.settings_tab, x, y) {
            self.active_tab = Tab::Settings;
            return;
        }

        match self.active_tab {
            Tab::Home => {
                if let Some(btn) = self.ui_regions.start_button {
                    if in_rect(btn, x, y) {
                        self.activate_current();
                    }
                }
            }
            Tab::Task => {}
            Tab::Settings => {
                for (idx, rect) in self.ui_regions.setting_rows.iter().enumerate() {
                    if in_rect(*rect, x, y) {
                        self.selected_setting = idx;
                        self.activate_current();
                        break;
                    }
                }
            }
        }
    }
}

fn active_form_field_mut(form: &mut AddServicePopup) -> &mut String {
    match form.active_field {
        0 => &mut form.provider_name,
        1 => &mut form.model_name,
        2 => &mut form.api_url,
        _ => &mut form.api_key,
    }
}

fn fetch_service_providers() -> Result<Vec<ServiceProvider>> {
    let db_path = default_db_path();
    let runtime = tokio::runtime::Runtime::new()?;
    let list = runtime.block_on(view_service_providers(&db_path))?;
    Ok(list)
}

fn add_service(input: NewServiceProvider) -> Result<i64> {
    let db_path = default_db_path();
    let runtime = tokio::runtime::Runtime::new()?;
    let id = runtime.block_on(add_service_provider_via_api(&db_path, input))?;
    Ok(id)
}

fn delete_service(id: i64) -> Result<bool> {
    let db_path = default_db_path();
    let runtime = tokio::runtime::Runtime::new()?;
    let deleted = runtime.block_on(delete_service_provider_via_api(&db_path, id))?;
    Ok(deleted)
}

fn main() -> Result<()> {
    initialize_database_before_ui()?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run_app(&mut terminal);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

fn initialize_database_before_ui() -> Result<()> {
    let db_path = default_db_path();
    let runtime = tokio::runtime::Runtime::new()?;
    let report = runtime.block_on(init_or_recover_database(&db_path))?;

    if report.recovered_from_corruption {
        eprintln!("database recovered: {}", report.db_path.display());
    }

    Ok(())
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    let mut app = App::new();

    while app.running {
        terminal.draw(|f| {
            app.ui_regions = draw_ui(f, &app);
        })?;

        if event::poll(Duration::from_millis(120))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => app.on_key(key.code),
                Event::Mouse(mouse) if is_left_click(mouse.kind) => {
                    app.on_mouse_click(mouse.column, mouse.row)
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }

    Ok(())
}

fn draw_ui(f: &mut Frame<'_>, app: &App) -> UiRegions {
    let size = f.area();
    f.render_widget(Clear, size);

    let bg = Block::default().style(Style::default().bg(COLOR_BG).fg(COLOR_TEXT));
    f.render_widget(bg, size);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(size);

    let top_block = Block::default()
        .borders(Borders::ALL)
        .title(" Job-Agent ")
        .border_style(Style::default().fg(COLOR_ACCENT));
    let tab_inner = top_block.inner(chunks[0]);
    f.render_widget(top_block, chunks[0]);

    let tab_row = tab_inner.inner(ratatui::layout::Margin {
        horizontal: 1,
        vertical: 0,
    });

    let btn_w: u16 = 4;
    let btn_h: u16 = 1;
    let gap: u16 = 1;
    let y = tab_row.y;
    let h = btn_h.min(tab_row.height);
    let home_tab = Rect::new(tab_row.x, y, btn_w.min(tab_row.width), h);
    let task_x = tab_row.x.saturating_add(btn_w).saturating_add(gap);
    let task_w = if task_x < tab_row.x.saturating_add(tab_row.width) {
        btn_w.min(tab_row.x + tab_row.width - task_x)
    } else {
        0
    };
    let task_tab = Rect::new(task_x, y, task_w, h);
    let settings_x = task_x.saturating_add(btn_w).saturating_add(gap);
    let settings_w = if settings_x < tab_row.x.saturating_add(tab_row.width) {
        btn_w.min(tab_row.x + tab_row.width - settings_x)
    } else {
        0
    };
    let settings_tab = Rect::new(settings_x, y, settings_w, h);

    render_menu_button(f, home_tab, "首页", app.active_tab == Tab::Home);
    render_menu_button(f, task_tab, "任务", app.active_tab == Tab::Task);
    render_menu_button(f, settings_tab, "设置", app.active_tab == Tab::Settings);

    let mut regions = UiRegions {
        home_tab,
        task_tab,
        settings_tab,
        start_button: None,
        setting_rows: Vec::new(),
    };

    match app.active_tab {
        Tab::Home => {
            regions.start_button = render_home(f, chunks[1]);
        }
        Tab::Task => {
            render_task(f, chunks[1]);
        }
        Tab::Settings => {
            regions.setting_rows = render_settings(f, chunks[1], app);
        }
    }

    let footer = Paragraph::new(app.status.clone())
        .style(Style::default().fg(COLOR_TEXT).bg(COLOR_BG))
        .alignment(Alignment::Left)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" 状态栏 (Q/Esc退出, 方向键导航, Enter执行) ")
                .border_style(Style::default().fg(COLOR_ACCENT)),
        );
    f.render_widget(footer, chunks[2]);

    if !matches!(app.popup, Popup::None) {
        render_modal_backdrop(f, chunks[1]);
        render_popup(f, &app.popup, chunks[1]);
    }

    regions
}

fn render_home(f: &mut Frame<'_>, area: Rect) -> Option<Rect> {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

    let logo = Paragraph::new(vec![
        Line::from(Span::styled(
            "     _       _          _                      ",
            Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            r"    | | ___ | |__      / \   __ _  ___ _ __   ",
            Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            r" _  | |/ _ \| '_ \    / _ \ / _` |/ _ \ '_ \  ",
            Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            r"| |_| | (_) | |_) |  / ___ \ (_| |  __/ | | | ",
            Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            r" \___/ \___/|_.__/  /_/   \_\__, |\___|_| |_| ",
            Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "                           |___/               ",
            Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD),
        )),
    ])
    .alignment(Alignment::Center)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Logo ")
            .border_style(Style::default().fg(COLOR_ACCENT)),
    );
    f.render_widget(logo, chunks[0]);

    let intro = Paragraph::new(vec![
        Line::from("Job-Agent: AutoGen + OmniParser + 鼠标控制 的自动求职系统"),
        Line::from("在本页面可直接开始任务。支持键盘和鼠标交互。"),
    ])
    .style(Style::default().fg(COLOR_TEXT).bg(COLOR_BG))
    .alignment(Alignment::Left)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" 项目介绍 ")
            .border_style(Style::default().fg(COLOR_ACCENT)),
    );
    f.render_widget(intro, chunks[1]);

    let start_button = centered_rect(chunks[2], 22, 3);
    let start_btn = Paragraph::new("[ 开始任务 ]")
        .alignment(Alignment::Center)
        .style(
            Style::default()
                .fg(COLOR_BG)
                .bg(COLOR_ACCENT)
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COLOR_ACCENT)),
        );
    f.render_widget(start_btn, start_button);

    Some(start_button)
}

fn render_settings(f: &mut Frame<'_>, area: Rect, app: &App) -> Vec<Rect> {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" 设置 ")
        .border_style(Style::default().fg(COLOR_ACCENT));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let items: Vec<ListItem<'_>> = app
        .settings_items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let line = Line::from(vec![
                Span::styled(format!("{:>2}. ", idx + 1), Style::default().fg(COLOR_ACCENT)),
                Span::styled(*item, Style::default().fg(COLOR_TEXT)),
            ]);
            ListItem::new(line)
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.selected_setting));

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .fg(COLOR_BG)
                .bg(COLOR_ACCENT)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ")
        .style(Style::default().bg(COLOR_BG));

    f.render_stateful_widget(list, inner, &mut state);

    let mut rows = Vec::with_capacity(app.settings_items.len());
    let row_width = inner.width.saturating_sub(2);
    for idx in 0..app.settings_items.len() {
        rows.push(Rect::new(inner.x + 1, inner.y + idx as u16, row_width, 1));
    }
    rows
}

fn render_task(f: &mut Frame<'_>, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" 任务 ")
        .border_style(Style::default().fg(COLOR_ACCENT));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let content = Paragraph::new(vec![
        Line::from("任务中心"),
        Line::from("后续接入：任务队列、执行进度、失败重试、日志追踪。"),
    ])
    .style(Style::default().fg(COLOR_TEXT).bg(COLOR_BG))
    .alignment(Alignment::Left);

    f.render_widget(content, inner);
}

fn render_popup(f: &mut Frame<'_>, popup: &Popup, content_area: Rect) {
    match popup {
        Popup::ViewServices(state) => {
            render_service_popup(f, content_area, "查看服务商", state, false)
        }
        Popup::DeleteService(state) => {
            render_service_popup(f, content_area, "删除服务商", state, true)
        }
        Popup::AddService(form) => render_add_service_popup(f, content_area, form),
        Popup::None => {}
    }
}

fn render_modal_backdrop(f: &mut Frame<'_>, area: Rect) {
    f.render_widget(Clear, area);
    let mask = Block::default()
        .style(Style::default().bg(Color::Rgb(20, 20, 20)))
        .borders(Borders::NONE);
    f.render_widget(mask, area);
}

fn render_service_popup(
    f: &mut Frame<'_>,
    content_area: Rect,
    title: &str,
    state: &ServiceListPopup,
    allow_delete: bool,
) {
    let width = content_area.width.saturating_sub(6).max(40);
    let height = content_area.height.saturating_sub(4).max(10);
    let area = centered_rect(content_area, width, height.min(18));
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(format!(" {title} "))
        .borders(Borders::ALL)
        .style(Style::default().bg(COLOR_POPUP_THEME).fg(COLOR_POPUP_TEXT))
        .border_style(Style::default().fg(COLOR_POPUP_TEXT));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if state.items.is_empty() {
        let text = if allow_delete {
            "暂无可删除服务商（Esc关闭）"
        } else {
            "暂无服务商记录（Esc关闭）"
        };
        let p = Paragraph::new(text)
            .style(Style::default().fg(COLOR_POPUP_TEXT).bg(COLOR_POPUP_THEME))
            .alignment(Alignment::Center);
        f.render_widget(p, inner);
        return;
    }

    let mut list_state = ListState::default();
    list_state.select(Some(state.selected.min(state.items.len() - 1)));

    let items = state
        .items
        .iter()
        .map(|s| {
            let line = format!("#{: <3} {} | {} | {}", s.id, s.provider_name, s.model_name, s.api_url);
            ListItem::new(line)
        })
        .collect::<Vec<_>>();

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .fg(COLOR_POPUP_THEME)
                .bg(COLOR_POPUP_TEXT)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ")
        .style(Style::default().fg(COLOR_POPUP_TEXT).bg(COLOR_POPUP_THEME));

    f.render_stateful_widget(list, inner, &mut list_state);

    let hint = if allow_delete {
        "Esc关闭 | ↑↓选择 | Enter删除"
    } else {
        "Esc关闭 | ↑↓浏览"
    };
    let hint_area = Rect::new(area.x + 2, area.y + area.height.saturating_sub(2), area.width.saturating_sub(4), 1);
    f.render_widget(
        Paragraph::new(hint).style(Style::default().fg(COLOR_POPUP_TEXT).bg(COLOR_POPUP_THEME)),
        hint_area,
    );
}

fn render_add_service_popup(f: &mut Frame<'_>, content_area: Rect, form: &AddServicePopup) {
    let width = content_area.width.saturating_sub(6).max(40);
    let height = content_area.height.saturating_sub(4).max(10);
    let area = centered_rect(content_area, width, height.min(16));
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" 添加服务商 ")
        .borders(Borders::ALL)
        .style(Style::default().bg(COLOR_POPUP_THEME).fg(COLOR_POPUP_TEXT))
        .border_style(Style::default().fg(COLOR_POPUP_TEXT));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(inner);

    render_form_line(
        f,
        chunks[0],
        "服务商名称",
        &form.provider_name,
        form.active_field == 0,
    );
    render_form_line(
        f,
        chunks[1],
        "模型名称",
        &form.model_name,
        form.active_field == 1,
    );
    render_form_line(f, chunks[2], "API URL", &form.api_url, form.active_field == 2);
    render_form_line(f, chunks[3], "API Key", &form.api_key, form.active_field == 3);

    let hint = Paragraph::new("Tab/↑↓切换字段 | 输入内容 | Backspace删除 | Enter下一项/提交 | Esc关闭")
        .style(Style::default().fg(COLOR_POPUP_TEXT).bg(COLOR_POPUP_THEME));
    f.render_widget(hint, chunks[5]);
}

fn render_form_line(f: &mut Frame<'_>, area: Rect, label: &str, value: &str, active: bool) {
    let style = if active {
        Style::default().fg(COLOR_POPUP_THEME).bg(COLOR_POPUP_TEXT)
    } else {
        Style::default().fg(COLOR_POPUP_TEXT).bg(COLOR_POPUP_THEME)
    };
    let text = format!("{}: {}", label, value);
    f.render_widget(Paragraph::new(text).style(style), area);
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}

fn in_rect(rect: Rect, x: u16, y: u16) -> bool {
    let within_x = x >= rect.x && x < rect.x.saturating_add(rect.width);
    let within_y = y >= rect.y && y < rect.y.saturating_add(rect.height);
    within_x && within_y
}

fn is_left_click(kind: MouseEventKind) -> bool {
    matches!(
        kind,
        MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
    )
}

fn render_menu_button(f: &mut Frame<'_>, area: Rect, label: &str, selected: bool) {
    let (fg, bg) = if selected {
        (COLOR_BG, COLOR_ACCENT)
    } else {
        (COLOR_TEXT, COLOR_BG)
    };

    let bg_block = Block::default().style(Style::default().bg(bg));
    f.render_widget(bg_block, area);

    let button = Paragraph::new(label)
        .alignment(Alignment::Center)
        .style(Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD));

    f.render_widget(button, area);
}


