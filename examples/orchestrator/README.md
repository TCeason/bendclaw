# 多角色协调器

基于 evot 本机服务，用 Python 编排多个角色完成协作实验。

## 架构

```
┌─────────────────────────────────────────────┐
│  Python 协调器 (multi_role.py)              │
│  - 定义角色 (name, description, prompt)      │
│  - 控制发言顺序                              │
│  - 收集 & 校验结果                           │
└──────────────┬──────────────────┬───────────┘
               │                  │
         HTTP SSE 模式        CLI 子进程模式
               │                  │
               ▼                  ▼
┌──────────────────────┐  ┌──────────────────┐
│  evot serve :8082    │  │  evot -p <prompt> │
│  /api/chat (SSE)     │  │  --output-format  │
└──────────────────────┘  └──────────────────┘
```

## 快速开始

### 方式一：HTTP 模式（推荐）

```bash
# 终端 1：启动 evot 服务
evot serve --port 8082

# 终端 2：运行协调器
cd examples/orchestrator
pip install -r requirements.txt
python multi_role.py
```

### 方式二：CLI 模式（无需单独启 serve）

```bash
cd examples/orchestrator
pip install -r requirements.txt
python multi_role.py cli
```

## 自定义角色

编辑 `multi_role.py` 中的 `ROLES` 列表：

```python
ROLES = [
    Role(name="Developer", description="负责代码开发..."),
    Role(name="QA", description="负责测试验收..."),
    Role(name="PM", description="负责需求管理..."),
    # 添加更多角色...
]
```

## 自定义实验

修改 `SENTENCE` 变量或重写 `run_experiment()` 函数即可定义不同的多角色协作场景。

## 高级用法

`multi_role_advanced.py` 提供了更完整的框架：
- 支持多轮对话（session 持续）
- 角色间消息传递
- 可配置的调度策略（轮询 / 自由讨论）
