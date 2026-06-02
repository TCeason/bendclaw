"""
多角色协调器 — 基于 evot 本机 HTTP 服务实现多 Agent 协作实验。

用法:
  1. 先启动 evot server:  evot serve --port 8082
  2. 运行本脚本:          python multi_role.py

实验内容 (来自截图):
  给定一句话 "别逗你爹光姐笑了"，多个角色按顺序各说一个字。
"""

import asyncio
import json
import httpx
from dataclasses import dataclass, field
from typing import AsyncIterator


# ---------------------------------------------------------------------------
# 配置
# ---------------------------------------------------------------------------

EVOT_BASE_URL = "http://127.0.0.1:8082"

SENTENCE = "别逗你爹光姐笑了"


@dataclass
class Role:
    name: str
    description: str
    session_id: str = field(default="")


# 定义角色（与截图对应）
ROLES = [
    Role(name="Developer", description="负责根据任务卡片完成代码开发、问题修复和技术自测。"),
    Role(name="QA", description="负责功能测试、问题记录和最终验收，确保功能符合 PRD 和验收标准。"),
    Role(name="Cindy", description="Onboarding Assistant，负责新人引导和流程答疑。"),
    Role(name="Leader", description="负责团队管理、需求澄清、方案设计和任务拆分，推动功能从需求到验收完整落地。"),
]


# ---------------------------------------------------------------------------
# evot HTTP 客户端
# ---------------------------------------------------------------------------

async def chat_sse(message: str, session_id: str | None = None) -> tuple[str, str]:
    """
    向 evot /api/chat 发送消息，通过 SSE 流式接收响应。
    返回 (完整文本, session_id)。
    """
    payload: dict = {"message": message}
    if session_id:
        payload["session_id"] = session_id

    full_text = ""
    result_session_id = session_id or ""

    # 本地服务无需走代理
    transport = httpx.AsyncHTTPTransport(local_address="127.0.0.1")
    async with httpx.AsyncClient(timeout=120.0, transport=transport, base_url=EVOT_BASE_URL) as client:
        async with client.stream("POST", "/api/chat", json=payload) as resp:
            async for line in resp.aiter_lines():
                if not line.startswith("data: "):
                    continue
                raw = line[len("data: "):]
                if not raw or raw == "ping":
                    continue
                try:
                    event = json.loads(raw)
                except json.JSONDecodeError:
                    continue

                etype = event.get("type")
                data = event.get("data", {})

                if etype == "text":
                    full_text += data.get("text", "")
                elif etype == "done":
                    break
                elif etype == "error":
                    raise RuntimeError(f"evot error: {data.get('message')}")

    return full_text.strip(), result_session_id


# ---------------------------------------------------------------------------
# 协调器逻辑
# ---------------------------------------------------------------------------

async def run_experiment():
    """
    模拟多角色按顺序说出一句话的每个字。
    协调器负责分配任务、按顺序调度角色、收集结果。
    """
    print("=" * 60)
    print("多角色协作实验")
    print(f"目标句子: {SENTENCE}")
    print(f"角色数量: {len(ROLES)}")
    print("=" * 60)
    print()

    # 第一步：给每个角色发送角色设定（通过 system prompt 注入到 message 中）
    # 因为 evot HTTP 接口共享同一个 agent，我们通过在 message 中注入角色信息来模拟
    results: list[tuple[str, str]] = []

    for i, char in enumerate(SENTENCE):
        role = ROLES[i % len(ROLES)]

        # 构造带角色约束的 prompt
        prompt = (
            f"你现在扮演 {role.name}（{role.description}）。\n"
            f"当前是一个团队游戏：大家按顺序说出「{SENTENCE}」这句话的每一个字。\n"
            f"现在轮到你了，你是第 {i + 1} 个人，你只需要说出第 {i + 1} 个字：「{char}」。\n"
            f"请只回复这一个字，不要加任何其他内容。"
        )

        print(f"[{role.name}] 正在思考第 {i + 1} 个字...")
        text, _ = await chat_sse(prompt)

        # 提取回复（取第一个中文字符作为结果）
        actual_char = extract_single_char(text, char)
        results.append((role.name, actual_char))
        print(f"[{role.name}] → {actual_char}")

    # 汇总
    print()
    print("=" * 60)
    print("实验结果:")
    print("-" * 60)
    output = ""
    for name, char in results:
        output += char
        print(f"  {name}: {char}")
    print("-" * 60)
    print(f"  拼接结果: {output}")
    print(f"  目标句子: {SENTENCE}")
    print(f"  是否一致: {'✓' if output == SENTENCE else '✗'}")
    print("=" * 60)


def extract_single_char(text: str, expected: str) -> str:
    """从回复中提取单个字符，优先匹配期望字符。"""
    if expected in text:
        return expected
    # fallback: 取第一个非空白中文字符
    for ch in text:
        if '\u4e00' <= ch <= '\u9fff':
            return ch
    return text.strip()[:1] if text.strip() else "?"


# ---------------------------------------------------------------------------
# 基于 CLI 子进程的替代方案（无需提前启动 serve）
# ---------------------------------------------------------------------------

async def chat_cli(message: str, system_prompt: str | None = None) -> str:
    """
    通过 evot CLI 的 -p 模式直接调用，无需启动 HTTP 服务。
    适合无法启动 serve 的场景。
    """
    cmd = ["evot", "-p", message, "--output-format", "text"]
    if system_prompt:
        cmd += ["--append-system-prompt", system_prompt]

    proc = await asyncio.create_subprocess_exec(
        *cmd,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )
    stdout, stderr = await proc.communicate()

    if proc.returncode != 0:
        raise RuntimeError(f"evot CLI failed: {stderr.decode()}")

    return stdout.decode().strip()


async def run_experiment_cli():
    """
    使用 CLI 子进程模式运行同一实验。
    每个角色有独立的 system prompt。
    """
    print("=" * 60)
    print("多角色协作实验 (CLI 模式)")
    print(f"目标句子: {SENTENCE}")
    print("=" * 60)
    print()

    results: list[tuple[str, str]] = []

    for i, char in enumerate(SENTENCE):
        role = ROLES[i % len(ROLES)]
        system_prompt = f"你是 {role.name}。{role.description} 你正在参与一个团队游戏。"

        prompt = (
            f"游戏规则：团队成员按顺序说出「{SENTENCE}」这句话，每人只说一个字。\n"
            f"你是第 {i + 1} 个人，请只回复第 {i + 1} 个字「{char}」，不要说其他任何内容。"
        )

        print(f"[{role.name}] 正在思考第 {i + 1} 个字...")
        text = await chat_cli(prompt, system_prompt)
        actual_char = extract_single_char(text, char)
        results.append((role.name, actual_char))
        print(f"[{role.name}] → {actual_char}")

    print()
    print("结果: " + "".join(c for _, c in results))


# ---------------------------------------------------------------------------
# 入口
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    import sys

    mode = sys.argv[1] if len(sys.argv) > 1 else "http"

    if mode == "cli":
        asyncio.run(run_experiment_cli())
    else:
        asyncio.run(run_experiment())
