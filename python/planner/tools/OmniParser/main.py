# -*- coding: utf-8 -*-
import json
import os
import time
import traceback
from pathlib import Path

import torch

from util.utils import check_ocr_box, get_som_labeled_img, get_yolo_model


def build_paths(base_dir: Path) -> tuple[Path, Path]:
    image_path = base_dir / "image.png"

    yolo_model_path = base_dir / "weights" / "icon_detect" / "model.pt"
    return image_path, yolo_model_path


def main(include_annotated_image_base64: bool = False) -> dict:
    base_dir = Path(__file__).resolve().parent
    image_path, yolo_model_path = build_paths(base_dir)

    if not yolo_model_path.exists():
        raise FileNotFoundError(f"未找到检测模型文件: {yolo_model_path}")

    device = "cuda" if torch.cuda.is_available() else "cpu"
    box_threshold = 0.05

    som_model = get_yolo_model(str(yolo_model_path))
    som_model.to(device)

    start_time = time.time()
    ocr_bbox_rslt, _ = check_ocr_box(
        str(image_path),
        display_img=False,
        output_bb_format="xyxy",
        goal_filtering=None,
        easyocr_args={"paragraph": False, "text_threshold": 0.9},
        use_paddleocr=True,
    )
    text, ocr_bbox = ocr_bbox_rslt
    ocr_end_time = time.time()

    dino_labeled_img, label_coordinates, parsed_content_list = get_som_labeled_img(
        str(image_path),
        som_model,
        BOX_TRESHOLD=box_threshold,
        output_coord_in_ratio=True,
        ocr_bbox=ocr_bbox,
        draw_bbox_config=None,
        caption_model_processor=None,
        ocr_text=text,
        use_local_semantics=False,
        iou_threshold=0.7,
        scale_img=False,
        batch_size=1,
    )
    end_time = time.time()

    result = {
        "image_path": str(image_path),
        "device": device,
        "timing": {
            "ocr_seconds": round(ocr_end_time - start_time, 4),
            "parse_seconds": round(end_time - ocr_end_time, 4),
            "total_seconds": round(end_time - start_time, 4),
        },
        "ocr_text_count": len(text) if text else 0,
        "element_count": len(parsed_content_list) if parsed_content_list else 0,
        "ocr_text": text if text else [],
        "elements": parsed_content_list if parsed_content_list else [],
        "label_coordinates": label_coordinates if label_coordinates else {},
    }
    if include_annotated_image_base64:
        result["annotated_image_base64"] = dino_labeled_img

    return result


if __name__ == "__main__":
    try:
        include_img_b64 = os.getenv("INCLUDE_ANNOTATED_IMAGE_BASE64", "0") == "1"
        print(json.dumps(main(include_annotated_image_base64=include_img_b64), ensure_ascii=False, indent=2))
    except Exception as e:
        print(
            json.dumps(
                {"success": False, "error": str(e), "traceback": traceback.format_exc()},
                ensure_ascii=False,
                indent=2,
            )
        )
