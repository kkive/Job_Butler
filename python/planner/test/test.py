import operator
import os
from typing import Literal

from langchain_core.messages import HumanMessage, SystemMessage, ToolMessage, AnyMessage
from langchain_core.tools import tool
from langchain_openai import ChatOpenAI
from langgraph.graph import END, START, StateGraph
import pyautogui
from typing_extensions import Annotated, TypedDict
from OmniParser.main import main as OmniParserMain
from openai import OpenAI

# 固定硅基流动接口与模型，仅从环境变量读取密钥。
SILICONFLOW_BASE_URL = "https://api.siliconflow.cn/v1"
SILICONFLOW_MODEL = "Qwen/Qwen3.5-35B-A3B"


def build_model() -> ChatOpenAI:
    api_key = os.getenv("SILICONFLOW_API_KEY") or os.getenv("OPENAI_API_KEY")
    if not api_key:
        raise ValueError("未检测到 API Key，请设置 SILICONFLOW_API_KEY 或 OPENAI_API_KEY")

    return ChatOpenAI(
        model=SILICONFLOW_MODEL,
        base_url=SILICONFLOW_BASE_URL,
        api_key=api_key,
        temperature=0,
    )



@tool
def MOVE(a: float, b: float) -> str:
    """将 a 移动到 b。"""
    screen_w, screen_h = pyautogui.size()
    x = int(a * screen_w) if 0 <= a <= 1 else int(a)
    y = int(b * screen_h) if 0 <= b <= 1 else int(b)
    pyautogui.moveTo(x, y)
    return f"移动到 {x}  {y}."

@tool
def CLICK(a: str) -> str:
    """点击鼠标左键。"""
    if a == 'right' :
        pyautogui.click(button='right')
    elif a == 'left':   
        pyautogui.click(button='left')
    elif a == 'double_left':
        pyautogui.doubleClick(button='left')
    elif a == 'double_right':
        pyautogui.doubleClick(button='right')
    return f"点击鼠标 {a}."

@tool
def SCREENSHOT() -> str:
    """截图当前屏幕。"""
    screenshot = pyautogui.screenshot()
    path = "image.png"
    screenshot.save(path)
    return f"已截图，保存为 {path}."


@tool
def OMNIPARSER() -> str:
    """分析屏幕内容的工具，返回结构化的信息。"""
    result = OmniParserMain("image.png")
    return result

# @tool
# def LLM_CALL(OMNIPARSER: str,user_message: str) -> str:
#     """让模型决定下一步操作。""" 

#     client = OpenAI(
#         api_key=os.getenv("SILICONFLOW_API_KEY") or os.getenv("OPENAI_API_KEY"),
#         base_url=SILICONFLOW_BASE_URL
#     )
#     response = client.chat.completions.create(
        
#         model=SILICONFLOW_MODEL,
#         messages=[
#             {"role": "system", "content": "你是一个有用的助手"},
#             {"role": "user", "content": "OMNIPARSER工具分析屏幕内容的结果是：" + OMNIPARSER + "用户的问题是：" + user_message + "请根据这个结果，决定下一步操作，输出格式必须是MOVE、CLICK、SCREENSHOT、OMNIPARSER其中一个工具的调用，参数可以是left、right、double_left、double_right或者坐标，坐标可以是绝对坐标也可以是相对坐标，相对坐标是0-1之间的小数，代表屏幕宽高的比例。例如：MOVE(0.5, 0.5)代表移动到屏幕中心，CLICK(left)代表点击鼠标左键。"}
#         ]
#     )
#     print(response.choices[0].message.content)
#     return f"调用模型决策下一步操作。{response.choices[0].message.content}"


