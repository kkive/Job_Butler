import argparse

from service import run_planner


def main():
    parser = argparse.ArgumentParser(description="基于截图驱动的 LangGraph 电脑操作代理")
    parser.add_argument("goal", nargs="?", help="你的电脑操作目标")
    parser.add_argument(
        "--provider",
        default="siliconflow",
        help="服务商名称（从数据库读取 api_url/api_key/model_name）",
    )
    parser.add_argument(
        "--model",
        default="",
        help="覆盖数据库中的模型名称（默认使用数据库配置）",
    )
    parser.add_argument(
        "--max-steps",
        type=int,
        default=6,
        help="最大执行步数",
    )
    parser.add_argument(
        "--show-state",
        action="store_true",
        help="打印中间状态",
    )
    args = parser.parse_args()

    goal = args.goal or input("请输入你的电脑操作目标：").strip()
    result = run_planner(
        goal=goal,
        provider=args.provider,
        model=args.model if args.model else None,
        max_steps=args.max_steps,
    )

    if args.show_state:
        print("=== 思考 ===")
        print(result["thought"])
        print("\n=== 规划 ===")
        print(result["plan"])
        print("\n=== 最近决策 ===")
        print(result["decision"])
        print("\n=== 最近工具 ===")
        print(result["tool_name"])
        print("\n=== 最近工具输出 ===")
        print(result["tool_output"])
        print("\n=== 最近截图 ===")
        print(result["screenshot_path"])
        print("\n=== 最近按钮候选 ===")
        print(result["latest_buttons"])
        print("\n=== 最终结果 ===")

    print(result["final_answer"])


if __name__ == "__main__":
    main()
