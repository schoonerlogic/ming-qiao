# Decision Traces

**Purpose:** Capture agent decision-making for archaeology and training  
**Storage:** `.council/decisions/<domain>/`

---

## The 5-Coordinate System

| Coordinate | Captures |
|------------|----------|
| **Timeline** | When, duration, sequence |
| **Semantic** | Concepts, domain, goals |
| **Event** | Trigger, alternatives, action |
| **Attribution** | Who contributed what |
| **Outcome** | Result, learnings, artifacts |

---

## Schema

```yaml
apiVersion: council.dev/v1
kind: DecisionTrace
metadata:
  id: dec-20260124-144000
  timestamp: "2026-01-24T14:40:00Z"
  domain: development
  status: approved

question: "How should we version artifacts?"
resolution: "Version in path"
rationale: "Balances traceability with simplicity"

alternatives:
  - option: "Content-addressed only"
    verdict: rejected
    reason: "No version traceability"

decided_by: thales
approved_by: proteus

reflection:
  decisions_made:
    - choice: "Path-based versioning"
      reasoning: "Simpler, self-documenting"
  confidence:
    level: high
    rationale: "Clear requirements"
```

---

## When to Trace

| Decision Type | Trace? | Depth |
|---------------|--------|-------|
| Architectural | Yes | Detailed + ADR |
| Library selection | Yes | Standard |
| Implementation | Yes | Standard |
| Bug fix | Optional | Minimal |

**Rule:** If you'd explain it in a PR review, trace it.

---

## Directory Structure

```
.council/decisions/
├── development/
├── research/
├── operations/
├── planning/
└── review/

docs/decisions/    # Human-readable ADRs
```

---

## References

- `.council/schemas/reflection-extension.yaml`
- `docs/decisions/000-template.md`
- `docs/TRACE_CAPTURE.md`