@tool
def LLM_CALL(OMNIPARSER: str, user_message: str, thinking_budget: int = 4096) -> str:
    """让模型根据屏幕分析结果和用户目标决定下一步操作。"""

    client = OpenAI(
        api_key=os.getenv("SILICONFLOW_API_KEY") or os.getenv("OPENAI_API_KEY"),
        base_url=SILICONFLOW_BASE_URL
    )

    prompt = (
        f"OMNIPARSER工具分析屏幕内容的结果是：{OMNIPARSER}\n"
        f"用户的问题是：{user_message}\n"
        "请根据这个结果，决定下一步操作。\n"
        "输出格式必须包含是以下之一：\n"
        "MOVE(x, y)\n"
        "CLICK(left)\n"
        "CLICK(right)\n"
        "CLICK(double_left)\n"
        "CLICK(double_right)\n"
        "SCREENSHOT()\n"
        "OMNIPARSER()\n"
        "其中坐标可以是绝对坐标，也可以是相对坐标。\n" \
        "坐标必须计算出按钮框的中间点再返回出来。\n"
        "相对坐标是 0 到 1 之间的小数，表示屏幕宽高比例。\n"
        "例如：我建议使用MOVE(1, 0.5)，将鼠标移动到这里\n" \
        ""
    )

    request_kwargs = {
        "model": SILICONFLOW_MODEL,
        "messages": [
            {"role": "system", "content": "你是一个电脑操作助手，只输出下一步工具调用，不要解释。"},
            {"role": "user", "content": prompt}
        ],
        "temperature": 0,
        # 通过 extra_body 透传供应商扩展字段，避免 SDK 参数校验报错。
        "extra_body": {"thinking_budget": thinking_budget},
    }

    try:
        response = client.chat.completions.create(**request_kwargs)
    except Exception as e:
        # 某些网关可能不支持该扩展字段，降级重试保证流程可继续。
        if "thinking_budget" in str(e):
            request_kwargs.pop("extra_body", None)
            response = client.chat.completions.create(**request_kwargs)
        else:
            raise

    action = response.choices[0].message.content.strip()
    print(action)
    return action




TOOLS = [MOVE, CLICK, SCREENSHOT, OMNIPARSER, LLM_CALL]
TOOLS_BY_NAME = {t.name: t for t in TOOLS}
MODEL = build_model().bind_tools(TOOLS)


class MessagesState(TypedDict):
    messages: Annotated[list[AnyMessage], operator.add]
    llm_calls: int


def llm_call(state: MessagesState) -> dict:
    """让模型决定是否调用工具。"""
    msg = MODEL.invoke(
        [
            SystemMessage(content="你是一个电脑操作高手。遇到任何问题时优先调用工具进行电脑操作。" \
            "1.需要需要控制鼠标移动的时候可以使用工具MOVE，这个工具可以让鼠标移动" \
            "2.需要点击鼠标的时候可以使用工具CLICK，这个工具可以让鼠标点击，参数可以是left、right、double_left、double_right。" \
            "3.多是情况是需要先截图，分析屏幕内容的OMNIPARSER工具后再决定下一步操作，截图的工具是SCREENSHOT" \
            "4.分析屏幕内容的工具是OMNIPARSER，这个工具会将屏幕的内容分析成结构化的信息，方便你做出决策")
        ]
        + state["messages"]
    )
    return {
        "messages": [msg],
        "llm_calls": state.get("llm_calls", 0) + 1,
    }


# def tool_node(state: MessagesState) -> dict:
#     """执行模型请求的工具，并将结果回填到消息流。"""
#     result = []
#     for tool_call in state["messages"][-1].tool_calls:
#         tool_impl = TOOLS_BY_NAME[tool_call["name"]]
#         observation = tool_impl.invoke(tool_call["args"])
#         result.append(ToolMessage(content=str(observation), tool_call_id=tool_call["id"]))
#     return {"messages": result}

def tool_node(state: MessagesState) -> dict:
    result = []

    user_message = state["messages"][0].content

    for tool_call in state["messages"][-1].tool_calls:
        tool_impl = TOOLS_BY_NAME[tool_call["name"]]
        args = dict(tool_call["args"] or {})
        # 仅 LLM_CALL 需要 user_message，避免向其他工具注入无效参数。
        if tool_call["name"] == "LLM_CALL":
            args["user_message"] = user_message

        observation = tool_impl.invoke(args)

        result.append(
            ToolMessage(
                content=str(observation),
                tool_call_id=tool_call["id"]
            )
        )

    return {"messages": result}




def should_continue(state: MessagesState) -> Literal["tool_node", END]:
    """如果模型产生工具调用则继续，否则结束。"""
    last_message = state["messages"][-1]
    if last_message.tool_calls:
        return "tool_node"
    return END


def build_agent():
    workflow = StateGraph(MessagesState)
    workflow.add_node("llm_call", llm_call)
    workflow.add_node("tool_node", tool_node)
    workflow.add_edge(START, "llm_call")
    workflow.add_conditional_edges("llm_call", should_continue, ["tool_node", END])
    workflow.add_edge("tool_node", "llm_call")
    return workflow.compile()


def main() -> None:
    agent = build_agent()
    messages = [HumanMessage(content="找到桌面上的回收站，双击打开")]
    result = agent.invoke({"messages": messages, "llm_calls": 0})
    for m in result["messages"]:
        print(f"[{m.type}] {getattr(m, 'content', '')}")


if __name__ == "__main__":
    main()
