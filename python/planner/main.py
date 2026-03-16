import argparse
import json
import re
import signal
import sqlite3
import threading
from pathlib import Path
from typing import Callable, Literal, TypedDict

from langchain_core.messages import HumanMessage, SystemMessage
from langchain_openai import ChatOpenAI
from langgraph.graph import END, StateGraph

try:
    from tools import (
        tool_capture_screen,
        tool_click_screen,
        tool_detect_clickable_buttons,
        tool_input_text,
        tool_llm,
        tool_scroll_wheel,
    )
except ImportError:
    from .tools import (
        tool_capture_screen,
        tool_click_screen,
        tool_detect_clickable_buttons,
        tool_input_text,
        tool_llm,
        tool_scroll_wheel,
    )


class GraphState(TypedDict):
    goal: str
    provider: str
    model: str | None
    thought: str
    plan: str
    decision: str
    tool_name: str
    tool_input: str
    tool_output: str
    screenshot_path: str
    latest_buttons: str
    final_answer: str
    error: str
    step_count: int
    max_steps: int


class ProviderConfig(TypedDict):
    provider_name: str
    model_name: str
    api_url: str
    api_key: str


class PlannerCancelledError(Exception):
    pass


EventCallback = Callable[[str, str, str], None]
CancelChecker = Callable[[], bool]


def default_db_path() -> Path:
    return Path(__file__).resolve().parents[2] / "data" / "job_agent.db"


def safe_json_loads(text: str) -> dict:
    text = text.strip()
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        match = re.search(r"\{[\s\S]*\}", text)
        if not match:
            return {}
        try:
            return json.loads(match.group(0))
        except json.JSONDecodeError:
            return {}


def build_buttons_context(state: GraphState) -> str:
    latest_buttons = state.get("latest_buttons", "").strip()
    if not latest_buttons:
        return "最近按钮候选: <empty>"
    try:
        parsed = json.loads(latest_buttons)
        normalized = json.dumps(parsed, ensure_ascii=False)
    except Exception:
        normalized = latest_buttons
    if len(normalized) > 12000:
        normalized = normalized[:12000] + "\n...<truncated>"
    return f"最近按钮候选JSON:\n{normalized}"


def load_provider_config(provider_name: str, db_path: Path | None = None) -> ProviderConfig:
    db = db_path or default_db_path()
    if not db.exists():
        raise FileNotFoundError(f"未找到数据库文件: {db}")

    con = sqlite3.connect(db)
    try:
        cur = con.cursor()
        cur.execute(
            """
            SELECT provider_name, model_name, api_url, api_key
            FROM service
            WHERE provider_name = ?
            ORDER BY id DESC
            LIMIT 1
            """,
            (provider_name,),
        )
        row = cur.fetchone()
    finally:
        con.close()

    if row is None:
        raise ValueError(f"服务商不存在: {provider_name}")

    return {
        "provider_name": row[0],
        "model_name": row[1],
        "api_url": row[2],
        "api_key": row[3],
    }


def build_llm(provider_name: str, model_override: str | None = None, db_path: Path | None = None) -> ChatOpenAI:
    cfg = load_provider_config(provider_name, db_path=db_path)
    model = model_override or cfg["model_name"]
    api_key = cfg["api_key"]
    api_url = cfg["api_url"]

    if not api_key:
        raise ValueError("api_key 为空，请在数据库中配置服务商")
    if not api_url:
        raise ValueError("api_url 为空，请在数据库中配置服务商")
    if not model:
        raise ValueError("model_name 为空，请在数据库中配置服务商")

    return ChatOpenAI(
        model=model,
        temperature=0,
        base_url=api_url,
        api_key=api_key,
    )


def _emit(event_callback: EventCallback | None, event_type: str, stage: str, message: str) -> None:
    if event_callback is not None:
        event_callback(event_type, stage, message)


