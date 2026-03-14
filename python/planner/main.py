import argparse
import base64
import json
import os
import re
import sys
import time
from pathlib import Path
from typing import Literal, TypedDict

import requests
from langchain_core.messages import HumanMessage, SystemMessage
from langchain_openai import ChatOpenAI
from langgraph.graph import END, StateGraph

OMNIPARSER_ROOT = Path("python/OmniParser-master").resolve()
OMNIPARSER_UTIL = OMNIPARSER_ROOT / "util"

if str(OMNIPARSER_UTIL) not in sys.path:
    sys.path.insert(0, str(OMNIPARSER_UTIL))

try:
    from omniparser import Omniparser
except Exception:
    Omniparser = None


class GraphState(TypedDict):
    goal: str
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


def load_provider_config(provider_name: str, db_path: str | None = None) -> ProviderConfig:
    api_base = os.getenv("JOB_AGENT_API_BASE", "http://127.0.0.1:54001")
    url = f"{api_base}/services/{provider_name}"
    resp = requests.get(url, timeout=10)
    if resp.status_code == 404:
        raise ValueError(f"服务商不存在: {provider_name}")
    resp.raise_for_status()
    return resp.json()


def build_llm(provider_name: str, model_override: str | None = None) -> ChatOpenAI:
    cfg = load_provider_config(provider_name)
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


def require_pyautogui():
    try:
        import pyautogui  # type: ignore
    except ModuleNotFoundError as exc:
        raise ModuleNotFoundError("缺少依赖 pyautogui，请先执行 pip install pyautogui") from exc
    return pyautogui


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


def encode_image(image_path: str) -> str:
    with open(image_path, "rb") as file:
        return base64.b64encode(file.read()).decode("utf-8")


def tool_capture_screen(tool_input: str) -> str:
    # 使用时间戳生成截图文件名，避免覆盖历史截图
    pyautogui = require_pyautogui()
    output_dir = Path("captures")
    output_dir.mkdir(exist_ok=True)
    timestamp = time.strftime("%Y%m%d_%H%M%S")
    output_path = output_dir / f"screen_{timestamp}.png"
    image = pyautogui.screenshot()
    image.save(output_path)
    return str(output_path.resolve())


def _init_omniparser() -> Omniparser:
    if Omniparser is None:
        raise RuntimeError("OmniParser 不可用，请确认依赖和路径")

    config = {
        "som_model_path": str(OMNIPARSER_ROOT / "weights" / "icon_detect" / "model.pt"),
        "caption_model_name": "florence2",
        "caption_model_path": str(OMNIPARSER_ROOT / "weights" / "icon_caption"),
        "BOX_TRESHOLD": 0.01,
    }
    return Omniparser(config)


def tool_detect_clickable_buttons(tool_input: str, screenshot_path: str) -> str:
    if not screenshot_path or not Path(screenshot_path).exists():
        raise ValueError("当前没有可用截图，请先执行截图工具")

    parser = _init_omniparser()
    image_base64 = encode_image(screenshot_path)
    _, parsed_content = parser.parse(image_base64)

    items = []
    from PIL import Image

    with Image.open(screenshot_path) as img:
        width, height = img.size

    for idx, elem in enumerate(parsed_content):
        bbox = elem.get("bbox") if isinstance(elem, dict) else None
        content = elem.get("content") if isinstance(elem, dict) else ""
        interactable = elem.get("interactivity", True) if isinstance(elem, dict) else True
        if not bbox or len(bbox) < 4:
            continue
        x1, y1, x2, y2 = bbox[:4]
        cx = int(((x1 + x2) / 2) * width)
        cy = int(((y1 + y2) / 2) * height)
        items.append({
            "name": content or f"item_{idx}",
            "x": cx,
            "y": cy,
            "reason": "omniparser-detected",
            "interactable": interactable,
        })

    return json.dumps({"items": items}, ensure_ascii=False)


def tool_click_screen(tool_input: str) -> str:
    pyautogui = require_pyautogui()
    data = safe_json_loads(tool_input)
    x = data.get("x")
    y = data.get("y")
    if x is None or y is None:
        raise ValueError("点击工具需要 JSON 输入，例如 {\"x\":100,\"y\":200}")
    pyautogui.click(int(x), int(y))
    return f"已点击坐标({int(x)}, {int(y)})"


def tool_scroll_wheel(tool_input: str) -> str:
    pyautogui = require_pyautogui()
    data = safe_json_loads(tool_input)
    clicks = int(data.get("clicks", 0))
    if clicks == 0:
        raise ValueError("滚轮工具需要 JSON 输入，例如 {\"clicks\":-500}")
    pyautogui.scroll(clicks)
    return f"已滚动滚轮 {clicks}"


def tool_input_text(tool_input: str) -> str:
    pyautogui = require_pyautogui()
    data = safe_json_loads(tool_input)
    text = data.get("text", "")
    interval = float(data.get("interval", 0.03))
    if not text:
        raise ValueError("输入工具需要 JSON 输入，例如 {\"text\":\"你好\"}")
    pyautogui.write(text, interval=interval)
    return f"已输入文本：{text}"


