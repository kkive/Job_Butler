import argparse
import json
import os
import re
import subprocess
import tempfile
from typing import Literal, TypedDict

from langchain_core.messages import HumanMessage, SystemMessage
from langchain_openai import ChatOpenAI
from langgraph.graph import END, StateGraph


class GraphState(TypedDict):
    question: str
    thought: str
    plan: str
    tool_name: str
    tool_input: str
    tool_output: str
    final_answer: str
    error: str
    retries: int


def build_llm(model: str) -> ChatOpenAI:
    api_key = os.getenv("SILICONFLOW_API_KEY") or os.getenv("OPENAI_API_KEY")
    if not api_key:
        raise ValueError("请先设置 SILICONFLOW_API_KEY 或 OPENAI_API_KEY")
    return ChatOpenAI(
        model=model,
        temperature=0,
        base_url="https://api.siliconflow.cn/v1",
        api_key=api_key,
    )


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


def tool_generate_code(task: str, llm: ChatOpenAI) -> str:
    messages = [
        SystemMessage(content="你是 使用电脑很厉害 工程师。根据用户需求，生成完整的操作步骤逻辑，不要解释。"),
        HumanMessage(content=task),
    ]
    return llm.invoke(messages).content


def tool_explain_code(task: str, llm: ChatOpenAI) -> str:
    messages = [
        SystemMessage(content="你是代码讲解助手。请用中文分点说明思路与关键实现。"),
        HumanMessage(content=task),
    ]
    return llm.invoke(messages).content


def tool_run_python(code: str) -> str:
    with tempfile.NamedTemporaryFile(mode="w", suffix=".py", delete=False, encoding="utf-8") as f:
        f.write(code)
        temp_path = f.name
    try:
        result = subprocess.run(
            ["python", temp_path],
            capture_output=True,
            text=True,
            timeout=15,
            check=False,
        )
        stdout = result.stdout.strip()
        stderr = result.stderr.strip()
        return (
            f"exit_code={result.returncode}\n"
            f"stdout:\n{stdout if stdout else '(empty)'}\n"
            f"stderr:\n{stderr if stderr else '(empty)'}"
        )
    finally:
        try:
            os.remove(temp_path)
        except OSError:
            pass


def build_graph(llm: ChatOpenAI):
    def think_node(state: GraphState) -> GraphState:
        messages = [
            SystemMessage(content="你是任务分析器。请给出简短思考过程（3-5行）。"),
            HumanMessage(content=state["question"]),
        ]
        thought = llm.invoke(messages).content
        return {**state, "thought": thought}

    def plan_node(state: GraphState) -> GraphState:
        messages = [
            SystemMessage(content="你是规划器。请输出一个简洁可执行的步骤计划。"),
            HumanMessage(content=f"用户需求：{state['question']}\n思考：{state['thought']}"),
        ]
        plan = llm.invoke(messages).content
        return {**state, "plan": plan}

    def select_tool_node(state: GraphState) -> GraphState:
        tool_schema = (
            "可用工具:\n"
            "1) generate_code: 生成 Python 代码\n"
            "2) explain_code: 解释代码或方案\n"
            "3) run_python: 执行 Python 代码\n"
            "返回严格 JSON: {\"tool_name\":\"...\",\"tool_input\":\"...\"}"
        )
        messages = [
            SystemMessage(content=f"你是工具路由器。\n{tool_schema}"),
            HumanMessage(
                content=(
                    f"用户需求：{state['question']}\n"
                    f"思考：{state['thought']}\n"
                    f"规划：{state['plan']}"
                )
            ),
        ]
        raw = llm.invoke(messages).content
        data = safe_json_loads(raw)
        tool_name = data.get("tool_name", "generate_code")
        tool_input = data.get("tool_input", state["question"])
        return {**state, "tool_name": tool_name, "tool_input": tool_input}

    def execute_tool_node(state: GraphState) -> GraphState:
        name = state["tool_name"].strip()
        tool_input = state["tool_input"]
        try:
            if name == "generate_code":
                output = tool_generate_code(tool_input, llm)
            elif name == "explain_code":
                output = tool_explain_code(tool_input, llm)
            elif name == "run_python":
                output = tool_run_python(tool_input)
            else:
                output = f"未知工具: {name}"
            return {**state, "tool_output": output, "error": ""}
        except Exception as e:  # noqa: BLE001
            return {**state, "error": str(e), "retries": state["retries"] + 1}

    def route_after_execute(state: GraphState) -> Literal["select_tool", "finalize"]:
        if state.get("error") and state.get("retries", 0) < 1:
            return "select_tool"
        return "finalize"

    def finalize_node(state: GraphState) -> GraphState:
        messages = [
            SystemMessage(
                content=(
                    "你是最终答复器。整合思考、规划、工具执行结果，"
                    "输出最终可交付结果。若工具输出是代码，直接给代码。"
                )
            ),
            HumanMessage(
                content=(
                    f"用户需求：{state['question']}\n"
                    f"思考：{state['thought']}\n"
                    f"规划：{state['plan']}\n"
                    f"工具：{state['tool_name']}\n"
                    f"工具输入：{state['tool_input']}\n"
                    f"工具输出：{state['tool_output']}\n"
                    f"错误：{state['error']}"
                )
            ),
        ]
        answer = llm.invoke(messages).content
        return {**state, "final_answer": answer}

    graph = StateGraph(GraphState)
    graph.add_node("think", think_node)
    graph.add_node("plan", plan_node)
    graph.add_node("select_tool", select_tool_node)
    graph.add_node("execute_tool", execute_tool_node)
    graph.add_node("finalize", finalize_node)

    graph.set_entry_point("think")
    graph.add_edge("think", "plan")
    graph.add_edge("plan", "select_tool")
    graph.add_edge("select_tool", "execute_tool")
    graph.add_conditional_edges(
        "execute_tool",
        route_after_execute,
        {"select_tool": "select_tool", "finalize": "finalize"},
    )
    graph.add_edge("finalize", END)
    return graph.compile()


def main():
    parser = argparse.ArgumentParser(description="LangGraph：思考->规划->选工具->执行->完成")
    parser.add_argument("question", nargs="?", help="你的需求")
    parser.add_argument(
        "--model",
        default="Qwen/Qwen2.5-Coder-32B-Instruct",
        help="硅基流动模型名",
    )
    parser.add_argument(
        "--show-state",
        action="store_true",
        help="打印中间状态（思考/规划/工具）",
    )
    args = parser.parse_args()

    question = args.question or input("请输入你的需求：").strip()
    if not question:
        raise ValueError("问题不能为空")

    llm = build_llm(args.model)
    graph = build_graph(llm)
    init_state: GraphState = {
        "question": question,
        "thought": "",
        "plan": "",
        "tool_name": "",
        "tool_input": "",
        "tool_output": "",
        "final_answer": "",
        "error": "",
        "retries": 0,
    }
    result = graph.invoke(init_state)

    if args.show_state:
        print("=== 思考 ===")
        print(result["thought"])
        print("\n=== 规划 ===")
        print(result["plan"])
        print("\n=== 工具选择 ===")
        print(result["tool_name"])
        print("\n=== 工具输出 ===")
        print(result["tool_output"])
        print("\n=== 最终结果 ===")

    print(result["final_answer"])


if __name__ == "__main__":
    main()
