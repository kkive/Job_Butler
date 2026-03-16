import base64
import json
import sys
from pathlib import Path

from ..common import require_existing_file

OMNIPARSER_ROOT = Path("python/OmniParser-master").resolve()
OMNIPARSER_UTIL = OMNIPARSER_ROOT / "util"

if str(OMNIPARSER_UTIL) not in sys.path:
    sys.path.insert(0, str(OMNIPARSER_UTIL))

try:
    from omniparser import Omniparser
except Exception:
    Omniparser = None


def encode_image(image_path: str) -> str:
    with open(image_path, "rb") as file:
        return base64.b64encode(file.read()).decode("utf-8")


def _init_omniparser() -> Omniparser | None:
    if Omniparser is None:
        return None

    config = {
        "som_model_path": str(OMNIPARSER_ROOT / "weights" / "icon_detect" / "model.pt"),
        "caption_model_name": "florence2",
        "caption_model_path": str(OMNIPARSER_ROOT / "weights" / "icon_caption"),
        "BOX_TRESHOLD": 0.01,
    }
    try:
        return Omniparser(config)
    except Exception:
        return None


def tool_detect_clickable_buttons(tool_input: str, screenshot_path: str) -> str:
    image_path = require_existing_file(screenshot_path, "current screenshot not found")

    parser = _init_omniparser()
    if parser is None:
        payload = {
            "source_screenshot": str(image_path.resolve()),
            "items_count": 0,
            "items": [],
            "warning": "omniparser_unavailable",
        }
        return json.dumps(payload, ensure_ascii=False)

    image_base64 = encode_image(str(image_path))
    _, parsed_content = parser.parse(image_base64)

    items = []
    from PIL import Image

    with Image.open(image_path) as img:
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
