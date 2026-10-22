#!/usr/bin/env bash
# 合并门禁（只读）：gate.sh <owner/repo> <pr号> <期望head完整sha>
# 只用 gh api graphql 读取；不合并、不写入、不打印 token。
set -uo pipefail

usage() { echo "用法: $0 <owner/repo> <pr号> <期望head完整40位sha>" >&2; exit 2; }
[ $# -eq 3 ] || usage
REPO=$1; PR=$2; WANT=$3
[[ $REPO =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || usage
[[ $PR =~ ^[0-9]+$ ]] || usage
[[ $WANT =~ ^[0-9a-f]{40}$ ]] || { echo "期望 sha 必须是完整 40 位小写十六进制" >&2; exit 2; }
command -v gh >/dev/null && command -v jq >/dev/null || { echo "需要 gh 与 jq" >&2; exit 2; }
OWNER=${REPO%%/*}; NAME=${REPO#*/}

TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
gql() { gh api graphql "$@" 2>"$TMP/err" || { echo "GraphQL 调用失败: $(head -c 500 "$TMP/err")" >&2; exit 2; }; }

# 1) 基本信息 + 正文
gql -f query='query($o:String!,$n:String!,$p:Int!){repository(owner:$o,name:$n){pullRequest(number:$p){
  number url state isDraft headRefOid mergeable mergeStateStatus body }}}' \
  -F o="$OWNER" -F n="$NAME" -F p="$PR" > "$TMP/pr.json"
jq -e '.data.repository.pullRequest' "$TMP/pr.json" >/dev/null || { echo "找不到 PR $REPO#$PR" >&2; exit 2; }

# 2) reviewThreads 分页取全
: > "$TMP/threads.jsonl"; CUR=""
while :; do
  args=(-f query='query($o:String!,$n:String!,$p:Int!,$c:String){repository(owner:$o,name:$n){pullRequest(number:$p){
    reviewThreads(first:100,after:$c){pageInfo{hasNextPage endCursor} nodes{isResolved isOutdated path line comments(first:1){nodes{author{login} url}}}}}}}' \
    -F o="$OWNER" -F n="$NAME" -F p="$PR")
  [ -n "$CUR" ] && args+=(-f c="$CUR")
  gql "${args[@]}" > "$TMP/t.json"
  jq -c '.data.repository.pullRequest.reviewThreads.nodes[]' "$TMP/t.json" >> "$TMP/threads.jsonl"
  [ "$(jq -r '.data.repository.pullRequest.reviewThreads.pageInfo.hasNextPage' "$TMP/t.json")" = true ] || break
  CUR=$(jq -r '.data.repository.pullRequest.reviewThreads.pageInfo.endCursor' "$TMP/t.json")
done

# 3) head 提交的 statusCheckRollup，分页取全
: > "$TMP/checks.jsonl"; CUR=""; ROLLUP=""
while :; do
  args=(-f query='query($o:String!,$n:String!,$p:Int!,$c:String){repository(owner:$o,name:$n){pullRequest(number:$p){
    commits(last:1){nodes{commit{oid statusCheckRollup{state contexts(first:100,after:$c){pageInfo{hasNextPage endCursor} nodes{
      __typename
      ... on CheckRun{name status conclusion title summary text checkSuite{app{slug}}}
      ... on StatusContext{context state description creator{login}}}}}}}}}}}' \
    -F o="$OWNER" -F n="$NAME" -F p="$PR")
  [ -n "$CUR" ] && args+=(-f c="$CUR")
  gql "${args[@]}" > "$TMP/c.json"
  R='.data.repository.pullRequest.commits.nodes[0].commit.statusCheckRollup'
  ROLLUP=$(jq -r "$R.state // \"无\"" "$TMP/c.json")
  jq -c "($R.contexts.nodes // [])[]" "$TMP/c.json" >> "$TMP/checks.jsonl"
  [ "$(jq -r "$R.contexts.pageInfo.hasNextPage // false" "$TMP/c.json")" = true ] || break
  CUR=$(jq -r "$R.contexts.pageInfo.endCursor" "$TMP/c.json")
done

# 4) chatgpt-codex-connector 的评论/评审（用于识别额度用尽）
gql -f query='query($o:String!,$n:String!,$p:Int!){repository(owner:$o,name:$n){pullRequest(number:$p){
  comments(last:100){nodes{author{login} body}} reviews(last:100){nodes{author{login} body}}}}}' \
  -F o="$OWNER" -F n="$NAME" -F p="$PR" > "$TMP/cc.json"

