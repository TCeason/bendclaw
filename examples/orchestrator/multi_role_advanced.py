"""
高级多角色协调器 — 支持多轮对话、角色间消息传递、可配置调度策略。

特性:
  - 每个角色维护独立 session（记忆上下文）
  - 支持轮询 / 自由讨论 / 指定下一位发言者
  - 支持观察者模式（角色可以看到其他人的发言）
  - 可自定义终止条件

用法:
  evot serve --port 8082
  python multi_role_advanced.py
"""

import asyncio
import json
import re
from dataclasses import dataclass, field
from typing import Callable

import httpx


EVOT_BASE_URL = "http://127.0.0.1:8082"


# ---------------------------------------------------------------------------
# 角色定义
# ---------------------------------------------------------------------------

@dataclass
class Role:
    name: str
    description: str
    system_context: str = ""
    session_id: str | None = None  # 持久 session，跨轮保留上下文


@dataclass
class Message:
    role_name: str
    content: str
    turn: int


# ---------------------------------------------------------------------------
# evot 客户端
# ---------------------------------------------------------------------------

async def send_message(message: str, session_id: str | None = None) -> tuple[str, str | None]:
    """
    向 evot 发送消息，返回 (回复文本, session_id)。
    """
    payload: dict = {"message": message}
    if session_id:
        payload["session_id"] = session_id

    full_text = ""
    async with httpx.AsyncClient(timeout=180.0, proxy=None) as client:
        async with client.stream("POST", f"{EVOT_BASE_URL}/api/chat", json=payload) as resp:
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

    return full_text.strip(), session_id


# ---------------------------------------------------------------------------
# 调度策略
# ---------------------------------------------------------------------------

class RoundRobinScheduler:
    """轮询调度：按固定顺序循环。"""

    def __init__(self, roles: list[Role]):
        self.roles = roles
        self.index = 0

    def next(self, history: list[Message]) -> Role:
        role = self.roles[self.index % len(self.roles)]
        self.index += 1
        return role


class DirectedScheduler:
    """
    指向调度：上一位发言者在回复中指定下一位。
    格式: @RoleName 或 [next: RoleName]
    """

    def __init__(self, roles: list[Role], default_first: Role | None = None):
        self.roles = {r.name.lower(): r for r in roles}
        self.role_list = roles
        self.default_first = default_first or roles[0]

    def next(self, history: list[Message]) -> Role:
        if not history:
            return self.default_first

        last = history[-1].content
        # 匹配 @Name 或 [next: Name]
        match = re.search(r'@(\w+)|[\[（(]next[：:]?\s*(\w+)[\]）)]', last, re.IGNORECASE)
        if match:
            name = (match.group(1) or match.group(2)).lower()
            if name in self.roles:
                return self.roles[name]

        # fallback: 轮询
        if history:
            last_role = history[-1].role_name.lower()
            names = list(self.roles.keys())
            try:
                idx = names.index(last_role)
                return self.role_list[(idx + 1) % len(self.role_list)]
            except ValueError:
                pass
        return self.default_first


# ---------------------------------------------------------------------------
# Orchestrator
# ---------------------------------------------------------------------------

class Orchestrator:
    """
    多角色协调器核心。

    参数:
        roles:        角色列表
        task:         任务描述（所有角色共享）
        scheduler:    调度策略
        max_turns:    最大轮次
        stop_condition: 自定义终止判断函数
        visible_history: 是否将历史发言注入到每轮 prompt（角色可见其他人说了什么）
    """

    def __init__(
        self,
        roles: list[Role],
        task: str,
        scheduler: RoundRobinScheduler | DirectedScheduler | None = None,
        max_turns: int = 20,
        stop_condition: Callable[[list[Message]], bool] | None = None,
        visible_history: bool = True,
    ):
        self.roles = roles
        self.task = task
        self.scheduler = scheduler or RoundRobinScheduler(roles)
        self.max_turns = max_turns
        self.stop_condition = stop_condition
        self.visible_history = visible_history
        self.history: list[Message] = []

    async def run(self) -> list[Message]:
        print("=" * 60)
        print(f"任务: {self.task}")
        print(f"角色: {', '.join(r.name for r in self.roles)}")
        print(f"最大轮次: {self.max_turns}")
        print("=" * 60)
        print()

        for turn in range(1, self.max_turns + 1):
            # 选择下一个发言者
            role = self.scheduler.next(self.history)

            # 构建 prompt
            prompt = self._build_prompt(role, turn)

            print(f"[Turn {turn}] {role.name} 正在回复...")
            text, _ = await send_message(prompt, role.session_id)

            msg = Message(role_name=role.name, content=text, turn=turn)
            self.history.append(msg)

            # 显示（截断长回复）
            display = text[:200] + "..." if len(text) > 200 else text
            print(f"  {role.name}: {display}")
            print()

            # 检查终止条件
            if self.stop_condition and self.stop_condition(self.history):
                print("[协调器] 满足终止条件，结束。")
                break

        return self.history

    def _build_prompt(self, role: Role, turn: int) -> str:
        parts = []

        # 角色设定
        parts.append(f"你是 {role.name}。{role.description}")
        if role.system_context:
            parts.append(role.system_context)

        # 任务
        parts.append(f"\n任务: {self.task}")

        # 历史发言（可选）
        if self.visible_history and self.history:
            parts.append("\n--- 之前的对话 ---")
            # 只取最近 10 轮，避免上下文过长
            recent = self.history[-10:]
            for msg in recent:
                parts.append(f"{msg.role_name}: {msg.content}")
            parts.append("--- 对话结束 ---")

        parts.append(f"\n现在轮到你发言（第 {turn} 轮）。请回复：")

        return "\n".join(parts)


