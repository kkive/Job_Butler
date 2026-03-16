import json
import re
from pathlib import Path


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


def require_existing_file(path: str, hint: str) -> Path:
    file_path = Path(path)
    if not path or not file_path.exists():
        raise ValueError(hint)
    return file_path
