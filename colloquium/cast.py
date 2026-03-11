#!/usr/bin/env python3
"""
Colloquium Casting — Phase 2.

Full pipeline: ASTROLABE briefing + signed envelopes + voice invocation.

Usage:
    # Single voice dry run
    python cast.py --thread <thread-id> --agent aleph

    # All 7 voices (inauguration mode)
    python cast.py --thread <thread-id> --all

    # Custom proposal, specific agents
    python cast.py --proposal "Should we use X?" --agent aleph --agent thales --agent ogma

    # Live — post to thread
    python cast.py --thread <thread-id> --all --post

    # Authored cast — includes captain voices (thales, merlin)
    python cast.py --thread <thread-id> --all --authored --post

    # Specify model
    python cast.py --thread <thread-id> --all --model opus
"""

import asyncio
import sys

import click

from adapter import ClaudeVoice, VOICE_CONFIGS, CAPTAIN_VOICES, all_voices
from pipeline import cast


@click.command()
@click.option("--thread", "thread_id", help="Ming-qiao thread ID to respond to")
@click.option("--proposal", help="Custom proposal text (instead of reading from thread)")
@click.option("--tags", help="Comma-separated ASTROLABE context tags")
@click.option("--agent", "agents", multiple=True, help="Agent(s) to cast (repeatable). Default: aleph")
@click.option("--all", "all_agents", is_flag=True, help="Cast all 7 Council voices")
@click.option("--post", "do_post", is_flag=True, help="Post response to ming-qiao thread")
@click.option("--model", default="sonnet", help="Claude model alias (sonnet, opus)")
@click.option("--authored", is_flag=True, help="Proteus-authored cast — includes captain voices (thales, merlin)")
def main(thread_id, proposal, tags, agents, all_agents, do_post, model, authored):
    if all_agents:
        voices = all_voices(model=model, authored=authored)
    elif agents:
        voices = [ClaudeVoice(a, model=model, authored=authored) for a in agents]
    else:
        voices = [ClaudeVoice("aleph", model=model)]

    context_tags = [t.strip() for t in tags.split(",")] if tags else None

    # Report captain voice skips
    agent_names = [v.agent_id() for v in voices]
    if all_agents and not authored:
        skipped = [a for a in CAPTAIN_VOICES if a not in agent_names]
        if skipped:
            click.echo(f"Captain voices skipped (use --authored to include): {', '.join(skipped)}")

    results = asyncio.run(_cast_all(voices, thread_id, proposal, context_tags, do_post))

    for result in results:
        provenance = "authored" if not result.response.autonomous else "autonomous"
        click.echo()
        click.echo(f"ASTROLABE: {'available' if result.astrolabe_briefing.available else 'UNAVAILABLE'} "
                   f"({len(result.astrolabe_briefing.nodes)} nodes, {len(result.astrolabe_briefing.facts)} facts)")
        click.echo(f"Invocation: {result.invocation_envelope.event_id} (signed, verified)")
        click.echo(f"Provenance: {provenance}")
        click.echo(f"Commitment: {'DETECTED — held' if result.commitment_detected else 'none'}")
        click.echo()
        click.echo("=" * 72)
        click.echo(f"{result.response.agent_id.upper()} COLLOQUIUM VOICE — {result.response.model} | {provenance}")
        click.echo("=" * 72)
        click.echo(result.response.content)
        click.echo("=" * 72)

        if result.posted:
            click.echo(f"\nPosted to thread {thread_id}")
        elif do_post and result.commitment_detected:
            click.echo("\n[HELD] Commitment detected — not posted")

    if not do_post:
        click.echo("\nDry run — use --post to send to ming-qiao")

    click.echo(f"\nAudit log: logs/audit-log.jsonl")
    click.echo(f"Voices cast: {len(results)}")


async def _cast_all(voices, thread_id, proposal, context_tags, do_post):
    """Cast all voices sequentially (shared nonce registry, ordered responses)."""
    results = []
    for voice in voices:
        try:
            result = await cast(
                voice=voice,
                thread_id=thread_id,
                proposal_text=proposal,
                context_tags=context_tags,
                post=do_post,
            )
            results.append(result)
        except Exception as e:
            click.echo(f"\n[ERROR] {voice.agent_id()}: {e}", err=True)
    return results


if __name__ == "__main__":
    main()
