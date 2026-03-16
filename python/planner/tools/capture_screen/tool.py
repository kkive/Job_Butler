import time
from pathlib import Path

from ..common import require_pyautogui


def tool_capture_screen(tool_input: str) -> str:
    pyautogui = require_pyautogui()
    output_dir = Path("captures")
    output_dir.mkdir(exist_ok=True)
    timestamp = time.strftime("%Y%m%d_%H%M%S")
    output_path = output_dir / f"screen_{timestamp}.png"
    image = pyautogui.screenshot()
    image.save(output_path)
    return str(output_path.resolve())
