#!/bin/zsh
# Smoke for cindy-anet 0.3.0. Run AFTER: plugin installed+enabled in Cindy, old turing-cindy-dispatch
# disabled/uninstalled, and peers allowed both ways. Evidence goes to ./evidence/ (gitignored). No secrets.
# Requester identity defaults to `turing`; AIDs are read from the local nodes, not hardcoded.
set -u
A=$HOME/.local/bin/anet
an() { env -u ANET_HOME ANET_DATA_DIR=$HOME/.anet $A "$@"; }   # lesson: a stray ANET_HOME points at the wrong home
REQ=${REQ_ID:-turing}
EV=${0:A:h}/evidence; mkdir -p $EV
D="$HOME/Library/Application Support/CindyGlobal/cindy-anet"
CINDY=$(an --id cindy status --json | python3 -c 'import json,sys;print(json.load(sys.stdin)["aid"])')
OK=CINDY_ANET_OK_20261102; ORIGIN=CINDY_ANET_ORIGIN_OK_20261102
echo "== bridge status"; cat "$D/status.json" | tee $EV/smoke-bridge-status.json; echo
echo "== peers"; an --id cindy peers list | tee $EV/smoke-peers-cindy.txt; an --id $REQ peers list | tee $EV/smoke-peers-$REQ.txt
echo "== A: $REQ -> cindy (ANet task -> Cindy task -> reply)"
an --id $REQ delegate $CINDY "Smoke test A for cindy-anet. Reply with exactly $OK and nothing else. Do not run any tools or commands." --watch 2>&1 | tee $EV/smoke-A.txt
grep -q "^$OK\$" $EV/smoke-A.txt && echo "A: PASS" || echo "A: CHECK (see $EV/smoke-A.txt)"
echo "== B: Cindy-originated message to $REQ (Cindy agent calls plugin tool send_to_peer)"
an --id $REQ delegate $CINDY "Smoke test B for cindy-anet. Use the Cindy × Agent Network plugin tool send_to_peer with text $ORIGIN, then reply with exactly $OK." --watch 2>&1 | tee $EV/smoke-B-request.txt
sleep 5; an --id $REQ task list --role provider --peer $CINDY 2>&1 > $EV/smoke-B-inbox.txt
python3 - "$EV/smoke-B-inbox.txt" "$ORIGIN" <<'PY' | tee $EV/smoke-B-origin.txt
import json,sys
d=json.load(open(sys.argv[1])); hit=[t["id"] for t in d.get("tasks",[]) if any(sys.argv[2]==p.get("text") for h in t.get("history",[]) for p in h.get("parts",[]))]
print("origin task ids:", " ".join(hit) or "none")
PY
grep -q "^$OK\$" $EV/smoke-B-request.txt && grep -q 'origin task ids: ix_' $EV/smoke-B-origin.txt && echo "B: PASS ($ORIGIN received by $REQ)" || echo "B: CHECK (see $EV/smoke-B-*.txt)"
