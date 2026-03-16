import asyncio
import io
import threading
import time
import re
from collections import deque
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any

from fastapi import FastAPI, HTTPException, WebSocket, WebSocketDisconnect
from pydantic import BaseModel, Field

PLANNER_DIR = Path(__file__).resolve().parents[1] / "planner"
if str(PLANNER_DIR) not in __import__("sys").path:
    __import__("sys").path.insert(0, str(PLANNER_DIR))

from service import PlannerCancelledError, run_planner


def now_iso() -> str:
    return datetime.now().astimezone().isoformat(timespec="seconds")


@dataclass
class TaskRecord:
    task_id: str
    goal: str
    provider: str
    model: str | None
    max_steps: int
    status: str = "pending"
    stage: str = "queued"
    message: str = "任务已创建"
    progress: int = 0
    step_count: int = 0
    error: str | None = None
    result: dict[str, Any] | None = None
    cancelled: bool = False
    created_at: str = field(default_factory=now_iso)
    started_at: str | None = None
    updated_at: str = field(default_factory=now_iso)
    finished_at: str | None = None
    events: list[dict[str, Any]] = field(default_factory=list)


class SubmitTaskRequest(BaseModel):
    goal: str = Field(min_length=1)
    provider: str = "siliconflow"
    model: str | None = None
    max_steps: int = Field(default=6, ge=1, le=50)


class SubmitTaskResponse(BaseModel):
    task_id: str
    status: str


class TaskStatusResponse(BaseModel):
    task_id: str
    status: str
    stage: str
    message: str
    progress: int
    step_count: int
    max_steps: int
    error: str | None
    result: dict[str, Any] | None
    created_at: str
    started_at: str | None
    updated_at: str
    finished_at: str | None


class TaskEventsResponse(BaseModel):
    task_id: str
    events: list[dict[str, Any]]


class CancelTaskResponse(BaseModel):
    task_id: str
    cancelled: bool
    status: str


app = FastAPI(title="Job-Agent Python Bridge", version="0.1.0")
executor = ThreadPoolExecutor(max_workers=4)
planner_runner = run_planner
task_store: dict[str, TaskRecord] = {}
task_lock = threading.Lock()


class RuntimeLogHub:
    def __init__(self, max_lines: int = 300):
        self._history: deque[str] = deque(maxlen=max_lines)
        self._subscribers: dict[int, tuple[asyncio.AbstractEventLoop, asyncio.Queue[str]]] = {}
        self._next_subscriber_id = 1
        self._lock = threading.Lock()

    def emit(self, line: str) -> None:
        if not line:
            return
        with self._lock:
            self._history.append(line)
            subscribers = list(self._subscribers.items())
        for _, (loop, queue) in subscribers:
            try:
                loop.call_soon_threadsafe(self._push_queue, queue, line)
            except RuntimeError:
                continue

    @staticmethod
    def _push_queue(queue: asyncio.Queue[str], line: str) -> None:
        try:
            queue.put_nowait(line)
        except asyncio.QueueFull:
            try:
                queue.get_nowait()
            except asyncio.QueueEmpty:
                pass
            try:
                queue.put_nowait(line)
            except asyncio.QueueFull:
                pass

    def subscribe(self) -> tuple[int, asyncio.Queue[str], list[str]]:
        queue: asyncio.Queue[str] = asyncio.Queue(maxsize=200)
        loop = asyncio.get_running_loop()
        with self._lock:
            subscriber_id = self._next_subscriber_id
            self._next_subscriber_id += 1
            self._subscribers[subscriber_id] = (loop, queue)
            history = list(self._history)
        return subscriber_id, queue, history

    def unsubscribe(self, subscriber_id: int) -> None:
        with self._lock:
            self._subscribers.pop(subscriber_id, None)


runtime_log_hub = RuntimeLogHub()


class RuntimeStreamProxy(io.TextIOBase):
    def __init__(self, stream: io.TextIOBase, stream_name: str):
        self._stream = stream
        self._stream_name = stream_name
        self._buffer = ""

    @property
    def encoding(self) -> str | None:
        return self._stream.encoding

    def writable(self) -> bool:
        return True

    def write(self, s: str) -> int:
        written = self._stream.write(s)
        self._stream.flush()
        self._buffer += s
        while "\n" in self._buffer:
            line, rest = self._buffer.split("\n", 1)
            runtime_log_hub.emit(f"[{self._stream_name}] {line}")
            self._buffer = rest
        return written

    def flush(self) -> None:
        self._stream.flush()
        if self._buffer:
            runtime_log_hub.emit(f"[{self._stream_name}] {self._buffer}")
            self._buffer = ""


def append_event(task: TaskRecord, event_type: str, stage: str, message: str) -> None:
    task.events.append(
        {
            "seq": len(task.events) + 1,
            "type": event_type,
            "stage": stage,
            "message": message,
            "timestamp": now_iso(),
        }
    )
    task.updated_at = now_iso()
    runtime_log_hub.emit(f"[planner_event] {event_type} | {stage} | {message}")


def snapshot_status(task: TaskRecord) -> TaskStatusResponse:
    return TaskStatusResponse(
        task_id=task.task_id,
        status=task.status,
        stage=task.stage,
        message=task.message,
        progress=task.progress,
        step_count=task.step_count,
        max_steps=task.max_steps,
        error=task.error,
        result=task.result,
        created_at=task.created_at,
        started_at=task.started_at,
        updated_at=task.updated_at,
        finished_at=task.finished_at,
    )


