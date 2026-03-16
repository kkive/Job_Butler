from ..common import require_pyautogui, safe_json_loads


def tool_input_text(tool_input: str) -> str:
    pyautogui = require_pyautogui()
    data = safe_json_loads(tool_input)
    text = data.get("text", "")
    interval = float(data.get("interval", 0.03))
    if not text:
        raise ValueError("输入工具需要 JSON 输入，例如 {\"text\":\"你好\"}")
    pyautogui.write(text, interval=interval)
    return f"已输入文本：{text}"