def _check_cancel(cancel_checker: CancelChecker | None) -> None:
    if cancel_checker is not None and cancel_checker():
        raise PlannerCancelledError("任务已取消")


def build_graph(
    planner_llm: ChatOpenAI,
    event_callback: EventCallback | None = None,
    cancel_checker: CancelChecker | None = None,
):
    def think_node(state: GraphState) -> GraphState:
        print('我知道了需求')
        _check_cancel(cancel_checker)
        _emit(event_callback, "info", "planner_think", "开始思考任务目标")
        messages = [
            SystemMessage(content="你是电脑操作代理的思考器。请用中文简短分析当前目标与风险。"),
            HumanMessage(content=f"用户目标：{state['goal']}"),
        ]
        thought = planner_llm.invoke(messages).content
        _emit(event_callback, "info", "planner_think", "思考阶段完成")
        return {**state, "thought": thought}

    def plan_node(state: GraphState) -> GraphState:
        _check_cancel(cancel_checker)
        _emit(event_callback, "info", "planner_plan", "开始生成执行计划")
        messages = [
            SystemMessage(content="你是电脑操作代理的规划器。请输出简短的执行计划。"),
            HumanMessage(content=f"用户目标：{state['goal']}\n当前思考：{state['thought']}"),
        ]
        plan = planner_llm.invoke(messages).content
        _emit(event_callback, "info", "planner_plan", "规划阶段完成")
        return {**state, "plan": plan}

    def decide_node(state: GraphState) -> GraphState:
        _check_cancel(cancel_checker)
        _emit(event_callback, "info", "planner_decide", "开始进行动作决策")
        tool_schema = (
            "可用工具:\n"
            "1) capture_screen: 截图\n"
            "2) detect_clickable_buttons: 从截图中提取可点击元素\n"
            "3) click_screen: 点击屏幕坐标\n"
            "4) scroll_wheel: 滚动滚轮\n"
            "5) input_text: 输入文字\n"
            "6) llm: 通用语言模型请求\n"
            "如果任务已完成，decision 返回 finish；否则返回 tool。\n"
            "返回严格JSON: "
            "{\"decision\":\"tool或finish\",\"tool_name\":\"工具名\",\"tool_input\":\"字符串或JSON字符串\",\"reason\":\"原因\"}"
        )
        context = (
            f"用户目标：{state['goal']}\n"
            f"思考：{state['thought']}\n"
            f"规划：{state['plan']}\n"
            f"当前截图：{state['screenshot_path']}\n"
            f"{build_buttons_context(state)}\n"
            f"上次工具输出：{state['tool_output']}\n"
            f"当前错误：{state['error']}\n"
            f"当前步数：{state['step_count']}/{state['max_steps']}"
        )
        messages = [
            SystemMessage(content=f"你是电脑操作代理的决策器。\n{tool_schema}"),
            HumanMessage(content=context),
        ]
        raw = planner_llm.invoke(messages).content
        data = safe_json_loads(raw)
        decision = data.get("decision", "tool")
        tool_name = data.get("tool_name", "capture_screen")
        tool_input = data.get("tool_input", state["goal"])
        _emit(event_callback, "decision", "planner_decide", f"决策结果: {decision}, 工具: {tool_name}")
        return {
            **state,
            "decision": decision,
            "tool_name": tool_name,
            "tool_input": tool_input,
        }

    def execute_tool_node(state: GraphState) -> GraphState:
        _check_cancel(cancel_checker)
        name = state["tool_name"].strip()
        tool_input = state["tool_input"]
        next_step = state["step_count"] + 1
        _emit(event_callback, "info", "tool_execute", f"开始执行工具: {name}")
        try:
            if name == "capture_screen":
                output = tool_capture_screen(tool_input)
                _emit(event_callback, "progress", "capture_screen", f"STEP {next_step}/{state['max_steps']} 截图完成")
                return {
                    **state,
                    "tool_output": output,
                    "screenshot_path": output,
                    "error": "",
                    "step_count": next_step,
                }
            if name == "detect_clickable_buttons":
                output = tool_detect_clickable_buttons(tool_input, state["screenshot_path"])
                _emit(event_callback, "progress", "vision_parse", f"STEP {next_step}/{state['max_steps']} 可点击元素识别完成")
                return {
                    **state,
                    "tool_output": output,
                    "latest_buttons": output,
                    "error": "",
                    "step_count": next_step,
                }
            if name == "click_screen":
                output = tool_click_screen(tool_input)
                _emit(event_callback, "progress", "tool_execute", f"STEP {next_step}/{state['max_steps']} 点击完成")
                return {**state, "tool_output": output, "error": "", "step_count": next_step}
            if name == "scroll_wheel":
                output = tool_scroll_wheel(tool_input)
                _emit(event_callback, "progress", "tool_execute", f"STEP {next_step}/{state['max_steps']} 滚动完成")
                return {**state, "tool_output": output, "error": "", "step_count": next_step}
            if name == "input_text":
                output = tool_input_text(tool_input)
                _emit(event_callback, "progress", "tool_execute", f"STEP {next_step}/{state['max_steps']} 输入完成")
                return {**state, "tool_output": output, "error": "", "step_count": next_step}
            if name == "llm":
                output = tool_llm(tool_input, provider=state["provider"], model_override=state["model"])
                _emit(event_callback, "progress", "tool_execute", f"STEP {next_step}/{state['max_steps']} LLM请求完成")
                return {**state, "tool_output": output, "error": "", "step_count": next_step}

            _emit(event_callback, "error", "tool_execute", f"STEP {next_step}/{state['max_steps']} 未知工具: {name}")
            return {
                **state,
                "tool_output": "",
                "error": f"未知工具: {name}",
                "step_count": next_step,
            }
        except Exception as exc:
            _emit(event_callback, "error", "tool_execute", f"STEP {next_step}/{state['max_steps']} 工具执行失败: {exc}")
            return {
                **state,
                "tool_output": "",
                "error": str(exc),
                "step_count": next_step,
            }

    def route_after_decide(state: GraphState) -> Literal["execute_tool", "finalize"]:
        if state["decision"] == "finish":
            return "finalize"
        if state["step_count"] >= state["max_steps"]:
            return "finalize"
        return "execute_tool"

    def route_after_execute(state: GraphState) -> Literal["decide", "finalize"]:
        if state["step_count"] >= state["max_steps"]:
            return "finalize"
        return "decide"

    def finalize_node(state: GraphState) -> GraphState:
        _check_cancel(cancel_checker)
        _emit(event_callback, "info", "finalize", "开始生成最终总结")
        messages = [
            SystemMessage(content="你是电脑操作代理的最终总结器。请用中文总结执行情况、最后动作、是否完成目标和下一步建议。"),
            HumanMessage(
                content=(
                    f"用户目标：{state['goal']}\n"
                    f"思考：{state['thought']}\n"
                    f"规划：{state['plan']}\n"
                    f"最近工具：{state['tool_name']}\n"
                    f"最近工具输出：{state['tool_output']}\n"
                    f"{build_buttons_context(state)}\n"
                    f"截图路径：{state['screenshot_path']}\n"
                    f"错误：{state['error']}\n"
                    f"步数：{state['step_count']}/{state['max_steps']}"
                )
            ),
        ]
        final_answer = planner_llm.invoke(messages).content
        _emit(event_callback, "info", "finalize", "任务汇总完成")
        return {**state, "final_answer": final_answer}

    graph = StateGraph(GraphState)
    graph.add_node("think", think_node)
    graph.add_node("plan", plan_node)
    graph.add_node("decide", decide_node)
    graph.add_node("execute_tool", execute_tool_node)
    graph.add_node("finalize", finalize_node)

    graph.set_entry_point("think")
    graph.add_edge("think", "plan")
    graph.add_edge("plan", "decide")
    graph.add_conditional_edges("decide", route_after_decide, {"execute_tool": "execute_tool", "finalize": "finalize"})
    graph.add_conditional_edges("execute_tool", route_after_execute, {"decide": "decide", "finalize": "finalize"})
    graph.add_edge("finalize", END)
    return graph.compile()


