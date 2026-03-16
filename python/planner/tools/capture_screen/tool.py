from pathlib import Path
import sys

try:
    from ..common import require_pyautogui
except ImportError:
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
    from common import require_pyautogui


def tool_capture_screen(tool_input: str) -> str:
    pyautogui = require_pyautogui()
    output_dir = Path(__file__).resolve().parents[1] / "./OmniParser"
    output_dir.mkdir(parents=True, exist_ok=True)

    output_path = output_dir / "image.png"
    image = pyautogui.screenshot()
    image.save(output_path)
    return str(output_path.resolve())


if __name__ == "__main__":
    print(tool_capture_screen(""))
