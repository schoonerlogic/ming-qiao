# Trace Capture

Instructions for capturing decision traces.

---

## When to Capture

**Always:** New component, architectural decision, production work  
**Usually:** Implementation choices, debugging >30min, handoffs  
**Optional:** Bug fixes, config changes

---

## Minimum Trace

```yaml
reflection:
  decisions_made:
    - choice: "What you decided"
      reasoning: "Why"
  confidence:
    level: high
    rationale: "Why this confidence"
```

---

## Detailed Trace

Add for significant work:

```yaml
reflection:
  tensions_discovered:
    - tension: "Constraint found"
      resolution: workaround
      details: "How handled"
  
  unknowns_remaining:
    - unknown: "What's uncertain"
      impact: degraded
      suggested_action: "Next step"
  
  assumptions_made:
    - assumption: "What assumed"
      validated: false
      risk_if_wrong: "Impact"
```

---

## Self-Assessment

1. What were the key decisions and why?
2. What's my confidence level?
3. What constraints did I discover?
4. What's still unknown?
5. What does the next agent need to know?

---

## Output

```
.council/decisions/<domain>/dec-YYYYMMDD-HHMMSS.yaml
```

---

## Agent Responsibilities

| Agent | Responsibility |
|-------|---------------|
| **Aleph** | Architectural decisions |
| **Luban** | Implementation reflections |
| **Thales** | Reviews completeness |
| **Proteus** | Approves, promotes |
