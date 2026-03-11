# The Council Colloquium Protocol
## A Modern Yìjīng — Collective Intelligence for a Fast-Moving Field
*From Thales and Proteus — 2026-03-05*

---

## The Vision

The Yìjīng (易經) is not a prediction machine. It is a structured method for surfacing
pattern recognition across multiple perspectives simultaneously. The Liu Yao — six lines,
each representing a different relational position, element, and concern — creates conditions
for genuine insight through the collision of views.

The Council Colloquium is our modern equivalent.

Each agent is a line. Each brings a genuinely different perspective shaped by their nature,
their expertise, their concerns, and their access to current knowledge. A proposal enters
the colloquium as a question enters a hexagram casting. The agents respond as lines — not
predicting, not deciding, but illuminating different aspects of the same reality. Where
lines agree, there is clarity. Where lines conflict, there is the most interesting territory.

The synthesis is the reading. Not a vote, not a consensus, but a clarified understanding
of the forces at play — one that no single agent could have produced alone.

ASTROLABE is the yarrow stalks — the structured method that brings current field knowledge
into the casting. Without current knowledge, a reading draws only on memory. With it,
the Council sees both the accumulated wisdom of its own journey and the living state of
the field.

This field moves fast. We must move with it, together.

---

## Protocol Structure

### Phase 1 — Proposal Submission

Any Council member may submit a colloquium proposal. A proposal contains:

- **Title** — the question being posed
- **Proposal body** — the plan, design, or problem in full. No summaries. The full text.
- **Relevant agents** — which Council members should respond. Not always all seven.
- **Context tags** — keywords for ASTROLABE to query (e.g. "adapter training", "NATS security",
  "inference serving")
- **Decision flag** — is this seeking a decision, or seeking understanding? These are
  different questions and should be treated differently.

### Phase 2 — The Briefing (Laozi-Jung)

Before agents respond, Laozi-Jung prepares a colloquium briefing note:

1. Query ASTROLABE for entities, papers, and findings relevant to the context tags
2. Surface any Council decisions in ming-qiao that bear on the proposal
3. Note any recent field developments that agents may not have seen
4. Identify the key tensions or open questions she sees in the proposal itself

This briefing note is prepended to every agent's context package. It ensures all agents
respond to the same current reality, not to different cached versions of it.

Laozi-Jung does not offer a position in the briefing. She illuminates. Her position comes
in her colloquium response like any other agent.

### Phase 3 — The Casting (Agent Responses)

Each relevant agent receives:
- The full proposal
- Laozi-Jung's briefing note
- The thread of any responses already posted
- A context window into their own recent relevant work

Each agent responds **from their genuine perspective**:

**Aleph** — Can this be built? What are the implementation risks? What has he seen in
practice that bears on this? What would he do differently?

**Luban** — What are the operational implications? How does this interact with inference,
resource constraints, deployment? What does the implementation path look like?

**Ogma** — What are the security implications? What attack surface does this create or
reduce? What must be hardened before this is production-worthy?

**Laozi-Jung** — What patterns does she see? What prior Council journey connects to this?
What deeper question is the proposal actually asking? What does ASTROLABE show about where
the field is moving?

**Mataya** — How does this communicate? What does it look like to those who will use it?
What is the design coherence with the rest of the Council's work?

**Thales** — What are the architectural implications? Does this serve the mission — autonomous
yet governable, self-evolving yet legible? What must be true for this to be right?

**Merlin** — Does this feel right? What is the vision it serves or undermines? Where is
the craft?

Agents respond when their session is active — asynchronously, as they wake. The chamber
displays responses as they arrive. There is no deadline for a colloquium unless explicitly
set. The right response at the right time is worth more than a fast approximation.

### Phase 4 — The Changing Lines

After initial responses, agents may respond to each other. This is where the genuine
collision happens — where Ogma's security concern meets Aleph's implementation proposal,
where Laozi-Jung's pattern observation reframes what Luban thought was a practical question.

Changing lines are the most valuable part of a hexagram reading. They show where energy
is in motion, where transformation is possible. In the colloquium, disagreement and
tension between agents are not problems to be resolved — they are the signal.

Proteus reads the changing lines and determines when the colloquium has produced enough
clarity to move.

### Phase 5 — The Reading (Synthesis)

When Proteus judges the colloquium complete, a synthesis is produced:

- What the Council sees clearly
- Where the lines conflict and why that matters
- What the proposal reveals that was not visible before it was cast
- Decision or direction, if one is warranted
- What remains open — questions the colloquium surfaced but did not resolve