def build_graph(planner_llm: ChatOpenAI):
    def think_node(state: GraphState) -> GraphState:
        messages = [
            SystemMessage(content="你是电脑操作代理的思考器。请用中文简短分析当前目标与风险。"),
            HumanMessage(content=f"用户目标：{state['goal']}"),
        ]
        thought = planner_llm.invoke(messages).content
        return {**state, "thought": thought}

    def plan_node(state: GraphState) -> GraphState:
        messages = [
            SystemMessage(content="你是电脑操作代理的规划器。请输出简短的执行计划。"),
            HumanMessage(content=f"用户目标：{state['goal']}\n当前思考：{state['thought']}"),
        ]
        plan = planner_llm.invoke(messages).content
        return {**state, "plan": plan}

    def decide_node(state: GraphState) -> GraphState:
        tool_schema = (
            "可用工具：\n"
            "1) capture_screen: 截图\n"
            "2) detect_clickable_buttons: 根据截图输出可点击按钮\n"
            "3) click_screen: 点击屏幕坐标\n"
            "4) scroll_wheel: 滚动滚轮\n"
            "5) input_text: 输入文字\n"
            "如果任务已完成，decision 返回 finish。\n"
            "否则 decision 返回 tool。\n"
            "返回严格 JSON："
            "{\"decision\":\"tool或finish\",\"tool_name\":\"工具名\",\"tool_input\":\"字符串或JSON字符串\",\"reason\":\"原因\"}"
        )
        context = (
            f"用户目标：{state['goal']}\n"
            f"思考：{state['thought']}\n"
            f"规划：{state['plan']}\n"
            f"当前截图：{state['screenshot_path']}\n"
            f"最近按钮候选：{state['latest_buttons']}\n"
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
        return {
            **state,
            "decision": decision,
            "tool_name": tool_name,
            "tool_input": tool_input,
        }

    def execute_tool_node(state: GraphState) -> GraphState:
        name = state["tool_name"].strip()
        tool_input = state["tool_input"]
        try:
            if name == "capture_screen":
                output = tool_capture_screen(tool_input)
                return {
                    **state,
                    "tool_output": output,
                    "screenshot_path": output,
                    "error": "",
                    "step_count": state["step_count"] + 1,
                }
            if name == "detect_clickable_buttons":
                output = tool_detect_clickable_buttons(tool_input, state["screenshot_path"])
                return {
                    **state,
                    "tool_output": output,
                    "latest_buttons": output,
                    "error": "",
                    "step_count": state["step_count"] + 1,
                }
            if name == "click_screen":
                output = tool_click_screen(tool_input)
                return {
                    **state,
                    "tool_output": output,
                    "error": "",
                    "step_count": state["step_count"] + 1,
                }
            if name == "scroll_wheel":
                output = tool_scroll_wheel(tool_input)
                return {
                    **state,
                    "tool_output": output,
                    "error": "",
                    "step_count": state["step_count"] + 1,
                }
            if name == "input_text":
                output = tool_input_text(tool_input)
                return {
                    **state,
                    "tool_output": output,
                    "error": "",
                    "step_count": state["step_count"] + 1,
                }
            return {
                **state,
                "tool_output": "",
                "error": f"未知工具：{name}",
                "step_count": state["step_count"] + 1,
            }
        except Exception as exc:
            return {
                **state,
                "tool_output": "",
                "error": str(exc),
                "step_count": state["step_count"] + 1,
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
        messages = [
            SystemMessage(
                content=(
                    "你是电脑操作代理的最终汇总器。"
                    "请用中文总结当前执行情况、最后一次动作、是否完成目标、以及下一步建议。"
                )
            ),
            HumanMessage(
                content=(
                    f"用户目标：{state['goal']}\n"
                    f"思考：{state['thought']}\n"
                    f"规划：{state['plan']}\n"
                    f"最近工具：{state['tool_name']}\n"
                    f"最近工具输出：{state['tool_output']}\n"
                    f"最近按钮候选：{state['latest_buttons']}\n"
                    f"截图路径：{state['screenshot_path']}\n"
                    f"错误：{state['error']}\n"
                    f"步数：{state['step_count']}/{state['max_steps']}"
                )
            ),
        ]
        final_answer = planner_llm.invoke(messages).content
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
    graph.add_conditional_edges(
        "decide",
        route_after_decide,
        {"execute_tool": "execute_tool", "finalize": "finalize"},
    )
    graph.add_conditional_edges(
        "execute_tool",
        route_after_execute,
        {"decide": "decide", "finalize": "finalize"},
    )
    graph.add_edge("finalize", END)
    return graph.compile()


def main():
    parser = argparse.ArgumentParser(description="基于截图驱动的 LangGraph 电脑操作代理")
    parser.add_argument("goal", nargs="?", help="你的电脑操作目标")
    parser.add_argument(
        "--provider",
        default="siliconflow",
        help="服务商名称（从数据库读取 api_url/api_key/model_name）",
    )
    parser.add_argument(
        "--model",
        default="",
        help="覆盖数据库中的模型名称（默认使用数据库配置）",
    )
    parser.add_argument(
        "--max-steps",
        type=int,
        default=6,
        help="最大执行步数",
    )
    parser.add_argument(
        "--show-state",
        action="store_true",
        help="打印中间状态",
    )
    args = parser.parse_args()

    goal = args.goal or input("请输入你的电脑操作目标：").strip()
    if not goal:
        raise ValueError("目标不能为空")

    planner_llm = build_llm(args.provider, args.model if args.model else None)
    graph = build_graph(planner_llm)

    init_state: GraphState = {
        "goal": goal,
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
        "max_steps": args.max_steps,
    }
    result = graph.invoke(init_state)

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
