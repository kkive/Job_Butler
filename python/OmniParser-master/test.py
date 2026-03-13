# -*- coding: utf-8 -*-
"""
测试脚本说明：
1. 用于检测当前项目中的 1.png 图片
2. 只执行 OCR 和界面元素检测，不加载 Florence-2
3. 保存标注后的图片到 outputs/1_annotated.png
4. 保存 OCR 文本和检测结果到文本、CSV 文件
5. 注释全部使用中文，方便直接二次修改

使用方式：
    python test_pure_detect.py
"""

import io
import time
import base64
import traceback
from pathlib import Path

import pandas as pd
import torch
from PIL import Image

from util.utils import (
    get_som_labeled_img,
    check_ocr_box,
    get_yolo_model,
)


def main():
    # =========================
    # 1. 基础路径配置
    # =========================
    # 当前脚本所在目录
    base_dir = Path(__file__).resolve().parent

    # 待检测图片
    # 优先读取根目录下的 1.png
    # 如果根目录没有，则回退到 imgs/1.png
    image_path = base_dir / "1.png"
    if not image_path.exists():
        image_path = base_dir / "imgs" / "test.png"

    # YOLO 检测模型路径
    yolo_model_path = base_dir / "weights" / "icon_detect" / "model.pt"

    # 输出目录
    output_dir = base_dir / "outputs"
    output_dir.mkdir(exist_ok=True)

    # 输出文件路径
    annotated_img_path = output_dir / "1_annotated.png"
    csv_path = output_dir / "1_parsed_content.csv"
    txt_path = output_dir / "1_parsed_content.txt"

    # =========================
    # 2. 设备选择
    # =========================
    # 这里只加载 YOLO 检测模型，因此即使没有 CUDA 也可以跑
    device = "cuda" if torch.cuda.is_available() else "cpu"
    print(f"[信息] 当前使用设备: {device}")

    # =========================
    # 3. 参数配置
    # =========================
    # 阈值越低，检测框越多
    # 如果你觉得框太多，可以把它调高到 0.08、0.10、0.15 再试
    box_threshold = 0.05

    # 检查文件是否存在
    if not image_path.exists():
        raise FileNotFoundError(
            f"未找到目标图片，请将图片放到以下任一位置：\n"
            f"1. {base_dir / '1.png'}\n"
            f"2. {base_dir / 'imgs' / '1.png'}"
        )

    if not yolo_model_path.exists():
        raise FileNotFoundError(f"未找到检测模型文件: {yolo_model_path}")

    # =========================
    # 4. 加载图片并计算绘制参数
    # =========================
    image = Image.open(image_path).convert("RGB")
    print(f"[信息] 当前检测图片: {image_path}")
    print(f"[信息] 图片尺寸: {image.size}")

    # 根据图片尺寸动态调整框线和文字显示比例
    box_overlay_ratio = max(image.size) / 3200
    draw_bbox_config = {
        "text_scale": 0.8 * box_overlay_ratio,
        "text_thickness": max(int(2 * box_overlay_ratio), 1),
        "text_padding": max(int(3 * box_overlay_ratio), 1),
        "thickness": max(int(3 * box_overlay_ratio), 1),
    }

    # =========================
    # 5. 加载 YOLO 检测模型
    # =========================
    print("[信息] 正在加载 YOLO 检测模型...")
    som_model = get_yolo_model(str(yolo_model_path))
    som_model.to(device)
    print(f"[信息] 检测模型已加载到: {device}")

    # =========================
    # 6. OCR 检测
    # =========================
    print("[信息] 正在执行 OCR 检测...")
    start_time = time.time()

    ocr_bbox_rslt, is_goal_filtered = check_ocr_box(
        str(image_path),
        display_img=False,
        output_bb_format="xyxy",
        goal_filtering=None,
        easyocr_args={
            "paragraph": False,
            "text_threshold": 0.9,
        },
        use_paddleocr=True,
    )

    text, ocr_bbox = ocr_bbox_rslt
    ocr_end_time = time.time()

    print(f"[信息] OCR 耗时: {ocr_end_time - start_time:.2f} 秒")
    print(f"[信息] OCR 文本数量: {len(text) if text is not None else 0}")

    # =========================
    # 7. 仅执行界面元素检测与标注
    # =========================
    # 这里不加载 Florence-2，不做图标语义描述
    # 只保留：
    # 1. OCR 识别结果
    # 2. 图标/按钮/界面元素检测框
    # 3. 标注后的图片
    print("[信息] 正在执行界面元素检测与标注...")
    dino_labeled_img, label_coordinates, parsed_content_list = get_som_labeled_img(
        str(image_path),
        som_model,
        BOX_TRESHOLD=box_threshold,
        output_coord_in_ratio=True,
        ocr_bbox=ocr_bbox,
        draw_bbox_config=draw_bbox_config,
        caption_model_processor=None,
        ocr_text=text,
        use_local_semantics=False,
        iou_threshold=0.7,
        scale_img=False,
        batch_size=1,
    )

    end_time = time.time()
    print(f"[信息] 元素检测耗时: {end_time - ocr_end_time:.2f} 秒")
    print(f"[信息] 总耗时: {end_time - start_time:.2f} 秒")

    # =========================
    # 8. 保存标注图片
    # =========================
    print("[信息] 正在保存标注后的图片...")
    labeled_image = Image.open(io.BytesIO(base64.b64decode(dino_labeled_img)))
    labeled_image.save(annotated_img_path)
    print(f"[成功] 标注图片已保存: {annotated_img_path}")

    # =========================
    # 9. 保存结构化结果
    # =========================
    print("[信息] 正在保存解析结果...")

    df = pd.DataFrame(parsed_content_list if parsed_content_list else [])
    if not df.empty:
        df["ID"] = range(len(df))
        cols = ["ID"] + [c for c in df.columns if c != "ID"]
        df = df[cols]

    df.to_csv(csv_path, index=False, encoding="utf-8-sig")

    with open(txt_path, "w", encoding="utf-8") as f:
        f.write("===== 基本信息 =====\n")
        f.write(f"检测图片: {image_path}\n")
        f.write(f"图片尺寸: {image.size}\n")
        f.write(f"设备: {device}\n")
        f.write(f"OCR 文本数量: {len(text) if text else 0}\n")
        f.write(f"界面元素数量: {len(parsed_content_list) if parsed_content_list else 0}\n")

        f.write("\n===== OCR 文本结果 =====\n")
        if text:
            for i, item in enumerate(text):
                f.write(f"{i}: {item}\n")
        else:
            f.write("未识别到 OCR 文本\n")

        f.write("\n===== 界面元素结果 =====\n")
        if parsed_content_list:
            for i, item in enumerate(parsed_content_list):
                f.write(f"{i}: {item}\n")
        else:
            f.write("未解析到界面元素\n")

        f.write("\n===== 检测框坐标 =====\n")
        if label_coordinates:
            for k, v in label_coordinates.items():
                f.write(f"{k}: {v}\n")
        else:
            f.write("未生成检测框坐标\n")

    print(f"[成功] CSV 结果已保存: {csv_path}")
    print(f"[成功] 文本结果已保存: {txt_path}")

    # =========================
    # 10. 控制台打印简要结果
    # =========================
    print("\n===== OCR 文本结果 =====")
    if text:
        for i, item in enumerate(text[:20]):
            print(f"{i}: {item}")
        if len(text) > 20:
            print(f"... 共 {len(text)} 条，仅展示前 20 条")
    else:
        print("未识别到 OCR 文本")

    print("\n===== 界面元素结果 =====")
    if parsed_content_list:
        for i, item in enumerate(parsed_content_list[:20]):
            print(f"{i}: {item}")
        if len(parsed_content_list) > 20:
            print(f"... 共 {len(parsed_content_list)} 条，仅展示前 20 条")
    else:
        print("未解析到界面元素")

    print("\n===== 输出文件 =====")
    print(f"标注图片: {annotated_img_path}")
    print(f"CSV结果 : {csv_path}")
    print(f"文本结果: {txt_path}")


if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        print("[错误] 脚本执行失败")
        print(f"[错误详情] {e}")
        print(traceback.format_exc())