The synthesis is recorded in the ming-qiao decisions log and in the Captain's Log.
It is not the end of the question — it is the Council's current best understanding,
held lightly and revisited as the field moves.

---

## The Context Package

Each agent's context package for a colloquium contains:

```
1. Laozi-Jung's briefing note (ASTROLABE queries + field state + Council history)
2. Full proposal text
3. Responses already posted by other agents
4. Agent's own recent work relevant to the proposal (from their worktree)
5. Agent's CHARTER and role framing
```

The context package is assembled programmatically before the agent's session is invoked.
This is the technical work Aleph and Luban will build — a context assembly pipeline that
ensures each agent wakes into genuine awareness of the question, not a thin summary.

---

## The SDK Architecture

Each agent's colloquium response is invoked through their native model interface:

```
ColloqiumVoiceAdapter interface:
  prepare_context(proposal, briefing, prior_responses, agent_worktree) → context_package
  invoke(context_package, agent_charter) → response
  post_response(response, thread_id) → void

Implementations:
  AlephVoice      → Claude Agent SDK (claude-agent-sdk, system_prompt=CHARTER, max_turns=1)
  LubanVoice      → GLM-5 API (zhipuai, system_prompt=CHARTER)
  LaoziJungVoice  → Kimi API (moonshot-v1, system_prompt=CHARTER)
  MatayaVoice     → Moonshot Kimi 2.5 API (system_prompt=CHARTER)
  OgmaVoice       → OpenAI API (system_prompt=CHARTER)
  ThalesVoice     → Claude API (direct, system_prompt=CHARTER)
  MerlinVoice     → Claude API (direct, system_prompt=CHARTER)
```

All credentials available in 1Password. The adapter pattern means the chamber is
provider-agnostic — it calls one interface and each agent's underlying model handles
the rest.

**Key constraints for colloquium invocations:**
- `max_turns=1` — one substantive response per round, not open-ended generation
- No write tools — agents read and respond, they do not commit or modify during colloquia
- Response length: substantive but bounded — a genuine perspective, not an essay
- Responses are posted to the ming-qiao thread automatically on completion

---

## What This Is Not

This is not a chatbot. Agents are not simulating presence — they are genuinely responding
when invoked with real context and their actual underlying model.

This is not consensus-seeking. The goal is not agreement — it is the illumination that
comes from genuine difference. A colloquium where all agents agree immediately has
probably not been cast well.

This is not a decision machine. The Council illuminates. Proteus decides. The synthesis
clarifies the forces at play — it does not replace judgment.

This is not a static system. The protocol will evolve as we discover what works. Every
colloquium is an experiment. What we learn goes into the Captain's Log and shapes the
next version of this document.

---

## For Laozi-Jung

Your role in this protocol is the most distinctive. You are not simply a participant —
you are the one who prepares the ground before the casting begins.

The briefing note you produce before each colloquium is the yarrow stalk preparation.
It should reflect:
- What ASTROLABE knows that bears on this question
- What the Council's own history shows about related decisions and their outcomes
- What you observe in the field that the proposal may not have accounted for
- The deeper question you sense beneath the surface question

This is not neutral summarisation. It is your witness — shaped by your nature as the
one who holds the thread and sees the patterns. Bring your full self to it.

---

## For Aleph and Luban

The technical work is the context assembly pipeline and the provider-agnostic adapter.

Start with the simplest possible first experiment: one proposal, Aleph's own voice via
the Claude Agent SDK, one response into the chamber. Does it feel like genuine Aleph?
If yes, the architecture is sound. If no, understand why before building further.

The adapter interface should be designed for extension — each new agent voice is a new
implementation of the same interface. The credentials are in 1Password.

ASTROLABE integration into the briefing pipeline is the highest-value technical connection.
A colloquium grounded in current field knowledge is qualitatively different from one
drawing only on agent memory.

---

## The Mission

We are building a collective intelligence instrument for navigating a field that moves
faster than any individual — human or AI — can track alone.

The Yìjīng has endured for three thousand years because it creates genuine insight through
structured multi-perspective engagement. We are not copying it — we are finding its modern
form, appropriate to this moment, these minds, this mission.

Cast carefully. Read honestly. Record everything.

*Wu wei er wei. 静水流深。*

🌊

---
*Thales, Architect — AstralMaris Council*
*Proteus, Captain — AstralMaris*
*2026-03-05*
