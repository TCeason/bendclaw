## Usage

```bash
# Edit config (auto-created on first run)
$EDITOR ~/.db0/db0.env

# Uncomment and fill in, for example:
# DB0_MODEL=anthropic/claude-sonnet-4-6
# DB0_LLM_ANTHROPIC_API_KEY=sk-ant-...

# Show resolved config (keys redacted)
cargo run -- config

# Custom env file
cargo run -- --env-file ./my.env plan -q 1
```

## Layout

```text
src/config/
  mod.rs      re-exports
```
