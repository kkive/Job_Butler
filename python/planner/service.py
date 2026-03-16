"""兼容层：原 service.py 已下沉到 main.py。"""

try:
    from .main import GraphState, PlannerCancelledError, run_planner
except ImportError:
    from main import GraphState, PlannerCancelledError, run_planner

__all__ = ["run_planner", "GraphState", "PlannerCancelledError"]
