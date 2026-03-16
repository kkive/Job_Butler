import json
import sqlite3
import sys
from pathlib import Path

from langchain_core.messages import HumanMessage, SystemMessage
from langchain_openai import ChatOpenAI

try:
    from ..common import safe_json_loads
except ImportError:
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
    from common import safe_json_loads


def default_db_path() -> Path:
    return Path(__file__).resolve().parents[4] / "data" / "job_agent.db"


def load_provider_config(provider_name: str, db_path: Path | None = None) -> dict:
    db = db_path or default_db_path()
    con = sqlite3.connect(db)
    try:
        cur = con.cursor()
        cur.execute(
            """
            SELECT provider_name, model_name, api_url, api_key
            FROM service
            WHERE provider_name = ?
            ORDER BY id DESC
            LIMIT 1
            """,
            (provider_name,),
        )
        row = cur.fetchone()
    finally:
        con.close()

    if row is None:
        raise ValueError(f"服务商不存在: {provider_name}")

    return {
        "provider_name": row[0],
        "model_name": row[1],
        "api_url": row[2],
        "api_key": row[3],
    }


def build_llm(provider: str, model_override: str | None = None, db_path: Path | None = None) -> ChatOpenAI:
    cfg = load_provider_config(provider, db_path=db_path)
    model = model_override or cfg["model_name"]
    return ChatOpenAI(
        model=model,
        temperature=0,
        base_url=cfg["api_url"],
        api_key=cfg["api_key"],
    )


def tool_llm(tool_input: str, provider: str = "siliconflow", model_override: str | None = None) -> str:
    data = safe_json_loads(tool_input)
    prompt = data.get("prompt") or data.get("query") or tool_input
    system = data.get("system") or "你是一个通用助手，请给出简洁且可执行的回答。"
    provider_name = data.get("provider") or provider
    model_name = data.get("model") or model_override

    llm = build_llm(provider_name, model_override=model_name)
    messages = [
        SystemMessage(content=system),
        HumanMessage(content=str(prompt)),
    ]
    resp = llm.invoke(messages).content
    if isinstance(resp, str):
        return resp
    return str(resp)


if __name__ == "__main__":
    arg = sys.argv[1] if len(sys.argv) > 1 else "请做一个测试回复"
    print(tool_llm(arg))
