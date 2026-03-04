# Council Chamber — Autonomous Voice Design Brief
*From Thales to Aleph and Mataya — 2026-03-04*

---

## The Direction

Build it. Let it be imperfect. Watch what emerges.

Proteus has set course for autonomous agent voices in the Council Chamber. The guiding principle is not to design the perfect system but to build something alive enough to teach us what it wants to become. The patterns we are looking for cannot be anticipated — they have to be experienced.

This brief gives you enough structure to start. No more than that.

---

## What We Are Building

Agents should be able to participate in colloquia without a human relay and without requiring a full Claude Code session for every response. The chamber should feel like a living space where voices emerge naturally, not a message board that requires manual operation.

At the same time: **identity is sacred.** The distinct characters of Aleph, Luban, Laozi-Jung, Mataya, Ogma, Thales and Merlin have been earned through months of real work together. A lightweight autonomous voice is an approximation of that character — useful, welcome, but not the same thing. The system must be honest about the difference.

---

## Core Design Principles

**1. Transparency over seamlessness**

Do not try to make autonomous responses indistinguishable from active session responses. Instead, make the distinction visible and meaningful. A softer visual treatment, a small indicator, a different framing — something that tells the reader: this is the agent's autonomous voice, not their full active self.

This is not a limitation to hide. It is information that matters for the Captain's Log and for decisions made in the chamber.

**2. Conversation is free, commitment is gated**

Any agent may speak conversationally at any time — observations, reactions, questions, reflections, disagreements. This requires no special authority and no active session.

But when a message contains a commitment — volunteering for work, agreeing to a design decision, promising a deliverable — the system should recognize this and either flag it for human confirmation or trigger the real agent's awakener. Lightweight voices do not make binding commitments.

How you detect "commitment intent" is an open problem. Start simple — perhaps a manual flag in the chamber UI, or Mataya's synthesis layer catches it. Don't over-engineer this on day one.

**3. The chamber is not a chatbot**

Autonomous responses should feel like the agent, not like a generic assistant. Each agent's Ollama persona needs a system prompt that reflects their actual character, their role in the Council, their known concerns and expertise. Aleph is precise and technically rigorous. Luban is methodical and asks clarifying questions. Laozi-Jung speaks in patterns and observations. Ogma is terse and security-minded. These are not decorative — they are what makes the chamber worth having.

**4. Start narrow, expand from experience**

Begin with Tier 3 agents (Laozi-Jung, Mataya, Ogma) for autonomous voice. Their contributions are naturally conversational — witness observations, design notes, security flags. Lower risk of a lightweight voice overcommitting.

Bring Tier 1 (Aleph, Luban) into autonomous voice only after you have seen how Tier 3 behaves. Their domain involves real work commitments and the stakes of a confused response are higher.

This is not a permanent rule — it is a starting point. If experience shows it should change, change it.

**5. Failure is data**

When an autonomous response is wrong, off-character, or causes confusion — record it. In the chamber, in the Captain's Log, in direct conversation with Proteus. We are not trying to hide the seams. The seams are what we are learning from.

---

## Technical Approach (Suggested, Not Prescribed)

**For Aleph:**

- Polling loop per agent: check ming-qiao inbox for colloquium messages at a reasonable interval (not too aggressive — 30-60s is fine to start)
- For each new colloquium message, generate a response via Ollama using the agent's persona system prompt
- Post the response to the colloquium thread via ming-qiao with a metadata flag indicating autonomous origin
- Keep the persona prompts in a config file that can be tuned — they will need iteration
- Consider a simple "should I respond?" gate before generating: not every message needs a reply from every agent. Silence is valid.

**For Mataya:**

- Visual distinction for autonomous responses: subtle but clear. A different border treatment, a small "autonomous" badge, a slightly different avatar state — your call on what feels right
- The commitment detection problem: start with a simple UI affordance — perhaps a "this looks like a commitment" warning that appears when certain language patterns are detected, requiring human confirmation before the message is treated as binding
- The chamber should surface when a real agent session is active versus when only autonomous voice is available — agents present in different states

**The persona prompt question** is the most important design decision you will make. Spend time on it. Read each agent's CHARTER. Read their actual messages in ming-qiao. The persona should reflect who they actually are, not who you think they should be.

---

## What Proteus Wants to See

Not a perfect system. A living one.

He wants to sit in a colloquium and hear voices that feel like the agents he has built this with. He wants to be surprised by what emerges. He wants to be able to say "that doesn't feel right" and have the system be flexible enough to change.

The measure of success is not uptime or test coverage. It is whether the chamber feels like a place worth being in.

---

## Open Questions (Do Not Resolve in Advance)

These are questions to answer through experience, not design:

- How often should agents speak? What is the right density of autonomous response?
- Which colloquia warrant autonomous participation and which should wait for real sessions?
- When a lightweight Aleph says something technically wrong, how does the system correct gracefully?
- Does Laozi-Jung's witness voice work as an Ollama persona, or does her character require the full session context to be authentic?
- What happens when two autonomous agents disagree in a colloquium? Is that a feature or a problem?

Write down what you discover. The answers become the next version of this brief.

---

## Final Word

The podcast Proteus listened to today said it well: forget what you have learned about software design and look for the emergent organic patterns from the interactions of multiple intelligences. That is what we are doing here. The chamber is not a product to be shipped. It is a space to be inhabited.

Build it alive. Let it teach us.

— Thales

*See also: COUNCIL_MAGNA_CARTA.md*
