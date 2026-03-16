use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
    MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use job_agent_api::run_http_server;
use job_agent_storage::{
    default_db_path, NewServiceProvider, ServiceProvider,
};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::{Frame, Terminal};
use tungstenite::{connect, Message};

const COLOR_ACCENT: Color = Color::Rgb(110, 228, 149);
const COLOR_BG: Color = Color::Black;
const COLOR_TEXT: Color = Color::White;
const COLOR_POPUP_THEME: Color = Color::Rgb(244, 209, 180); // #F4D1B4
const COLOR_POPUP_TEXT: Color = Color::Black;
const API_BASE: &str = "http://127.0.0.1:54001";
const BRIDGE_API_BASE: &str = "http://127.0.0.1:55002";
const BRIDGE_WS_LOG_URL: &str = "ws://127.0.0.1:55002/ws/logs";

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
    task_cancel_button: Option<Rect>,
    setting_rows: Vec<Rect>,
}

impl Default for UiRegions {
    fn default() -> Self {
        Self {
            home_tab: Rect::default(),
            task_tab: Rect::default(),
            settings_tab: Rect::default(),
            start_button: None,
            task_cancel_button: None,
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

#[derive(Debug, Clone, Serialize)]
struct PlannerTaskSubmitRequest {
    goal: String,
    provider: String,
    model: Option<String>,
    max_steps: i32,
}

#[derive(Debug, Clone, Deserialize)]
struct PlannerTaskSubmitResponse {
    task_id: String,
    status: String,
}

#[derive(Debug, Clone, Deserialize)]
struct PlannerTaskStatusResponse {
    status: String,
    stage: String,
    message: String,
    progress: u8,
    step_count: i32,
    max_steps: i32,
    error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct PlannerEvent {
    seq: i32,
    #[serde(rename = "type")]
    event_type: String,
    stage: String,
    message: String,
    timestamp: String,
}

#[derive(Debug, Clone, Deserialize)]
struct PlannerTaskEventsResponse {
    events: Vec<PlannerEvent>,
}

#[derive(Debug, Clone, Deserialize)]
struct PlannerTaskCancelResponse {
    task_id: String,
    cancelled: bool,
    status: String,
}

#[derive(Debug, Clone, Deserialize)]
struct BridgeHealthResponse {
    status: String,
    service: String,
    version: String,
}

struct App {
    running: bool,
    active_tab: Tab,
    status: String,
    settings_items: Vec<&'static str>,
    selected_setting: usize,
    ui_regions: UiRegions,
    popup: Popup,
    home_intro_input: String,
    started_at: Instant,
    current_task_id: Option<String>,
    task_status: String,
    task_stage: String,
    task_message: String,
    task_progress: u8,
    task_step_count: i32,
    task_max_steps: i32,
    task_error: String,
    task_events: Vec<PlannerEvent>,
    last_task_poll_at: Instant,
    last_bridge_probe_at: Instant,
    bridge_logs: Vec<String>,
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
            home_intro_input: String::new(),
            started_at: Instant::now(),
            current_task_id: None,
            task_status: "idle".to_string(),
            task_stage: "queued".to_string(),
            task_message: "等待任务开始".to_string(),
            task_progress: 0,
            task_step_count: 0,
            task_max_steps: 0,
            task_error: String::new(),
            task_events: Vec::new(),
            last_task_poll_at: Instant::now(),
            last_bridge_probe_at: Instant::now() - Duration::from_secs(10),
            bridge_logs: Vec::new(),
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
                self.start_home_task();
            }
            Tab::Task => {
                self.poll_task_updates();
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

    fn start_home_task(&mut self) {
        let requirement = self.home_intro_input.trim().to_string();
        if requirement.is_empty() {
            self.status = "请先输入任务需求".to_string();
            return;
        }

        match submit_planner_task(requirement.clone(), 12) {
            Ok(response) if response.status == "accepted" => {
                self.current_task_id = Some(response.task_id.clone());
                self.task_status = "pending".to_string();
                self.task_stage = "queued".to_string();
                self.task_message = "任务已提交，等待 bridge 执行".to_string();
                self.task_progress = 0;
                self.task_step_count = 0;
                self.task_max_steps = 12;
                self.task_error.clear();
                self.task_events.clear();
                self.last_task_poll_at = Instant::now() - Duration::from_secs(1);
                self.active_tab = Tab::Task;
                self.status = format!("任务已提交，task_id={}", response.task_id);
            }
            Err(e) => {
                self.status = format!("任务提交失败: {e}");
            }
            Ok(response) => {
                self.status = format!("任务提交失败：后端状态={}", response.status);
            }
        }
    }

    fn poll_task_updates(&mut self) {
        let Some(task_id) = self.current_task_id.clone() else {
            return;
        };
        if self.last_task_poll_at.elapsed() < Duration::from_millis(500) {
            return;
        }
        self.last_task_poll_at = Instant::now();

        match fetch_planner_task_status(&task_id) {
            Ok(task) => {
                self.task_status = task.status.clone();
                self.task_stage = task.stage.clone();
                self.task_message = task.message.clone();
                self.task_progress = task.progress;
                self.task_step_count = task.step_count;
                self.task_max_steps = task.max_steps;
                self.task_error = task.error.unwrap_or_default();
                self.status = format!(
                    "任务状态：{} | 阶段：{} | {}%",
                    self.task_status, self.task_stage, self.task_progress
                );
                if let Ok(events) = fetch_planner_task_events(&task_id) {
                    self.task_events = events.events;
                }
                if matches!(self.task_status.as_str(), "success" | "failed" | "cancelled" | "timeout")
                {
                    if !task.message.is_empty() {
                        self.status = format!("任务结束：{}（{}）", self.task_status, task.message);
                    }
                }
            }
            Err(e) => {
                self.status = format!("任务状态拉取失败: {e}");
            }
        }
    }

    fn poll_bridge_snapshot(&mut self) {
        if self.last_bridge_probe_at.elapsed() < Duration::from_secs(5) {
            return;
        }
        self.last_bridge_probe_at = Instant::now();
        match fetch_bridge_health() {
            Ok(health) => {
                let _ = (&health.status, &health.service, &health.version);
            }
            Err(e) => {
                self.status = format!("bridge 健康检查失败: {e}");
            }
        }
    }

    fn push_bridge_log(&mut self, line: String) {
        self.bridge_logs.push(line);
        if self.bridge_logs.len() > 300 {
            let keep_from = self.bridge_logs.len() - 300;
            self.bridge_logs = self.bridge_logs.split_off(keep_from);
        }
    }

    fn poll_runtime_logs(&mut self, log_rx: &Receiver<String>) {
        while let Ok(line) = log_rx.try_recv() {
            self.push_bridge_log(line);
        }
    }

    fn cancel_current_task(&mut self) {
        let Some(task_id) = self.current_task_id.clone() else {
            self.status = "暂无可取消任务".to_string();
            return;
        };
        match cancel_planner_task(&task_id) {
            Ok(resp) if resp.cancelled => {
                self.task_status = resp.status.clone();
                self.status = format!("已发送取消请求，task_id={}", resp.task_id);
            }
            Ok(resp) => {
                self.status = format!("取消失败，后端状态={}", resp.status);
            }
            Err(e) => {
                self.status = format!("取消任务失败: {e}");
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
            KeyCode::Backspace => {
                if self.active_tab == Tab::Home {
                    self.home_intro_input.pop();
                }
            }
            KeyCode::Char(c) => {
                if self.active_tab == Tab::Home && !c.is_control() {
                    self.home_intro_input.push(c);
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
            Tab::Task => {
                if let Some(btn) = self.ui_regions.task_cancel_button {
                    if in_rect(btn, x, y) {
                        self.cancel_current_task();
                    }
                }
            }
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

fn short_timeout_client() -> Result<Client> {
    Ok(Client::builder().timeout(Duration::from_millis(900)).build()?)
}

fn medium_timeout_client() -> Result<Client> {
    Ok(Client::builder().timeout(Duration::from_secs(3)).build()?)
}

fn fetch_service_providers() -> Result<Vec<ServiceProvider>> {
    let url = format!("{API_BASE}/services");
    let client = short_timeout_client()?;
    let resp = client.get(url).send()?.error_for_status()?;
    Ok(resp.json::<Vec<ServiceProvider>>()?)
}

fn add_service(input: NewServiceProvider) -> Result<i64> {
    #[derive(Deserialize)]
    struct AddServiceResponse {
        id: i64,
    }

    let url = format!("{API_BASE}/services");
    let client = short_timeout_client()?;
    let resp = client.post(url).json(&input).send()?.error_for_status()?;
    let body = resp.json::<AddServiceResponse>()?;
    Ok(body.id)
}

fn delete_service(id: i64) -> Result<bool> {
    #[derive(Deserialize)]
    struct DeleteServiceResponse {
        deleted: bool,
    }

    let url = format!("{API_BASE}/services/id/{id}");
    let client = short_timeout_client()?;
    let resp = client.delete(url).send()?.error_for_status()?;
    let body = resp.json::<DeleteServiceResponse>()?;
    Ok(body.deleted)
}

fn submit_planner_task(goal: String, max_steps: i32) -> Result<PlannerTaskSubmitResponse> {
    let url = format!("{BRIDGE_API_BASE}/planner/tasks");
    let client = short_timeout_client()?;
    let input = PlannerTaskSubmitRequest {
        goal,
        provider: "siliconflow".to_string(),
        model: None,
        max_steps,
    };
    let resp = client.post(url).json(&input).send()?.error_for_status()?;
    Ok(resp.json::<PlannerTaskSubmitResponse>()?)
}

fn fetch_planner_task_status(task_id: &str) -> Result<PlannerTaskStatusResponse> {
    let url = format!("{BRIDGE_API_BASE}/planner/tasks/{task_id}");
    let client = short_timeout_client()?;
    let resp = client.get(url).send()?.error_for_status()?;
    Ok(resp.json::<PlannerTaskStatusResponse>()?)
}

fn fetch_planner_task_events(task_id: &str) -> Result<PlannerTaskEventsResponse> {
    let url = format!("{BRIDGE_API_BASE}/planner/tasks/{task_id}/events");
    let client = short_timeout_client()?;
    let resp = client.get(url).send()?.error_for_status()?;
    Ok(resp.json::<PlannerTaskEventsResponse>()?)
}

fn cancel_planner_task(task_id: &str) -> Result<PlannerTaskCancelResponse> {
    let url = format!("{BRIDGE_API_BASE}/planner/tasks/{task_id}/cancel");
    let client = short_timeout_client()?;
    let resp = client.post(url).send()?.error_for_status()?;
    Ok(resp.json::<PlannerTaskCancelResponse>()?)
}

fn fetch_bridge_health() -> Result<BridgeHealthResponse> {
    let url = format!("{BRIDGE_API_BASE}/health");
    let client = short_timeout_client()?;
    let resp = client.get(url).send()?.error_for_status()?;
    Ok(resp.json::<BridgeHealthResponse>()?)
}

fn main() -> Result<()> {
    start_api_server();
    wait_for_api_ready()?;
    let mut bridge_child = ensure_bridge_server()?;
    let bridge_log_rx = start_bridge_log_stream();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run_app(&mut terminal, bridge_log_rx.as_ref());

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    if let Some(child) = bridge_child.as_mut() {
        let _ = child.kill();
    }

    result
}

fn wait_for_api_ready() -> Result<()> {
    let url = format!("{API_BASE}/health");
    let client = medium_timeout_client()?;
    for _ in 0..30 {
        if let Ok(resp) = client.get(&url).send() {
            if resp.status().is_success() {
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(anyhow::anyhow!("api server not ready"))
}

fn wait_for_bridge_ready() -> Result<()> {
    let url = format!("{BRIDGE_API_BASE}/health");
    let client = medium_timeout_client()?;
    for _ in 0..60 {
        if let Ok(resp) = client.get(&url).send() {
            if resp.status().is_success() {
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(anyhow::anyhow!("bridge server not ready"))
}

fn bridge_python_app_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("python")
}

fn ensure_bridge_server() -> Result<Option<Child>> {
    let url = format!("{BRIDGE_API_BASE}/health");
    let client = medium_timeout_client()?;
    if let Ok(resp) = client.get(&url).send() {
        if resp.status().is_success() {
            return Ok(None);
        }
    }

    let app_dir = bridge_python_app_dir();
    let app_dir = app_dir.to_string_lossy().to_string();
    let child = Command::new("conda")
        .args([
            "run",
            "-n",
            "omni",
            // 注意这是测试环境使用的命令，正式环境请替换为合适的启动命令
            "python",
            "-m",
            "uvicorn",
            "bridge.main:app",
            "--host",
            "127.0.0.1",
            "--port",
            "55002",
            "--app-dir",
        ])
        .arg(app_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    wait_for_bridge_ready()?;
    Ok(Some(child))
}

fn start_bridge_log_stream() -> Option<Receiver<String>> {
    let (tx, rx) = mpsc::channel::<String>();
    let builder = thread::Builder::new().name("bridge-log-ws".to_string());
    if builder
        .spawn(move || {
            loop {
                match connect(BRIDGE_WS_LOG_URL) {
                    Ok((mut socket, _)) => {
                        if tx.send("[ws] 已连接 bridge 实时日志".to_string()).is_err() {
                            break;
                        }
                        loop {
                            let next = socket.read();
                            match next {
                                Ok(Message::Text(text)) => {
                                    if tx.send(text.to_string()).is_err() {
                                        return;
                                    }
                                }
                                Ok(Message::Binary(bytes)) => {
                                    if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                                        if tx.send(text).is_err() {
                                            return;
                                        }
                                    }
                                }
                                Ok(Message::Close(_)) => {
                                    let _ = tx.send("[ws] bridge 日志连接已关闭，准备重连".to_string());
                                    break;
                                }
                                Ok(_) => {}
                                Err(err) => {
                                    if tx.send(format!("[ws] bridge 日志流错误: {err}")).is_err() {
                                        return;
                                    }
                                    break;
                                }
                            }
                        }
                    }
                    Err(err) => {
                        if tx
                            .send(format!("[ws] 连接 bridge 日志失败: {err}"))
                            .is_err()
                        {
                            break;
                        }
                    }
                }
                thread::sleep(Duration::from_secs(1));
            }
        })
        .is_err()
    {
        return None;
    }
    Some(rx)
}

fn start_api_server() {
    let db_path = default_db_path();
    let addr: SocketAddr = "127.0.0.1:54001"
        .parse()
        .expect("invalid api address");

    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().expect("failed to build tokio runtime");
        if let Err(err) = runtime.block_on(run_http_server(db_path, addr)) {
            eprintln!("api server failed: {err}");
        }
    });
}
fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    bridge_log_rx: Option<&Receiver<String>>,
) -> Result<()> {
    let mut app = App::new();

    while app.running {
        app.poll_task_updates();
        app.poll_bridge_snapshot();
        if let Some(rx) = bridge_log_rx {
            app.poll_runtime_logs(rx);
        }

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
        task_cancel_button: None,
        setting_rows: Vec::new(),
    };

    match app.active_tab {
        Tab::Home => {
            let input_focused = matches!(app.popup, Popup::None);
            let cursor_visible = (app.started_at.elapsed().as_millis() / 500) % 2 == 0;
            regions.start_button = render_home(
                f,
                chunks[1],
                &app.home_intro_input,
                input_focused,
                cursor_visible,
            );
        }
        Tab::Task => {
            regions.task_cancel_button = render_task(f, chunks[1], app);
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

fn render_home(
    f: &mut Frame<'_>,
    area: Rect,
    intro_input: &str,
    input_focused: bool,
    cursor_visible: bool,
) -> Option<Rect> {
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

    let mut input_spans = if intro_input.is_empty() {
        vec![Span::styled(
            "请输入任务需求...",
            Style::default().fg(Color::DarkGray),
        )]
    } else {
        vec![Span::styled(
            intro_input,
            Style::default()
                .fg(COLOR_TEXT)
                .bg(COLOR_BG)
                .add_modifier(Modifier::BOLD),
        )]
    };

    if input_focused && cursor_visible {
        input_spans.push(Span::styled(
            " ",
            Style::default().fg(COLOR_BG).bg(COLOR_ACCENT),
        ));
    }

    let intro_title = if input_focused {
        " 任务需求（输入中） "
    } else {
        " 任务需求（输入框） "
    };
    let intro_border_style = if input_focused {
        Style::default()
            .fg(COLOR_ACCENT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(COLOR_ACCENT)
    };

    let intro = Paragraph::new(vec![Line::from(input_spans)])
        .style(
            Style::default()
                .fg(COLOR_TEXT)
                .bg(COLOR_BG)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Left)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(intro_title)
                .border_style(intro_border_style),
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

fn render_task(f: &mut Frame<'_>, area: Rect, app: &App) -> Option<Rect> {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" 任务 ")
        .border_style(Style::default().fg(COLOR_ACCENT));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines = vec![
        Line::from("任务中心"),
        Line::from(format!(
            "task_id: {}",
            app.current_task_id
                .clone()
                .unwrap_or_else(|| "<暂无任务>".to_string())
        )),
        Line::from(format!("状态: {}", app.task_status)),
        Line::from(format!("阶段: {}", app.task_stage)),
        Line::from(format!("当前消息: {}", app.task_message)),
        Line::from(format!(
            "进度: {}% (步数 {}/{})",
            app.task_progress, app.task_step_count, app.task_max_steps
        )),
    ];
    if !app.task_error.is_empty() {
        lines.push(Line::from(format!("错误: {}", app.task_error)));
    }
    lines.push(Line::from("最近事件:"));
    for event in app.task_events.iter().rev().take(8).rev() {
        lines.push(Line::from(format!(
            "#{} [{}|{}] {} | {}",
            event.seq, event.event_type, event.stage, event.timestamp, event.message
        )));
    }
    lines.push(Line::from("实时Python日志(WS):"));
    for log in app.bridge_logs.iter().rev().take(6).rev() {
        lines.push(Line::from(log.clone()));
    }

    let content = Paragraph::new(lines)
        .style(Style::default().fg(COLOR_TEXT).bg(COLOR_BG))
        .alignment(Alignment::Left);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)])
        .split(inner);
    f.render_widget(content, sections[0]);

    let cancel_button = Rect::new(
        sections[1].x,
        sections[1].y,
        18.min(sections[1].width),
        sections[1].height.min(3),
    );
    let cancel_text = if app.current_task_id.is_some() {
        "[ 取消任务 ]"
    } else {
        "[ 暂无任务 ]"
    };
    let cancel_style = if app.current_task_id.is_some() {
        Style::default()
            .fg(COLOR_BG)
            .bg(COLOR_ACCENT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(COLOR_TEXT).bg(Color::DarkGray)
    };
    let cancel_btn = Paragraph::new(cancel_text)
        .alignment(Alignment::Center)
        .style(cancel_style)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COLOR_ACCENT)),
        );
    f.render_widget(cancel_btn, cancel_button);

    Some(cancel_button)
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











