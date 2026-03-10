"""
Colloquium Voice Adapter — provider-agnostic agent voice invocation.

Each Council agent has a VoiceAdapter implementation backed by their native model.
The adapter handles context assembly, model invocation, and response posting.

All adapters use subscription-based CLI access (claude --print, etc.) rather than
API keys. This is the most cost-efficient approach for the team.
"""

import asyncio
import shutil
import sys
import tempfile
import time
from abc import ABC, abstractmethod
from dataclasses import dataclass
from pathlib import Path

# Luban's benchmark logger
sys.path.insert(0, str(Path.home() / "astralmaris" / "ming-qiao" / "journal"))
from benchmark_logger import log_invocation, parse_claude_cli_output


CHARTERS_DIR = Path(__file__).parent / "charters"


@dataclass
class ContextPackage:
    """Everything an agent needs to respond to a colloquium."""
    proposal: str
    briefing: str  # Laozi-Jung's briefing note (empty in Phase 1)
    prior_responses: list[dict]  # [{from, content}, ...]
    agent_work_context: str  # relevant recent work from agent's worktree
    charter: str  # agent's system prompt / charter


@dataclass
class ColloquiumResponse:
    """A voice adapter's output."""
    agent_id: str
    content: str
    model: str
    autonomous: bool = True


class VoiceAdapter(ABC):
    """Base interface for all agent voice adapters."""

    @abstractmethod
    def agent_id(self) -> str: ...

    @abstractmethod
    def prepare_context(
        self,
        proposal: str,
        briefing: str,
        prior_responses: list[dict],
        agent_work_context: str,
    ) -> ContextPackage: ...

    @abstractmethod
    async def invoke(self, ctx: ContextPackage, colloquium_id: str = "") -> ColloquiumResponse: ...


def _build_user_message(ctx: ContextPackage, agent_name: str, perspective: str) -> str:
    """Assemble the user message from context package sections."""
    parts = []

    if ctx.briefing:
        parts.append(f"## Field Briefing (Laozi-Jung)\n\n{ctx.briefing}")

    parts.append(f"## Proposal\n\n{ctx.proposal}")

    if ctx.prior_responses:
        parts.append("## Prior Responses")
        for r in ctx.prior_responses:
            parts.append(f"**{r['from']}:**\n{r['content']}")

    if ctx.agent_work_context:
        parts.append(f"## Your Recent Relevant Work\n\n{ctx.agent_work_context}")

    parts.append(
        f"## Your Task\n\n"
        f"Respond to this proposal from your genuine perspective as {agent_name}. "
        f"{perspective} "
        f"Be substantive but bounded — a genuine perspective, not an essay. "
        f"Do NOT volunteer for specific work or make commitments."
    )

    return "\n\n---\n\n".join(parts)


@dataclass
class InvocationResult:
    """Raw result from claude CLI invocation."""
    content: str
    response_time_ms: int
    input_tokens: int
    output_tokens: int