def task_event_callback(task_id: str, event_type: str, stage: str, message: str) -> None:
    with task_lock:
        task = task_store.get(task_id)
        if task is None:
            return
        task.stage = stage
        task.message = message
        step_match = re.search(r"STEP\s+(\d+)/(\d+)", message)
        if step_match:
            step_count = int(step_match.group(1))
            max_steps = int(step_match.group(2))
            task.step_count = step_count
            task.max_steps = max_steps
            if max_steps > 0:
                task.progress = min(99, int(step_count * 100 / max_steps))
        append_event(task, event_type, stage, message)


def run_task(task_id: str) -> None:
    with task_lock:
        task = task_store[task_id]
        task.status = "running"
        task.stage = "planner_think"
        task.message = "任务开始执行"
        task.started_at = now_iso()
        task.updated_at = now_iso()
        append_event(task, "info", "planner_start", "任务开始执行")

    try:
        result = planner_runner(
            goal=task.goal,
            provider=task.provider,
            model=task.model,
            max_steps=task.max_steps,
            event_callback=lambda event_type, stage, message: task_event_callback(
                task_id, event_type, stage, message
            ),
            cancel_checker=lambda: task_store[task_id].cancelled,
        )
        with task_lock:
            task = task_store[task_id]
            if task.cancelled:
                task.status = "cancelled"
                task.stage = "cancelled"
                task.message = "任务已取消"
                task.progress = 0
                append_event(task, "info", "cancelled", "任务已取消")
            else:
                task.status = "success"
                task.stage = "finalize"
                task.message = "任务执行完成"
                task.step_count = result.get("step_count", task.step_count)
                task.progress = 100
                task.result = dict(result)
                append_event(task, "info", "finalize", "任务执行完成")
            task.finished_at = now_iso()
            task.updated_at = now_iso()
    except PlannerCancelledError:
        with task_lock:
            task = task_store[task_id]
            task.status = "cancelled"
            task.stage = "cancelled"
            task.message = "任务已取消"
            task.finished_at = now_iso()
            task.updated_at = now_iso()
            append_event(task, "info", "cancelled", "任务已取消")
    except Exception as exc:
        with task_lock:
            task = task_store[task_id]
            task.status = "failed"
            task.stage = "failed"
            task.message = "任务执行失败"
            task.error = str(exc)
            task.finished_at = now_iso()
            task.updated_at = now_iso()
            append_event(task, "error", "failed", f"任务失败: {exc}")


@app.get("/health")
def health() -> dict[str, str]:
    return {
        "status": "ok",
        "service": "job-agent-python-bridge",
        "version": "0.1.0",
    }


@app.on_event("startup")
def setup_runtime_streams() -> None:
    if not isinstance(__import__("sys").stdout, RuntimeStreamProxy):
        __import__("sys").stdout = RuntimeStreamProxy(__import__("sys").stdout, "stdout")
    if not isinstance(__import__("sys").stderr, RuntimeStreamProxy):
        __import__("sys").stderr = RuntimeStreamProxy(__import__("sys").stderr, "stderr")
    runtime_log_hub.emit("[system] bridge runtime logger started")


@app.websocket("/ws/logs")
async def websocket_runtime_logs(websocket: WebSocket) -> None:
    await websocket.accept()
    subscriber_id, queue, history = runtime_log_hub.subscribe()
    try:
        for line in history:
            await websocket.send_text(line)
        while True:
            line = await queue.get()
            await websocket.send_text(line)
    except WebSocketDisconnect:
        pass
    finally:
        runtime_log_hub.unsubscribe(subscriber_id)


@app.post("/planner/tasks", response_model=SubmitTaskResponse)
def submit_planner_task(req: SubmitTaskRequest) -> SubmitTaskResponse:
    task_id = f"planner_{int(time.time() * 1000)}"
    task = TaskRecord(
        task_id=task_id,
        goal=req.goal.strip(),
        provider=req.provider,
        model=req.model,
        max_steps=req.max_steps,
    )
    if not task.goal:
        raise HTTPException(status_code=400, detail="goal 不能为空")

    with task_lock:
        task_store[task_id] = task
    executor.submit(run_task, task_id)
    return SubmitTaskResponse(task_id=task_id, status="accepted")


@app.get("/planner/tasks/{task_id}", response_model=TaskStatusResponse)
def get_planner_task(task_id: str) -> TaskStatusResponse:
    with task_lock:
        task = task_store.get(task_id)
        if task is None:
            raise HTTPException(status_code=404, detail="task not found")
        return snapshot_status(task)


@app.get("/planner/tasks/{task_id}/events", response_model=TaskEventsResponse)
def get_planner_task_events(task_id: str) -> TaskEventsResponse:
    with task_lock:
        task = task_store.get(task_id)
        if task is None:
            raise HTTPException(status_code=404, detail="task not found")
        return TaskEventsResponse(task_id=task_id, events=list(task.events))


@app.post("/planner/tasks/{task_id}/cancel", response_model=CancelTaskResponse)
def cancel_planner_task(task_id: str) -> CancelTaskResponse:
    with task_lock:
        task = task_store.get(task_id)
        if task is None:
            raise HTTPException(status_code=404, detail="task not found")
        task.cancelled = True
        if task.status in {"pending", "running"}:
            task.status = "cancelled"
            task.stage = "cancelled"
            task.message = "收到取消请求"
            task.updated_at = now_iso()
            append_event(task, "info", "cancelled", "收到取消请求")
        return CancelTaskResponse(task_id=task_id, cancelled=True, status=task.status)
