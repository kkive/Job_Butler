from pathlib import Path
import sys

try:
    from ..common import require_pyautogui, safe_json_loads
except ImportError:
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
    from common import require_pyautogui, safe_json_loads


def tool_click_screen(tool_input: str) -> str:
    pyautogui = require_pyautogui()
    data = safe_json_loads(tool_input)
    x = data.get("x")
    y = data.get("y")
    if x is None or y is None:
        raise ValueError('点击工具需要 JSON 输入，例如 {"x":100,"y":200}')
    pyautogui.click(int(x), int(y))
    return f"已点击坐标: ({int(x)}, {int(y)})"


if __name__ == "__main__":
    arg = sys.argv[1] if len(sys.argv) > 1 else '{"x":100,"y":100}'
    print(tool_click_screen(arg))
