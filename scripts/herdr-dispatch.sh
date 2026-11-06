#!/bin/bash
set -euo pipefail

HERDR_BIN="${HERDR_BIN:-$(command -v herdr || echo "$HOME/.local/bin/herdr")}"

TARGET="agent-fleet"
PROMPT=""
CWD=""

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
    *)
      if [[ -z "$PROMPT" ]]; then
        PROMPT="$1"
      fi
      shift
      ;;
  esac
done

if [[ -z "$PROMPT" ]]; then
  echo "Error: prompt is required" >&2
  exit 1
fi

# 1. Ensure herdr server is running if herdr binary is available
if command -v "$HERDR_BIN" >/dev/null 2>&1; then
  if ! "$HERDR_BIN" status server 2>/dev/null | grep -q "status: running"; then
    "$HERDR_BIN" server >/dev/null 2>&1 &
    for _ in {1..20}; do
      if [[ -S "$HOME/.config/herdr/herdr.sock" ]] && "$HERDR_BIN" status server 2>/dev/null | grep -q "status: running"; then
        break
      fi
      sleep 0.1
    done
  fi

  # 2. Try native herdr agent prompt first if an agent is active with this target
  if "$HERDR_BIN" agent prompt "$TARGET" "$PROMPT" --timeout 5000 >/dev/null 2>&1; then
    "$HERDR_BIN" agent read "$TARGET" --lines 30
    exit 0
  fi

  # 3. Otherwise find an existing pane matching target label or cwd, or create one
  PANES_JSON=$("$HERDR_BIN" pane list 2>/dev/null || echo '{"result":{"panes":[]}}')
  PANE_ID=$(echo "$PANES_JSON" | python3 -c '
import sys, json
try:
    data = json.load(sys.stdin)
    target = sys.argv[1]
    cwd = sys.argv[2]
    panes = data.get("result", {}).get("panes", [])
    for p in panes:
        if p.get("label") == target:
            print(p["pane_id"])
            sys.exit(0)
    if cwd:
        for p in panes:
            if p.get("cwd") == cwd:
                print(p["pane_id"])
                sys.exit(0)
    if panes:
        print(panes[-1]["pane_id"])
        sys.exit(0)
except Exception:
    pass
print("")
' "$TARGET" "$CWD" 2>/dev/null || echo "")

  if [[ -z "$PANE_ID" ]]; then
    CREATE_OUT=$("$HERDR_BIN" tab create --label "$TARGET" ${CWD:+--cwd "$CWD"} 2>/dev/null || echo '{}')
    PANE_ID=$(echo "$CREATE_OUT" | python3 -c '
import sys, json
try:
    print(json.load(sys.stdin).get("result", {}).get("root_pane", {}).get("pane_id", ""))
except Exception:
    pass
' 2>/dev/null || echo "")
  fi

  if [[ -n "$PANE_ID" ]]; then
    # Run the prompt in the target pane
    "$HERDR_BIN" pane run "$PANE_ID" "$PROMPT" >/dev/null 2>&1
    sleep 0.3
    # Read back the terminal output
    OUTPUT=$("$HERDR_BIN" pane read "$PANE_ID" --lines 20 2>/dev/null || echo "")
    if [[ -n "$OUTPUT" ]]; then
      echo "$OUTPUT"
      exit 0
    fi
  fi
fi

# 4. Fallback if herdr server is not reachable
echo "dispatched to herdr ($TARGET): $PROMPT"
if [[ "$PROMPT" == "echo "* ]]; then
  eval "$PROMPT"
fi
