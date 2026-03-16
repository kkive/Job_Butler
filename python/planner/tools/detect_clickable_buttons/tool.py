import json
import shutil
import sys
from pathlib import Path

try:
    from ..common import require_existing_file
except ImportError:
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
    from common import require_existing_file

OMNIPARSER_ROOT = Path(__file__).resolve().parents[1] / "OmniParser"


def _run_omniparser() -> dict | None:
    try:
        if str(OMNIPARSER_ROOT) not in sys.path:
            sys.path.insert(0, str(OMNIPARSER_ROOT))
        from main import main as omniparser_main

        result = omniparser_main(include_annotated_image_base64=False)
        if isinstance(result, dict):
            return result
        return None
    except Exception:
        return None


def tool_detect_clickable_buttons(tool_input: str, screenshot_path: str) -> str:
    image_path = require_existing_file(screenshot_path, "current screenshot not found")

    # 统一复用 OmniParser 的固定输入文件路径。
    omniparser_input = OMNIPARSER_ROOT / "image.png"
    omniparser_input.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(image_path, omniparser_input)

    result = _run_omniparser()
    if result is None:
        payload = {
            "source_screenshot": str(image_path.resolve()),
            "items_count": 0,
            "items": [],
            "warning": "omniparser_unavailable",
        }
        return json.dumps(payload, ensure_ascii=False)

    elements = result.get("elements", []) if isinstance(result, dict) else []

    items = []
    from PIL import Image

    with Image.open(image_path) as img:
        width, height = img.size

    for idx, elem in enumerate(elements):
        bbox = elem.get("bbox") if isinstance(elem, dict) else None
        content = elem.get("content") if isinstance(elem, dict) else ""
        interactable = elem.get("interactivity", True) if isinstance(elem, dict) else True
        if not bbox or len(bbox) < 4:
            continue
        x1, y1, x2, y2 = bbox[:4]
        cx = int(((x1 + x2) / 2) * width)
        cy = int(((y1 + y2) / 2) * height)
        items.append(
            {
                "name": content or f"item_{idx}",
                "x": cx,
                "y": cy,
                "reason": "omniparser-detected",
                "interactable": interactable,
            }
        )

    payload = {
        "source_screenshot": str(image_path.resolve()),
        "items_count": len(items),
        "items": items,
    }
    return json.dumps(payload, ensure_ascii=False)


if __name__ == "__main__":
    screenshot = sys.argv[1] if len(sys.argv) > 1 else ""
    print(tool_detect_clickable_buttons("", screenshot))
