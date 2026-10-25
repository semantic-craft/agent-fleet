# Runtime provenance and reproducible inputs

Read-only evidence collected 2026-10-25. This is a dated installation snapshot,
not a live health check, an upgrade authorization, or a claim that all binaries
can be reproduced byte-for-byte. No account/configuration content is included.

| Component | Observed artifact and source | Proven / still missing |
| --- | --- | --- |
| Agenvo connectors | Four launchd entrypoints reference `cd10fb92e1da731b307b85f8e94e8db8b6b8808a`; [public fork commit](https://github.com/semantic-craft/agenvo/commit/cd10fb92e1da731b307b85f8e94e8db8b6b8808a) | All 5,539 release-manifest files match. This proves configured on-disk release integrity, not the currently executing in-memory Node code or independently authenticated manifest. A stale `current-codex.json` still names an older scoped release; actual plist entrypoints and full current manifest take precedence. Do not use that stale record for rollback. |
| DSH bridge plugin | agent-fleet `ff35cd8ae298bbad26a2c4295c63eb6f1b63c2b4` | All 7 release-manifest files match; use the fixed source plus current compatibility tests, not an unversioned plugin directory. |
| Herdr | `herdr --version`: 0.9.3. [Official release](https://github.com/herdrdev/herdr/releases/tag/v0.9.3), `herdr-macos-aarch64` | Installed SHA-256 exactly equals official asset digest `5173a3e0ae42d5d1ab7ebfa5d5e6329f7c3d23f8e1a3677c7ce3231da2884157`. Tag resolves to `7b116c05bfda646af39d2524c54e70c751f57ee8`. Binary is ad-hoc signed, TeamIdentifier not set; digest equality is not Apple notarization. |
| DSH desktop | 0.2.1-rc.3, identifier `com.deepseek.dsh`, TeamIdentifier `NAN929V4UM`; strict/deep signature verification succeeded | ASAR SHA-256 `848f8437a990ffb88fca6772eb8e0e69af84d066c76087ac973a8d8015b459f4`. Installed updater points to official `https://download.deepseek.com/dsh-desk/feeds/mac-arm64/`. ASAR package metadata has no gitHead; [source tag](https://github.com/deepseek-ai/deepseek-harness/tree/1f81c047fc48fa190a683e469daa699857e9afe8) exists but its GitHub release has no desktop assets. Exact signed-package-to-source linkage remains unproven. |
| Magpie current desktop | 0.1.1384; binary SHA-256 `aa8ee4ef63e704d2504495011e6dca784a5c33ad7d5b3145317068b6c161dd1f`; identifier `com.yetone.magpie`, TeamIdentifier `LQAVR62TK2`; strict/deep signature verification succeeded | Go metadata: revision `9d4f03c476210267bee225fceffdcae89be05aee`, `vcs.modified=true`. Local deployment notes cite `232242f4`, but that commit was not obtainable from the recorded public fork API. Do not label that checkout the exact deployed source. Signature validity does not prove source equivalence. |
| Magpie source candidate | [public upstream `a16e9b1c19aa23c78ebf803d7835fb8fe9774d51`](https://github.com/yetone/magpie/commit/a16e9b1c19aa23c78ebf803d7835fb8fe9774d51) | Contains Antigravity Gemini `parametersJsonSchema` preservation and mismatched legacy schema-keyword cleanup, with tests. This is a separate reproducible source input, not the current installed package or an approved deployment. |

## Read-only checks reusable on another machine

```sh
node scripts/release-evidence.mjs <explicit-release-directory> <expected-40-character-revision>
shasum -a 256 <explicit-downloaded-or-installed-binary>
codesign --verify --deep --strict <explicit-app-path>
codesign -dv --verbose=4 <explicit-app-path>
go version -m <explicit-magpie-binary>
```

`release-evidence` loads only the explicit release.json and its artifact directory,
never overlay, login files or service settings. It rejects a mismatched revision,
changed/missing/unlisted file, symlink or unsafe manifest path. It outputs fixed
codes/counts and revision only. It is a point-in-time check of trusted local
artifacts, not a security boundary against concurrent malicious filesystem writes.
It does not authenticate a self-consistent manifest; obtain expected revision and
release source through the approved repository/release process first.

## Install inputs, without copying credentials

1. **Agenvo:** clone `https://github.com/semantic-craft/agenvo`, fetch and check out
   `cd10fb92e1da731b307b85f8e94e8db8b6b8808a` detached for the known source baseline.
   Inspect package lock, then `npm ci && npm run build` as in runbook. The actual
   service install/release and new pairing are separate approved operations.
   Do not copy old device credentials. Reapprove the new fingerprint through the
   existing administrator and complete client OAuth with the user.
2. **DSH adapter:** use the explicit reviewed agent-fleet SHA with `release dsh
   --sha <SHA>` only after approval. The historical `ff35cd8` is provenance, not a
   direction to downgrade. Confirm runtime compatibility before loading the
   current reviewed plugin and restarting DSH at an idle window.
3. **Herdr:** obtain the architecture-specific official v0.9.3 asset; check the
   release digest before granting executable permission or installing it. The
   arm64 digest above does not apply to Intel. This avoids a floating shell
   installer. Newer versions are candidates until native contract/canary tests
   pass. Do not copy the old running process or assume processes survive reboot.
4. **DSH desktop:** download through DeepSeek's official desktop distribution,
   retain the downloaded version/hash and verify the signature. Do not infer its
   source revision merely from rc.3. Login is user-driven; do not copy credentials.
   Existing source tag is sufficient for source investigation, not a signed app
   replacement. A reproducible signed package needs upstream build provenance.
5. **Magpie:** official packaging is delegated by upstream's release workflow to
   `yetone/magpie-releases`; its public installer source is `https://usemagpie.ai/install.sh`.
   Do not run that floating installer as a migration pin. Select a fixed official
   release/package, verify its published digest/signature and test the actual
   model behavior. Preserve approved provider login via normal user login, not
   backup files (Magpie backups may contain secrets). A custom source build is a
   different artifact and must never overwrite the signed application silently.

## Separate Magpie candidate build recipe

Use an isolated checkout at `a16e9b1c19aa23c78ebf803d7835fb8fe9774d51`, never the
active source checkout. Read its AGENTS/Makefile first. `go.mod` declares Go 1.26.8;
use the available toolchain and locked module sums. With dependencies already
cached, disable downloads with `GOPROXY=off GOSUMDB=off GOTOOLCHAIN=local`, use an
empty HOME/CODEX_HOME/XDG_CONFIG_HOME, then:

```sh
CGO_ENABLED=0 go test -tags nogui ./internal/gateway \
  -run '^(TestBuildCodeAssistForGemini|TestBuildCodeAssistForAntigravity|TestPlainSchema.*)$' -count=1
CGO_ENABLED=0 go build -tags nogui -trimpath -o <temporary-candidate-binary> .
```

This builds a **CLI candidate**, not a notarized desktop application. Do not start
it with the existing user HOME or bind the production port as part of an offline
build. Missing cached dependencies are a build blocker, not permission to run an
installer. Candidate acceptance still needs independently reviewed source,
complete applicable tests, a distinct artifact hash and approved isolated live
canary before any production change. Original model-route evidence is not reused
as candidate runtime acceptance.

## Exact remaining operations

- Obtain upstream provenance tying the DSH signed desktop package to source, or
  keep that linkage explicitly unknown; a source checkout alone cannot supply it.
- For Magpie, choose between a fixed signed official release containing the public
  fix and the separate custom candidate. Confirm packaging, configuration-write
  behavior and rollback/data compatibility before requesting a specific switch.
- Implement and test the DSH session-only API in its owning runtime repository;
  add capability discovery without falling back to old global-saving selectModel.
- On a new host, user installs/logs in, pairs the new device and authorizes OAuth;
  then run read-only native checks, cloud canary, one bounded tool loop, reboot
  recovery and subscription recreation/actual event delivery. None occurred here.

## Candidate validation recorded in this revision

The fixed upstream candidate was built with Go 1.27.1, CGO disabled and `nogui`,
using cached dependencies and empty HOME/CODEX_HOME/XDG_CONFIG_HOME. The targeted
CodeAssist/PlainSchema gateway tests above and CLI build both exited 0. Binary
SHA-256: `29baa6027c55cc032f80a84d8b56509f87b5cd3850d78ae277ba11d402767586`;
embedded revision is `a16e9b1c19aa23c78ebf803d7835fb8fe9774d51`, `vcs.modified=false`.
No candidate process was started. Full Magpie suite, GUI packaging/notarization,
provider authentication and live carrier tests were not run for this candidate.
Different toolchains/platforms can yield different bytes; this hash identifies
this artifact, not a universal reproducible-build guarantee.

Independent source review confirmed that the fixed change uses native JSON Schema
for Gemini while retaining legacy `plainSchema` for Claude on Antigravity. That
legacy path still projects union types and can lose semantics; this is not a
general JSON Schema equivalence claim. The targeted tests do not cover the full
REA schema corpus or every oneOf/allOf constraint. Review candidate initialization
and subscription/configuration migrations before any future launch.

## DSH session-only candidate

The owning source is now pinned at official rc.3
`1f81c047fc48fa190a683e469daa699857e9afe8`. A separate runtime patch and Agenvo
capability-gated adapter patch are retained with replay instructions in
[session-only model selection](dsh-session-only.md). Neither is deployed. The
signed installed package-to-source gap above is unchanged; same tag is not proof
of binary equivalence. Applied event sequence is never represented as durable.
