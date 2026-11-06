#!/bin/bash
# Herdr external-agent dispatcher for agent-fleet (`<state_root>/agents.d/herdr.json`).
#
# Honest-receipt contract (parsed by ExternalAgentRegistry::dispatch):
#   exit 0 + "FLEET_TIER: completed"  -> prompt accepted AND --expect-marker was read back
#                                        from the agent's terminal output
#   exit 0 + "FLEET_TIER: accepted"   -> prompt accepted; marker absent or not requested
#   exit non-zero                     -> dispatch refused (e.g. no active agent for TARGET);
#                                        the error is surfaced on stderr, never swallowed
#
# There is deliberately NO pane fallback: prompts are never pasted into arbitrary shell
# panes. If no agent named TARGET is active, fail loudly with the exact herdr error.

set -euo pipefail

HERDR_BIN="${HERDR_BIN:-$(command -v herdr || echo "$HOME/.local/bin/herdr")}"

TARGET="agent-fleet"
PROMPT=""
CWD=""
EXPECT_MARKER=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target|-t)
      TARGET="$2"
      shift 2
      ;;
    --prompt|-p)
      PROMPT="$2"
      shift 2
      ;;
    --cwd|-c)
      CWD="$2"
      shift 2
      ;;
    --expect-marker|-m)
      EXPECT_MARKER="$2"
      shift 2
      ;;
    *)
      if [[ -z "$PROMPT" ]]; then
        PROMPT="$1"
      else
        echo "herdr-dispatch: unexpected argument: $1" >&2
        exit 2
      fi
      shift
      ;;
  esac
done

if [[ -z "$PROMPT" ]]; then
  echo "herdr-dispatch: --prompt is required" >&2
  exit 2
fi

if ! command -v "$HERDR_BIN" >/dev/null 2>&1; then
  echo "herdr-dispatch: herdr binary not found at $HERDR_BIN" >&2
  exit 3
fi

# 1. Ensure the herdr server is running (it must already host an active agent; we never
#    start agent processes ourselves — see the no-fallback contract above).
if ! "$HERDR_BIN" status server 2>/dev/null | grep -q "status: running"; then
  "$HERDR_BIN" server >/dev/null 2>&1 &
  for _ in {1..20}; do
    if [[ -S "$HOME/.config/herdr/herdr.sock" ]] && "$HERDR_BIN" status server 2>/dev/null | grep -q "status: running"; then
      break
    fi
    sleep 0.1
  done
fi
if ! "$HERDR_BIN" status server 2>/dev/null | grep -q "status: running"; then
  echo "herdr-dispatch: herdr server not running and could not be started" >&2
  exit 3
fi

# 2. Submit the prompt to an ACTIVE agent only. Any failure (including
#    agent_not_ready: the agent is no longer the pane foreground process) aborts with the
#    real herdr error on stderr — no silent swallow, no paste into a shell pane.
if ! SUBMIT_OUT=$("$HERDR_BIN" agent prompt "$TARGET" "$PROMPT" 2>&1); then
  echo "herdr-dispatch: no active herdr agent '$TARGET' (start one with: herdr agent start $TARGET --kind <kind> --pane <pane-id>): $SUBMIT_OUT" >&2
  exit 3
fi
echo "$SUBMIT_OUT"

TIER="accepted"
if [[ -n "$EXPECT_MARKER" ]]; then
  # 3. Poll the agent's terminal output until the marker appears (bounded). Agents run
  #    multi-step turns, so a single immediate read races the response; `agent wait`
  #    state semantics cannot be relied on for readback timing. completed is ONLY ever
  #    claimed on a real marker readback.
  DEADLINE=$(( $(date +%s) * 1000 + ${HERDR_WAIT_TIMEOUT_MS:-90000} ))
  while [[ "$(date +%s)" -lt $(( DEADLINE / 1000 )) ]]; do
    if OUT=$("$HERDR_BIN" agent read "$TARGET" --lines 80 2>/dev/null); then
      if printf '%s' "$OUT" | grep -qF -- "$EXPECT_MARKER"; then
        TIER="completed"
        break
      fi
    fi
    sleep 3
  done
  if [[ -n "${OUT:-}" ]]; then
    echo "$OUT" | tail -60
  fi
else
  if OUT=$("$HERDR_BIN" agent read "$TARGET" --lines 20 2>/dev/null); then
    echo "$OUT"
  fi
fi

echo "FLEET_TIER: $TIER"