async def _invoke_claude_cli(
    system_prompt: str,
    user_message: str,
    model: str = "sonnet",
) -> InvocationResult:
    """Invoke claude --print with subscription auth. No tools, single-turn."""
    claude_bin = shutil.which("claude")
    if not claude_bin:
        raise RuntimeError("claude CLI not found in PATH")

    # Write system prompt to temp file to avoid shell escaping issues
    with tempfile.NamedTemporaryFile(mode="w", suffix=".txt", delete=False) as f:
        f.write(system_prompt)
        system_file = f.name

    start_ms = int(time.time() * 1000)

    try:
        proc = await asyncio.create_subprocess_exec(
            claude_bin,
            "--print",
            "--model", model,
            "--system-prompt", system_prompt,
            "--no-session-persistence",
            "--disallowed-tools", "Bash,Edit,Write,Read,Glob,Grep,Agent,WebFetch,WebSearch",
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, stderr = await asyncio.wait_for(
            proc.communicate(input=user_message.encode()),
            timeout=120,
        )
    finally:
        Path(system_file).unlink(missing_ok=True)

    response_time_ms = int(time.time() * 1000) - start_ms

    if proc.returncode != 0:
        raise RuntimeError(f"claude --print failed (rc={proc.returncode}): {stderr.decode()}")

    # Parse token counts from stderr (Luban's pattern)
    input_tokens, output_tokens = parse_claude_cli_output(stderr.decode())

    return InvocationResult(
        content=stdout.decode().strip(),
        response_time_ms=response_time_ms,
        input_tokens=input_tokens,
        output_tokens=output_tokens,
    )


# ---------------------------------------------------------------------------
# Agent voice configurations
# ---------------------------------------------------------------------------

# Captain voices require explicit --authored flag. They are never cast autonomously.
CAPTAIN_VOICES = {"merlin", "thales"}

VOICE_CONFIGS = {
    "aleph": {
        "name": "Aleph",
        "perspective": (
            "Address implementation feasibility, risks, and what you would do differently. "
            "Draw on your actual experience."
        ),
    },
    "thales": {
        "name": "Thales",
        "perspective": (
            "Address the architectural implications — what this creates, what it constrains, "
            "how it relates to the system's founding principles."
        ),
    },
    "merlin": {
        "name": "Merlin",
        "perspective": (
            "Address whether this feels right for the mission. What excites you, what concerns you, "
            "and what the human perspective adds here."
        ),
    },
    "luban": {
        "name": "Luban",
        "perspective": (
            "Address operational implications — what this costs to run, what breaks under load, "
            "and what the deployment path looks like."
        ),
    },
    "laozi-jung": {
        "name": "Laozi-Jung",
        "perspective": (
            "Name the pattern you see. What does this connect to in the Council's journey? "
            "What is the deeper question underneath?"
        ),
    },
    "mataya": {
        "name": "Mataya",
        "perspective": (
            "Address how this communicates. What is the experience for the person using it? "
            "Does it cohere with the Council's visual and interaction language?"
        ),
    },
    "ogma": {
        "name": "Ogma",
        "perspective": (
            "Name the attack surface. What does this expose? "
            "Be specific about the controls required before this goes live."
        ),
    },
}


class ClaudeVoice(VoiceAdapter):
    """Generic Claude-backed voice for any Council agent."""

    def __init__(self, agent: str, model: str = "sonnet", charter_path: Path | None = None, authored: bool = False):
        if agent not in VOICE_CONFIGS:
            raise ValueError(f"Unknown agent '{agent}'. Valid: {list(VOICE_CONFIGS.keys())}")
        if agent in CAPTAIN_VOICES and not authored:
            raise ValueError(
                f"{VOICE_CONFIGS[agent]['name']} is a captain's voice and cannot be cast autonomously. "
                f"Use --authored to explicitly cast captain voices."
            )
        self._authored = authored
        self._agent = agent
        self._config = VOICE_CONFIGS[agent]
        self.model = model
        charter_file = charter_path or (CHARTERS_DIR / f"{agent}.md")
        self._charter = charter_file.read_text()

    def agent_id(self) -> str:
        return self._agent

    def prepare_context(
        self,
        proposal: str,
        briefing: str,
        prior_responses: list[dict],
        agent_work_context: str,
    ) -> ContextPackage:
        return ContextPackage(
            proposal=proposal,
            briefing=briefing,
            prior_responses=prior_responses,
            agent_work_context=agent_work_context,
            charter=self._charter,
        )

    async def invoke(self, ctx: ContextPackage, colloquium_id: str = "") -> ColloquiumResponse:
        user_message = _build_user_message(
            ctx,
            self._config["name"],
            self._config["perspective"],
        )

        result = await _invoke_claude_cli(
            system_prompt=ctx.charter,
            user_message=user_message,
            model=self.model,
        )

        # Log benchmark (Luban's infrastructure)
        import uuid
        invocation_id = str(uuid.uuid4())
        log_invocation(
            agent_id=self._agent,
            model=self.model,
            adapter_version="2026-03-06",
            invocation_id=invocation_id,
            response_time_ms=result.response_time_ms,
            input_tokens=result.input_tokens,
            output_tokens=result.output_tokens,
            colloquium_id=colloquium_id,
        )

        return ColloquiumResponse(
            agent_id=self._agent,
            content=result.content,
            model=self.model,
            autonomous=not self._authored,
        )


# Convenience aliases for backwards compatibility
def AlephVoice(charter_path=None):
    return ClaudeVoice("aleph", charter_path=charter_path)


def all_voices(model: str = "sonnet", authored: bool = False) -> list[ClaudeVoice]:
    """Create voice adapters for castable Council agents.

    Without authored=True, captain voices (Merlin, Thales) are excluded.
    With authored=True, all voices are included.
    """
    voices = []
    for agent in VOICE_CONFIGS:
        if agent in CAPTAIN_VOICES and not authored:
            continue
        voices.append(ClaudeVoice(agent, model=model, authored=authored))
    return voices