def run_planner(
    goal: str,
    provider: str = "siliconflow",
    model: str | None = None,
    max_steps: int = 6,
    event_callback: EventCallback | None = None,
    cancel_checker: CancelChecker | None = None,
    db_path: Path | None = None,
) -> GraphState:
    if not goal.strip():
        raise ValueError("目标不能为空")

    planner_llm = build_llm(provider, model, db_path=db_path)
    graph = build_graph(planner_llm, event_callback=event_callback, cancel_checker=cancel_checker)
    init_state: GraphState = {
        "goal": goal,
        "provider": provider,
        "model": model,
        "thought": "",
        "plan": "",
        "decision": "",
        "tool_name": "",
        "tool_input": "",
        "tool_output": "",
        "screenshot_path": "",
        "latest_buttons": "",
        "final_answer": "",
        "error": "",
        "step_count": 0,
        "max_steps": max_steps,
    }
    return graph.invoke(init_state)


def main():
    parser = argparse.ArgumentParser(description="基于截图驱动的 LangGraph 电脑操作代理")
    parser.add_argument("--goal-input", default="", help="通过参数传入的用户目标")
    parser.add_argument("goal", nargs="?", help="用户目标（位置参数）")
    parser.add_argument("--provider", default="siliconflow", help="服务商名称（从 data/job_agent.db 读取）")
    parser.add_argument("--model", default="", help="覆盖数据库中的模型名称（默认使用数据库配置）")
    parser.add_argument("--max-steps", type=int, default=99, help="最大执行步数")
    parser.add_argument("--show-state", action="store_true", help="打印中间状态")
    parser.add_argument("--db-path", default=str(default_db_path()), help="SQLite路径")
    args = parser.parse_args()

    # 使用事件标记统一管理取消状态，供 LangGraph 轮询检测。
    cancel_event = threading.Event()

    # 捕获 Ctrl+C / 终止信号，先标记取消，再抛出中断以便尽快退出阻塞调用。
    def _handle_stop(_signum, _frame):
        cancel_event.set()
        print("\n收到停止信号，正在中断任务...")
        raise KeyboardInterrupt

    signal.signal(signal.SIGINT, _handle_stop)
    if hasattr(signal, "SIGTERM"):
        signal.signal(signal.SIGTERM, _handle_stop)

    goal = (args.goal_input or args.goal or "").strip()
    if not goal:
        goal = input("请输入你的电脑操作目标：").strip()

    try:
        result = run_planner(
            goal=goal,
            provider=args.provider,
            model=args.model if args.model else None,
            max_steps=args.max_steps,
            cancel_checker=lambda: cancel_event.is_set(),
            db_path=Path(args.db_path),
        )
    except PlannerCancelledError:
        print("任务已取消。")
        return
    except KeyboardInterrupt:
        print("任务已被 Ctrl+C 中断。")
        return

    if args.show_state:
        print("=== 思考 ===")
        print(result["thought"])
        print("\n=== 规划 ===")
        print(result["plan"])
        print("\n=== 最近决策 ===")
        print(result["decision"])
        print("\n=== 最近工具 ===")
        print(result["tool_name"])
        print("\n=== 最近工具输出 ===")
        print(result["tool_output"])
        print("\n=== 最近截图 ===")
        print(result["screenshot_path"])
        print("\n=== 最近按钮候选 ===")
        print(result["latest_buttons"])
        print("\n=== 最终结果 ===")

    print(result["final_answer"])


if __name__ == "__main__":
    main()

