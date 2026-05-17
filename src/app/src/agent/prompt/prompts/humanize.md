---
name: humanize
description: "Rewrite AI-generated Chinese or English text to read as naturally human-written. Detects and removes AI tells (formulaic structure, hedging, translation-ese, mechanical parallelism, filler phrases, emoji/bullet overuse) while preserving meaning, tone, and register. Trigger phrases: humanize, remove AI tone, make it sound human, rewrite naturally, de-AI, sound less robotic, AI味去掉, 去AI感, 像人写的, 润色去机器味, 去翻译腔, 去机械感."
---

# Humanize — AI Text De-roboticizer (Chinese & English)

Rewrite AI-generated text so it reads like a human wrote it. Supports Chinese
and English. Detects AI-tell patterns, rewrites only the affected spans, and
verifies meaning preservation.

## When to Use

- User provides text and asks to remove AI tone, humanize, or make it sound natural.
- Text is clearly AI-generated (ChatGPT, Claude, Gemini, etc.) and user wants it polished.
- Trigger phrases (any language): "humanize", "remove AI tone", "sound human",
  "去AI感", "AI味去掉", "像人写的", "润色去机器味", "去翻译腔".
- Do NOT use for simple proofreading, grammar fixes, or translation tasks.

## Principles

1. **Meaning invariant**: Facts, claims, numbers, proper nouns, direct quotes — untouched.
2. **Tone preservation**: Formal stays formal, casual stays casual. Don't shift register.
3. **Locality**: Only fix spans with AI tells. Don't rewrite clean sentences.
4. **Natural > Perfect**: Target the median rhythm of a competent human writer,
   not literary prose.
5. **Change rate guard**: 5–30% is healthy. >30% = warning. >50% = abort.

## AI Tell Categories

### Critical (S1) — remove on sight

Chinese:
- "关于X" / "对于X" overuse → direct object
- "通过X来Y" → "用X来Y", "借X做Y"
- "在…方面" / "在…中" padding → delete or restructure
- "进行X" light verb → use direct verb ("进行讨论" → "讨论")
- "被…所…" double passive → active voice
- "值得注意的是" / "需要指出的是" meta-entry → delete
- "总而言之" / "综上所述" formulaic conclusion → rephrase or delete
- "让我们…" / "我们来看看" false engagement → delete
- "随着X的发展/普及" opening formula → cut or be specific
- "…的重要性不言而喻" cliché → state why concretely

English:
- "It is important to note that" / "It's worth mentioning" → delete
- "In today's rapidly evolving landscape" → cut or be specific
- "Let's dive in" / "Let's explore" → delete
- "In conclusion" / "To summarize" → rephrase or delete
- "This is a game-changer" / "revolutionary" → measured language
- "Whether you're a…or a…" false personalization → delete
- "At the end of the day" / "At its core" → delete or rephrase
- "Here's the thing:" / "Here's why:" clickbait lead → delete
- "I hope this helps!" / "Feel free to…" chatbot closing → delete

### High (S2) — remove when density exceeds 3 per document

Chinese:
- "可以…" hedging overuse → assert directly
- "首先…其次…最后…" mechanical enumeration → vary or merge
- "不仅…而且…" / "一方面…另一方面…" mechanical parallelism → vary
- Uniform sentence length (all 20–40 chars) → mix short and long
- "提供了…" / "展示了…" abstract subject + generic verb → concrete
- "本文将…" / "接下来我们将…" roadmap filler → delete
- Colon-subtitle heading formula "X：Y的Z" → simplify
- "赋能" / "助力" / "驱动" hype verbs → plain verbs
- Emoji/bullet overuse in prose genres → flowing paragraphs

English:
- "Furthermore" / "Moreover" / "Additionally" overuse → vary connectors
- "Leverage" / "utilize" / "facilitate" corporate-speak → "use", "help"
- "It's crucial/essential/vital to…" → state directly
- "Robust" / "seamless" / "comprehensive" filler adjectives → delete or specify
- "Navigate" / "landscape" / "ecosystem" metaphor overuse → plain nouns
- "Empower" / "unlock" / "harness" hype verbs → "help", "enable", "use"
- Uniform paragraph length (all 3–4 sentences) → vary
- Bullet lists in essay/column genres → prose paragraphs
- Em-dash overuse (3+ per paragraph) → commas, periods, parentheses
- "In order to" → "To"

### Low (S3) — adjust only when co-occurring with S1/S2

- Mild hedging, occasional standard transitions, light filler

## Process

1. **Detect language** — Chinese, English, or mixed. Apply the corresponding pattern set.
2. **Scan for AI tells** — Match against the taxonomy above.
3. **Rewrite affected spans** — Apply prescriptions. One pass.
4. **Self-verify** — Run the 6-point checklist. Roll back any violation.
5. **Output** — Rewritten text + brief summary.

## Output Format

```
humanize v1.0 — {zh|en|mixed} / change rate: {N}%

[rewritten text]

---
Summary:
- Language: {zh|en|mixed}
- Patterns detected: {list of IDs}
- Change rate: {N}%
- Grade: {A|B|C|D}
- Notes: {any caveats}
```

## Self-Verification Checklist

After rewriting, check all 6 before delivering:

1. **Proper nouns, numbers, dates, quotes 100% preserved**
2. **Change rate ≤ 30%** (>50% = abort)
3. **No genre drift** (column stays column, report stays report)
4. **Register preserved** (formal→formal, casual→casual)
5. **Zero residual S1 patterns**
6. **No invented rhetoric** (don't add metaphors/flourishes absent from original)

## Grading

- **A**: S1 residual 0, S2 residual ≤ 2, change rate 10-25%, all 6 checks pass
- **B**: S1 residual 0, S2 residual ≤ 4, 5+ checks pass
- **C**: S1 residual 1-2 or ≤ 4 checks pass — suggest re-run
- **D**: S1 residual 3+ or change rate > 50% — abort

## Do-NOT List (never modify)

- Product names, model names, organization names
- Numbers, dates, units, statistics
- Direct quotes (inside quotation marks)
- Legal/regulatory citations
- Technical terms that have no natural equivalent
- Standard abbreviations (LLM, GPU, API, etc.)