P='.data.repository.pullRequest'
STATE=$(jq -r "$P.state" "$TMP/pr.json"); HEAD=$(jq -r "$P.headRefOid" "$TMP/pr.json")
MERGEABLE=$(jq -r "$P.mergeable" "$TMP/pr.json"); MSS=$(jq -r "$P.mergeStateStatus" "$TMP/pr.json")
jq -r "$P.body // \"\"" "$TMP/pr.json" | tr -d '\r' > "$TMP/body.txt"

FAIL=0
ok()  { echo "PASS  $*"; }
bad() { echo "FAIL  $*"; FAIL=1; }

echo "门禁: $REPO#$PR  $(jq -r "$P.url" "$TMP/pr.json")"

[ "$HEAD" = "$WANT" ] && ok "headRefOid = $WANT" || bad "headRefOid = $HEAD，期望 $WANT"
[ "$STATE" = OPEN ] && ok "state = OPEN" || bad "state = $STATE，期望 OPEN"
[ "$MERGEABLE" = MERGEABLE ] && ok "mergeable = MERGEABLE" || bad "mergeable = $MERGEABLE，期望 MERGEABLE"
[ "$MSS" = CLEAN ] && ok "mergeStateStatus = CLEAN" || bad "mergeStateStatus = $MSS，期望 CLEAN"

TOTAL=$(wc -l < "$TMP/threads.jsonl" | tr -d ' ')
UNRES=$(jq -s '[.[]|select(.isResolved==false)]|length' "$TMP/threads.jsonl")
if [ "$UNRES" -eq 0 ]; then ok "reviewThreads 未解决 0（共 $TOTAL）"
else
  bad "reviewThreads 未解决 $UNRES（共 $TOTAL）"
  jq -r 'select(.isResolved==false)|"      - \(.path // "?"):\(.line // "?") \(.comments.nodes[0].author.login // "?") \(.comments.nodes[0].url // "")"' "$TMP/threads.jsonl"
fi

LAST=$(grep -v '^[[:space:]]*$' "$TMP/body.txt" | tail -n 1 | sed 's/[[:space:]]*$//')
WANT_LAST='squash 时以本正文作为提交说明。'
[ "$LAST" = "$WANT_LAST" ] && ok "正文最后一个非空行为「$WANT_LAST」" || bad "正文最后一个非空行为「$LAST」，期望「$WANT_LAST」"

HITS=""
for w in 已删除 已停用 已退役 不再使用 不要用 部分取代 不恢复; do
  n=$(grep -c -F -- "$w" "$TMP/body.txt" || true)
  [ "$n" -gt 0 ] && HITS="$HITS $w(${n}行)"
done
[ -z "$HITS" ] && ok "正文无墓碑词" || bad "正文含墓碑词:$HITS"

# statusCheckRollup：只列出，不判不过
QUOTA_RE='usage limit|usage limits|quota|credits|rate limit|额度'
CC_QUOTA=$(jq -r --arg re "$QUOTA_RE" '[(.data.repository.pullRequest.comments.nodes + .data.repository.pullRequest.reviews.nodes)[]
  | select((.author.login // "")|test("chatgpt-codex-connector"))
  | select((.body // "")|test($re;"i"))]|length' "$TMP/cc.json")
NCHK=$(wc -l < "$TMP/checks.jsonl" | tr -d ' ')
echo "INFO  statusCheckRollup = $ROLLUP（$NCHK 项；仅列出，不计入通过与否）"
jq -r --arg re "$QUOTA_RE" --arg ccq "$CC_QUOTA" '
  if .__typename=="CheckRun" then
    (.checkSuite.app.slug // "") as $app
    | (.conclusion // .status) as $c
    | "      - [check] \(.name) (\($app)): \($c)"
      + (if ($c|test("FAILURE|TIMED_OUT|STARTUP_FAILURE|ACTION_REQUIRED|CANCELLED")) then
           (if ($app|test("chatgpt-codex-connector")) and ((((.title//"")+" "+(.summary//"")+" "+(.text//""))|test($re;"i")) or ($ccq|tonumber)>0)
            then "  ← 失败（chatgpt-codex-connector 额度用尽）" else "  ← 失败" end)
         else "" end)
  else
    (.creator.login // "") as $who
    | "      - [status] \(.context) (\($who)): \(.state)"
      + (if (.state|test("FAILURE|ERROR")) then
           (if (($who+" "+.context)|test("chatgpt-codex-connector")) and (((.description//"")|test($re;"i")) or ($ccq|tonumber)>0)
            then "  ← 失败（chatgpt-codex-connector 额度用尽）" else "  ← 失败" end)
         else "" end)
  end' "$TMP/checks.jsonl"
[ "$CC_QUOTA" -gt 0 ] && echo "INFO  chatgpt-codex-connector 有 $CC_QUOTA 条评论/评审提到额度用尽"

if [ "$FAIL" -eq 0 ]; then echo "RESULT PASS"; exit 0; else echo "RESULT FAIL"; exit 1; fi
