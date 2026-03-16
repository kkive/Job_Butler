import time
from importlib.util import module_from_spec, spec_from_file_location
from pathlib import Path

from fastapi.testclient import TestClient

BRIDGE_MAIN_PATH = Path(__file__).resolve().parent / "main.py"
BRIDGE_MAIN_SPEC = spec_from_file_location("bridge_main_module", BRIDGE_MAIN_PATH)
assert BRIDGE_MAIN_SPEC is not None and BRIDGE_MAIN_SPEC.loader is not None
bridge_main = module_from_spec(BRIDGE_MAIN_SPEC)
BRIDGE_MAIN_SPEC.loader.exec_module(bridge_main)


def wait_for_task_finish(client: TestClient, task_id: str, timeout_sec: float = 2.0):
    deadline = time.time() + timeout_sec
    while time.time() < deadline:
        resp = client.get(f"/planner/tasks/{task_id}")
        assert resp.status_code == 200
        body = resp.json()
        if body["status"] in {"success", "failed", "cancelled"}:
            return body
        time.sleep(0.05)
    raise AssertionError("任务未在超时内结束")


def test_bridge_health():
    client = TestClient(bridge_main.app)
    resp = client.get("/health")
    assert resp.status_code == 200
    assert resp.json()["status"] == "ok"


def test_bridge_submit_and_status_and_events():
    bridge_main.task_store.clear()

    def fake_runner(goal, provider, model, max_steps, event_callback, cancel_checker):
        event_callback("info", "planner_think", "fake think")
        return {
            "goal": goal,
            "thought": "ok",
            "plan": "ok",
            "decision": "finish",
            "tool_name": "",
            "tool_input": "",
            "tool_output": "",
            "screenshot_path": "",
            "latest_buttons": "",
            "final_answer": "done",
            "error": "",
            "step_count": 1,
            "max_steps": max_steps,
        }

    bridge_main.planner_runner = fake_runner
    client = TestClient(bridge_main.app)

    submit_resp = client.post(
        "/planner/tasks",
        json={
            "goal": "测试任务",
            "provider": "siliconflow",
            "max_steps": 3,
        },
    )
    assert submit_resp.status_code == 200
    task_id = submit_resp.json()["task_id"]

    status = wait_for_task_finish(client, task_id)
    assert status["status"] == "success"
    assert status["result"]["final_answer"] == "done"

    events_resp = client.get(f"/planner/tasks/{task_id}/events")
    assert events_resp.status_code == 200
    events = events_resp.json()["events"]
    assert len(events) >= 1


def test_bridge_cancel():
    bridge_main.task_store.clear()

    def slow_runner(goal, provider, model, max_steps, event_callback, cancel_checker):
        for _ in range(20):
            if cancel_checker():
                raise bridge_main.PlannerCancelledError("任务已取消")
            time.sleep(0.02)
        return {
            "goal": goal,
            "thought": "",
            "plan": "",
            "decision": "finish",
            "tool_name": "",
            "tool_input": "",
            "tool_output": "",
            "screenshot_path": "",
            "latest_buttons": "",
            "final_answer": "done",
            "error": "",
            "step_count": 1,
            "max_steps": max_steps,
        }

    bridge_main.planner_runner = slow_runner
    client = TestClient(bridge_main.app)
    submit_resp = client.post("/planner/tasks", json={"goal": "取消测试"})
    assert submit_resp.status_code == 200
    task_id = submit_resp.json()["task_id"]

    cancel_resp = client.post(f"/planner/tasks/{task_id}/cancel")
    assert cancel_resp.status_code == 200

    final_status = wait_for_task_finish(client, task_id)
    assert final_status["status"] == "cancelled"