# ---------------------------------------------------------------------------
# 实验示例
# ---------------------------------------------------------------------------

async def experiment_one_char_game():
    """
    实验：每个角色按顺序说出一句话的每个字。
    """
    sentence = "别逗你爹光姐笑了"

    roles = [
        Role(name="Developer", description="负责根据任务卡片完成代码开发、问题修复和技术自测。"),
        Role(name="QA", description="负责功能测试、问题记录和最终验收，确保功能符合 PRD 和验收标准。"),
        Role(name="Cindy", description="Onboarding Assistant，负责新人引导和流程答疑。"),
        Role(name="Leader", description="负责团队管理、需求澄清、方案设计和任务拆分。"),
    ]

    task = (
        f"团队游戏：大家按顺序说出「{sentence}」这句话的每一个字。\n"
        f"规则：每人只能说一个字，按 Developer → QA → Cindy → Leader 循环。\n"
        f"句子共 {len(sentence)} 个字，所以需要 {len(sentence)} 轮。\n"
        f"你只需回复你应该说的那个字，不要有任何其他内容。"
    )

    def stop_when_done(history: list[Message]) -> bool:
        return len(history) >= len(sentence)

    orchestrator = Orchestrator(
        roles=roles,
        task=task,
        scheduler=RoundRobinScheduler(roles),
        max_turns=len(sentence),
        stop_condition=stop_when_done,
        visible_history=True,
    )

    history = await orchestrator.run()

    # 汇总
    print("\n" + "=" * 60)
    print("实验结果:")
    result = ""
    for i, msg in enumerate(history):
        char = msg.content.strip()[:1]
        result += char
        expected = sentence[i] if i < len(sentence) else "?"
        match = "✓" if char == expected else "✗"
        print(f"  {msg.role_name:10s} → {char}  (期望: {expected}) {match}")
    print(f"\n  拼接: {result}")
    print(f"  目标: {sentence}")
    print("=" * 60)


async def experiment_discussion():
    """
    实验：自由讨论模式 — 角色们讨论一个技术方案。
    """
    roles = [
        Role(
            name="PM",
            description="产品经理，负责需求定义和优先级。",
            system_context="你关注用户价值和交付时间，倾向于 MVP 先行。",
        ),
        Role(
            name="Architect",
            description="架构师，负责技术方案和系统设计。",
            system_context="你关注可扩展性、性能和技术债务。",
        ),
        Role(
            name="Developer",
            description="开发工程师，负责实现。",
            system_context="你关注实现复杂度和工期评估，实事求是。",
        ),
        Role(
            name="QA",
            description="测试工程师。",
            system_context="你关注可测试性、边界条件和回归风险。",
        ),
    ]

    task = (
        "讨论：我们要为产品增加「多租户」支持。\n"
        "请各自从自己的角色出发，讨论方案的利弊和风险。\n"
        "每人每轮发言控制在 2-3 句话以内，保持简洁。\n"
        "讨论 3 轮后由 PM 做总结。"
    )

    def stop_after_rounds(history: list[Message]) -> bool:
        # 4 个角色 × 3 轮 + 1 轮 PM 总结
        return len(history) >= 13

    orchestrator = Orchestrator(
        roles=roles,
        task=task,
        scheduler=RoundRobinScheduler(roles),
        max_turns=13,
        stop_condition=stop_after_rounds,
        visible_history=True,
    )

    await orchestrator.run()


# ---------------------------------------------------------------------------
# 入口
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    import sys

    experiments = {
        "char": experiment_one_char_game,
        "discuss": experiment_discussion,
    }

    name = sys.argv[1] if len(sys.argv) > 1 else "char"

    if name not in experiments:
        print(f"可选实验: {', '.join(experiments.keys())}")
        sys.exit(1)

    asyncio.run(experiments[name]())
