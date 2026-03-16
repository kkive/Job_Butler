from pathlib import Path
import sys

try:
    from ..common import require_pyautogui, safe_json_loads
except ImportError:
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
    from common import require_pyautogui, safe_json_loads


def tool_scroll_wheel(tool_input: str) -> str:
    pyautogui = require_pyautogui()
    data = safe_json_loads(tool_input)
    clicks = int(data.get("clicks", 0))
    if clicks == 0:
        raise ValueError('滚轮工具需要 JSON 输入，例如 {"clicks":-500}')
    pyautogui.scroll(clicks)
    return f"已滚动滚轮: {clicks}"


if __name__ == "__main__":
    arg = sys.argv[1] if len(sys.argv) > 1 else '{"clicks":-500}'
    print(tool_scroll_wheel(arg))
