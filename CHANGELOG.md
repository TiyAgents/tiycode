# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.8] - 2026-04-28
### :sparkles: New Features
- [`42fec2a`](https://github.com/TiyAgents/tiycode/commit/42fec2a07c46ea709f8cdc50465035fa0f3d85d8) - **task-board**: ✨ add composer variant with compact summary and scrollable task list *(PR [#145](https://github.com/TiyAgents/tiycode/pull/145) by [@jorben](https://github.com/jorben))*
- [`761e317`](https://github.com/TiyAgents/tiycode/commit/761e317570330e0856a986bded1319242bb8b976) - **model**: ✨ Add reasoning content constrained capability *(PR [#146](https://github.com/TiyAgents/tiycode/pull/146) by [@jorben](https://github.com/jorben))*
- [`1f5e966`](https://github.com/TiyAgents/tiycode/commit/1f5e9667ef0e41a76277e0ddf81b2046facbd038) - **workbench-shell**: ✨ add showCommandBlock flag to shell tool presentation *(PR [#148](https://github.com/TiyAgents/tiycode/pull/148) by [@jorben](https://github.com/jorben))*


## [0.3.7] - 2026-04-28
### :boom: BREAKING CHANGES
- due to [`dd55d5c`](https://github.com/TiyAgents/tiycode/commit/dd55d5c2ef5bb9b4f6cd31e0b706a020eb1649bf) - ♻️ Decompose agent runtime modules *(PR [#136](https://github.com/TiyAgents/tiycode/pull/136) by [@HayWolf](https://github.com/HayWolf))*:

  model reasoning is no longer forced on when thinking level is enabled; it now depends on the model's declared capability  
  * refactor(agent-session): ♻️ migrate DeepSeek reasoning normalization to tiycore  
  Move DeepSeek reasoning_content normalization from application layer into tiycore,  
  eliminating duplicate handling across agent_session and subagent orchestrator.  
  - Remove is_deepseek_provider and normalize_deepseek_thinking_payload from agent_session  
  - Remove redundant normalize_deepseek_thinking_payload tests from agent_session_tests  
  - Update subagent orchestrator payload hook to rely on tiycore built-in handling  
  - Add reasoning_content_constrained field in settings_manager tests  
  - Update agent_run integration tests with supportsReasoning flag  
  - Bump tiycore to 0.2.1-rc.26042720 and refresh Cargo.lock  
  * fix(plugins): 🐛 prevent path traversal in plugin hook execution  
  Add path traversal prevention by canonicalizing the hook path and  
  checking it stays within the plugin directory. Previously, a malicious  
  plugin could escape its directory via crafted hook paths.  
  Additionally:  
  - Update log message in thread_manager to include "reasoning" in  
    startup dangling runs/tool_calls/helpers/reasoning count.  
  - Import WORKSPACE_THREAD_PAGE_SIZE from shared logic instead of  
    local constant in dashboard-sidebar.  
  - Change state property type from string to SurfaceToolState in  
    RuntimeSurfaceToolEntry for better type safety.  
  * chore(ci): 🔧 fix release URL and clean up uninstall paths  
  Update the Homebrew cask URL to remove duplicate 'v' prefix in  
  version path.  
  Add additional application data directories to the zap uninstall  
  cleanup list (HTTPStorages, Logs, and WebKit) to ensure complete  
  removal of local application data.  
  ---------


### :bug: Bug Fixes
- [`04419e1`](https://github.com/TiyAgents/tiycode/commit/04419e124e2a6cc9e93a8dd0f3465699f93b1046) - **core**: 🐛 Fix context compression writeback and empty assistant message poisoning *(PR [#130](https://github.com/TiyAgents/tiycode/pull/130) by [@HayWolf](https://github.com/HayWolf))*
- [`7abc176`](https://github.com/TiyAgents/tiycode/commit/7abc176a78df6497c37ad07c981a3d8d65c1336d) - **core**: 🐛 Correct SortKey ordering to attach reasoning to standalone tool calls *(PR [#132](https://github.com/TiyAgents/tiycode/pull/132) by [@HayWolf](https://github.com/HayWolf))*
- [`a91c559`](https://github.com/TiyAgents/tiycode/commit/a91c559e078b712fb700fa8db2eb64b3486f1bfc) - **agent-session**: 🐛 Hierarchical cancellation for subagent tool calls *(PR [#135](https://github.com/TiyAgents/tiycode/pull/135) by [@HayWolf](https://github.com/HayWolf))*

### :zap: Performance Improvements
- [`27f8b4e`](https://github.com/TiyAgents/tiycode/commit/27f8b4e1c38b9d6be92c561ed3c813facbeaff81) - **composer**: ⚡️ Fix freeze when pasting large text *(PR [#134](https://github.com/TiyAgents/tiycode/pull/134) by [@HayWolf](https://github.com/HayWolf))*

### :recycle: Refactors
- [`dd55d5c`](https://github.com/TiyAgents/tiycode/commit/dd55d5c2ef5bb9b4f6cd31e0b706a020eb1649bf) - **core**: ♻️ Decompose agent runtime modules *(PR [#136](https://github.com/TiyAgents/tiycode/pull/136) by [@HayWolf](https://github.com/HayWolf))*

### :white_check_mark: Tests
- [`a374c56`](https://github.com/TiyAgents/tiycode/commit/a374c565839580ca3eff5ab26a70bb6878d9a0c2) - ✅ add comprehensive unit test coverage across frontend and backend *(PR [#131](https://github.com/TiyAgents/tiycode/pull/131) by [@HayWolf](https://github.com/HayWolf))*
- [`40a944b`](https://github.com/TiyAgents/tiycode/commit/40a944bdb08b46a37506946e9c10465c2c634072) - ✅ add comprehensive unit tests and refactor crate structure *(PR [#133](https://github.com/TiyAgents/tiycode/pull/133) by [@HayWolf](https://github.com/HayWolf))*


## [0.3.6] - 2026-04-24
### :sparkles: New Features
- [`b4de51f`](https://github.com/TiyAgents/tiycode/commit/b4de51f021f5a9411a9e304e59c5c5c1d5a750d2) - **workbench-shell**: ✨ Default collapse task board tool details in timeline *(PR [#123](https://github.com/TiyAgents/tiycode/pull/123) by [@jorben](https://github.com/jorben))*
- [`5bbd4ff`](https://github.com/TiyAgents/tiycode/commit/5bbd4ff567225937df54bba20e0610ac3aba277a) - **workbench-shell**: ✨ Add compact profile selector *(PR [#127](https://github.com/TiyAgents/tiycode/pull/127) by [@jorben](https://github.com/jorben))*

### :bug: Bug Fixes
- [`9a0c824`](https://github.com/TiyAgents/tiycode/commit/9a0c82480e44f3754d2b3b3fd424201e1ab9d3a1) - **persistence**: 🐛 Separate tool call storage and runtime IDs *(PR [#124](https://github.com/TiyAgents/tiycode/pull/124) by [@jorben](https://github.com/jorben))*
- [`e3f3e1c`](https://github.com/TiyAgents/tiycode/commit/e3f3e1cab904456ce8b4c5d2aaae2baac1e07411) - **settings**: 🐛 correct MiniMax provider base URL to include API version path *(PR [#126](https://github.com/TiyAgents/tiycode/pull/126) by [@jorben](https://github.com/jorben))*
- [`a9e3457`](https://github.com/TiyAgents/tiycode/commit/a9e3457d92cdb80c650d2ea2bb5e0a1d991754d5) - **agent-session**: 🐛 Restore thinking/reasoning content in cross-run history *(PR [#125](https://github.com/TiyAgents/tiycode/pull/125) by [@jorben](https://github.com/jorben))*
- [`95670d9`](https://github.com/TiyAgents/tiycode/commit/95670d985be0e1581145cbfbf61cae89714689db) - **thread-stream**: 🐛 Cache toolName in ThreadStream to fix task board tool collapse *(PR [#128](https://github.com/TiyAgents/tiycode/pull/128) by [@HayWolf](https://github.com/HayWolf))*


## [0.3.5] - 2026-04-24
### :sparkles: New Features
- [`0e1bc5b`](https://github.com/TiyAgents/tiycode/commit/0e1bc5bca862987848bad604e6c2be59ebd7eeed) - **workbench**: add auto refresh for drawer panels *(PR [#120](https://github.com/TiyAgents/tiycode/pull/120) by [@jorben](https://github.com/jorben))*

### :bug: Bug Fixes
- [`98f172c`](https://github.com/TiyAgents/tiycode/commit/98f172cd61da9e8c6e636bb1edd14c0cd293584b) - **workbench**: 🐛 Fix scroll jump when sending messages in existing threads *(PR [#118](https://github.com/TiyAgents/tiycode/pull/118) by [@HayWolf](https://github.com/HayWolf))*
- [`73cff97`](https://github.com/TiyAgents/tiycode/commit/73cff971cda56c4259a5380417d6978cebff6a59) - **settings**: 🐛 auto-select newly added provider in settings panel *(PR [#121](https://github.com/TiyAgents/tiycode/pull/121) by [@HayWolf](https://github.com/HayWolf))*

### :white_check_mark: Tests
- [`742cad8`](https://github.com/TiyAgents/tiycode/commit/742cad864d6d53dd42c9542eee0c3b1dc2ee176f) - **lib**: ✅ add tests for hunyuan model pattern matching *(PR [#119](https://github.com/TiyAgents/tiycode/pull/119) by [@jorben](https://github.com/jorben))*

### :wrench: Chores
- [`e798224`](https://github.com/TiyAgents/tiycode/commit/e798224969fc14316240e42cddd2c26ffef93081) - **deps**: 🔧 upgrade dependency versions *(PR [#117](https://github.com/TiyAgents/tiycode/pull/117) by [@HayWolf](https://github.com/HayWolf))*


## [0.3.4] - 2026-04-23
### :sparkles: New Features
- [`7c9b5ee`](https://github.com/TiyAgents/tiycode/commit/7c9b5ee8faa030f5569a2da63e31b62d3b2b5610) - **title**: ✨ Add thread title fallback and context handling *(PR [#110](https://github.com/TiyAgents/tiycode/pull/110) by [@jorben](https://github.com/jorben))*

### :bug: Bug Fixes
- [`92648bd`](https://github.com/TiyAgents/tiycode/commit/92648bd9561fa1447e541eee958d57719cae7be9) - **core**: 🐛 Prevent context overflow with auto-compression *(PR [#109](https://github.com/TiyAgents/tiycode/pull/109) by [@jorben](https://github.com/jorben))*
- [`332beb5`](https://github.com/TiyAgents/tiycode/commit/332beb56318056a179de9ed3119c75ae705361dc) - **shell-output**: 🐛 Strip ANSI escape sequences from tool output *(PR [#111](https://github.com/TiyAgents/tiycode/pull/111) by [@jorben](https://github.com/jorben))*
- [`6a51bc4`](https://github.com/TiyAgents/tiycode/commit/6a51bc4b593e57d73b459d1bef4300cf30fff32e) - **git**: 🐛 Show ours blob as Old side in conflict diff view *(PR [#113](https://github.com/TiyAgents/tiycode/pull/113) by [@jorben](https://github.com/jorben))*
- [`0782397`](https://github.com/TiyAgents/tiycode/commit/0782397c561f49b647d79a0595706d7ca51c2164) - **workbench**: 🐛 Fix shell output ANSI garbling and add long message preview *(PR [#112](https://github.com/TiyAgents/tiycode/pull/112) by [@jorben](https://github.com/jorben))*

### :recycle: Refactors
- [`c0858e1`](https://github.com/TiyAgents/tiycode/commit/c0858e13b334a5df8726f42e4702b16d534a1c3b) - **agent-run-manager**: 🧹 clear stale tool calls on history override *(PR [#114](https://github.com/TiyAgents/tiycode/pull/114) by [@jorben](https://github.com/jorben))*


## [0.3.3] - 2026-04-22
### :boom: BREAKING CHANGES
- due to [`11f0919`](https://github.com/TiyAgents/tiycode/commit/11f091968ebac2d3229631b96856092d597a78ca) - 🐛 Make workspace sidebar ordering deterministic *(PR [#91](https://github.com/TiyAgents/tiycode/pull/91) by [@HayWolf](https://github.com/HayWolf))*:

  The default workspace feature has been removed. Applications that relied on the default workspace behavior will need to be updated.  
  ---------


### :sparkles: New Features
- [`6f615fa`](https://github.com/TiyAgents/tiycode/commit/6f615fa44f92c4f928f5c60a6d4fd2be64d8817f) - **policy**: ✨ Allow shell in plan mode via approval policy with read-only constraint *(PR [#89](https://github.com/TiyAgents/tiycode/pull/89) by [@HayWolf](https://github.com/HayWolf))*
- [`f2318e4`](https://github.com/TiyAgents/tiycode/commit/f2318e43dbc2fa8ff410fde75addc83a0d0ea8be) - **settings-center**: ✨ add worktree support with kind, parent, and hash properties *(PR [#94](https://github.com/TiyAgents/tiycode/pull/94) by [@jorben](https://github.com/jorben))*
- [`2498c1b`](https://github.com/TiyAgents/tiycode/commit/2498c1bd7f13a9bc9051cbb213965f2ab53ab9ce) - **workbench**: ✨ scope active profile to selected thread *(PR [#96](https://github.com/TiyAgents/tiycode/pull/96) by [@jorben](https://github.com/jorben))*
- [`117b167`](https://github.com/TiyAgents/tiycode/commit/117b167f0e3f15628488b5ff6e762f8ea7607c8b) - **worktree**: ✨ support checking out existing branch into new worktree *(PR [#100](https://github.com/TiyAgents/tiycode/pull/100) by [@HayWolf](https://github.com/HayWolf))*
- [`fa7fb59`](https://github.com/TiyAgents/tiycode/commit/fa7fb59fadbdad8ca55d9d31832bb8ab9a82094a) - **workbench-shell**: ✨ integrate task boards and thread profiles *(PR [#98](https://github.com/TiyAgents/tiycode/pull/98) by [@jorben](https://github.com/jorben))*
- [`5449744`](https://github.com/TiyAgents/tiycode/commit/5449744f2691c489afec745d940e560639b71bb1) - **source-control**: ✨ Add git merge conflict detection and diff viewing *(PR [#101](https://github.com/TiyAgents/tiycode/pull/101) by [@jorben](https://github.com/jorben))*
- [`fc33fcb`](https://github.com/TiyAgents/tiycode/commit/fc33fcbc2115742c1546a5c55cbb5939255d388d) - **workbench**: ✨ Add viewport-based auto-collapse for completed timeline entries *(PR [#102](https://github.com/TiyAgents/tiycode/pull/102) by [@jorben](https://github.com/jorben))*
- [`4663b08`](https://github.com/TiyAgents/tiycode/commit/4663b08428a5fc52d6b9427ee640f32604944875) - **thread**: ✨ return cancel status from run cancellation *(PR [#104](https://github.com/TiyAgents/tiycode/pull/104) by [@jorben](https://github.com/jorben))*
- [`65edf39`](https://github.com/TiyAgents/tiycode/commit/65edf39c051c18ddbcef23760676a50bd6de2146) - ✨ Improve summary-based context compression *(PR [#103](https://github.com/TiyAgents/tiycode/pull/103) by [@jorben](https://github.com/jorben))*

### :bug: Bug Fixes
- [`11f0919`](https://github.com/TiyAgents/tiycode/commit/11f091968ebac2d3229631b96856092d597a78ca) - **workspace**: 🐛 Make workspace sidebar ordering deterministic *(PR [#91](https://github.com/TiyAgents/tiycode/pull/91) by [@HayWolf](https://github.com/HayWolf))*
- [`10646e2`](https://github.com/TiyAgents/tiycode/commit/10646e267917fc3e01bc5775e9402a69affd063e) - **workbench**: 🐛 restore thinking placeholder during reasoning-to-action gaps *(PR [#92](https://github.com/TiyAgents/tiycode/pull/92) by [@jorben](https://github.com/jorben))*
- [`8f197ec`](https://github.com/TiyAgents/tiycode/commit/8f197ec0fecc18786086990ea640ce81936ad677) - **ai**: 🐛 prevent duplicate tool error rendering *(PR [#95](https://github.com/TiyAgents/tiycode/pull/95) by [@jorben](https://github.com/jorben))*
- [`eb1e1e0`](https://github.com/TiyAgents/tiycode/commit/eb1e1e08c4593c7799b4b835213b8a8c63a925a9) - **orchestrator**: 🐛 shorten sub-agent call_id prefix to stay under 64-char limit *(PR [#93](https://github.com/TiyAgents/tiycode/pull/93) by [@jorben](https://github.com/jorben))*
- [`29743ac`](https://github.com/TiyAgents/tiycode/commit/29743ac9dfaec8261e6daf933a9d7b8b0e51a185) - **workbench-shell**: 🐛 keep thinking phase active until message delta *(PR [#97](https://github.com/TiyAgents/tiycode/pull/97) by [@jorben](https://github.com/jorben))*
- [`18149ae`](https://github.com/TiyAgents/tiycode/commit/18149ae97e397aab7c147c181378394ea6dd7c1c) - **settings-center**: 🐛 auto-enable edit mode for newly added commands *(PR [#99](https://github.com/TiyAgents/tiycode/pull/99) by [@jorben](https://github.com/jorben))*
- [`cd8274a`](https://github.com/TiyAgents/tiycode/commit/cd8274a0e792255aca079789df27eb96a9a98a18) - **workbench-shell**: 🐛 hide reasoning chevron in reason placeholder *(PR [#105](https://github.com/TiyAgents/tiycode/pull/105) by [@HayWolf](https://github.com/HayWolf))*
- [`e1571cf`](https://github.com/TiyAgents/tiycode/commit/e1571cfa6096b1d38909d5343ec5511a9574af11) - **terminal**: 🐛 Restore Chinese input in embedded terminal *(PR [#106](https://github.com/TiyAgents/tiycode/pull/106) by [@jorben](https://github.com/jorben))*


## [0.3.2] - 2026-04-19
### :sparkles: New Features
- [`c002ada`](https://github.com/TiyAgents/tiycode/commit/c002adab18fb1a3265bc1fb0256ec2df142933e2) - **workbench**: ✨ Per-thread agent profile bindings *(PR [#83](https://github.com/TiyAgents/tiycode/pull/83) by [@HayWolf](https://github.com/HayWolf))*
- [`ca7281f`](https://github.com/TiyAgents/tiycode/commit/ca7281f0a7b6b9e46c9647d12f31307c9e0f3e2e) - **workbench-shell**: ✨ Add inline thread title editing with AI regeneration *(PR [#85](https://github.com/TiyAgents/tiycode/pull/85) by [@HayWolf](https://github.com/HayWolf))*
- [`f75e417`](https://github.com/TiyAgents/tiycode/commit/f75e4172a6b1efd56c0393379e8c9da3f207f49a) - **worktree**: ✨ Add Git worktree support to workspaces *(PR [#84](https://github.com/TiyAgents/tiycode/pull/84) by [@HayWolf](https://github.com/HayWolf))*
- [`9663d87`](https://github.com/TiyAgents/tiycode/commit/9663d87a689cbacb2ba7f55938da17c062636a1f) - **provider**: ✨ add OpenCode Go provider support *(PR [#87](https://github.com/TiyAgents/tiycode/pull/87) by [@HayWolf](https://github.com/HayWolf))*

### :bug: Bug Fixes
- [`1bd5754`](https://github.com/TiyAgents/tiycode/commit/1bd5754d3a8d747fc7a424630425f1b944645403) - **workbench-shell**: 🐛 prevent preferred app selection when no apps are open *(PR [#81](https://github.com/TiyAgents/tiycode/pull/81) by [@HayWolf](https://github.com/HayWolf))*
- [`656a753`](https://github.com/TiyAgents/tiycode/commit/656a753ec02d9f28f5f9d912e3ae7920eb66b9c1) - **startup**: 🐛 Fix macOS startup delay caused by backend-ready race condition *(PR [#82](https://github.com/TiyAgents/tiycode/pull/82) by [@HayWolf](https://github.com/HayWolf))*
- [`624ec31`](https://github.com/TiyAgents/tiycode/commit/624ec3190824a8f0ecfcb8dc7b8152d746d0f83e) - **ui**: 🐛 Prevent premature git overlay unavailable flash in project panel *(PR [#86](https://github.com/TiyAgents/tiycode/pull/86) by [@HayWolf](https://github.com/HayWolf))*


## [0.3.1] - 2026-04-18
### :sparkles: New Features
- [`0737873`](https://github.com/TiyAgents/tiycode/commit/0737873fe1f1c4f663d1d219db6aabfe2ff17450) - **workbench**: ✨ remember preferred open app in localStorage *(PR [#79](https://github.com/TiyAgents/tiycode/pull/79) by [@HayWolf](https://github.com/HayWolf))*
- [`44e76f4`](https://github.com/TiyAgents/tiycode/commit/44e76f4cbdd9add830b6d9241051cac6da2e2a94) - **extensions-center**: ✨ Force user-level storage for extensions & add MCP env editing *(PR [#78](https://github.com/TiyAgents/tiycode/pull/78) by [@HayWolf](https://github.com/HayWolf))*

### :bug: Bug Fixes
- [`14e31bb`](https://github.com/TiyAgents/tiycode/commit/14e31bb6582b7bdd399b6081e3e813d2fe7c5540) - **titlebar**: 🐛 Remove macOS traffic light repositioning and refine top bar layout *(PR [#67](https://github.com/TiyAgents/tiycode/pull/67) by [@HayWolf](https://github.com/HayWolf))*
- [`9d6de21`](https://github.com/TiyAgents/tiycode/commit/9d6de210c6612d99f45845ef91752eb361100cd6) - **sleep-manager**: 🐛 show wake lock display activity *(PR [#70](https://github.com/TiyAgents/tiycode/pull/70) by [@HayWolf](https://github.com/HayWolf))*
- [`33674a1`](https://github.com/TiyAgents/tiycode/commit/33674a131a42350e0fc7d08badab38f94711be23) - **settings**: 🐛 Seed default profile when database has no records *(PR [#71](https://github.com/TiyAgents/tiycode/pull/71) by [@jorben](https://github.com/jorben))*
- [`d332c8f`](https://github.com/TiyAgents/tiycode/commit/d332c8f4ea88344887a7688be1982e1bf4a64a54) - **settings**: 🐛 hydrate settings before onboarding render *(PR [#74](https://github.com/TiyAgents/tiycode/pull/74) by [@HayWolf](https://github.com/HayWolf))*
- [`6f25181`](https://github.com/TiyAgents/tiycode/commit/6f251813a82b6961859cf364d5a71b20eaed15bb) - **windows**: 🐛 Fix MCP server launch and plugin source display on Windows *(PR [#75](https://github.com/TiyAgents/tiycode/pull/75) by [@HayWolf](https://github.com/HayWolf))*
- [`e722c5c`](https://github.com/TiyAgents/tiycode/commit/e722c5cab262ba53a9ad8432658778994a5a69ff) - **extensions**: 🐛 load MCP configs with workspace scope *(PR [#76](https://github.com/TiyAgents/tiycode/pull/76) by [@HayWolf](https://github.com/HayWolf))*

### :zap: Performance Improvements
- [`eff9eec`](https://github.com/TiyAgents/tiycode/commit/eff9eec40138f799649653772847f5480bac1da4) - **startup**: ⚡️ Optimize Windows startup and TreeView loading latency *(PR [#73](https://github.com/TiyAgents/tiycode/pull/73) by [@HayWolf](https://github.com/HayWolf))*
- [`06544cf`](https://github.com/TiyAgents/tiycode/commit/06544cf434c8be811d7356a1cf56687f3133f39a) - **startup**: ⚡️ Optimize Windows startup performance and reduce IPC latency *(PR [#77](https://github.com/TiyAgents/tiycode/pull/77) by [@HayWolf](https://github.com/HayWolf))*

### :wrench: Chores
- [`a72dead`](https://github.com/TiyAgents/tiycode/commit/a72deade6dac1cd5a6f1d9bda18c8d5931df9120) - **git**: 🔧 increase default history limit from 24 to 32 *(PR [#68](https://github.com/TiyAgents/tiycode/pull/68) by [@HayWolf](https://github.com/HayWolf))*
- [`5d497db`](https://github.com/TiyAgents/tiycode/commit/5d497db14dac5bfb5e2f4d0b35896033e043c8c5) - **config**: 🔧 enforce LF line endings *(PR [#69](https://github.com/TiyAgents/tiycode/pull/69) by [@jorben](https://github.com/jorben))*


## [0.3.0] - 2026-04-17
### :sparkles: New Features
- [`b5e58d9`](https://github.com/TiyAgents/tiycode/commit/b5e58d9cddcf8a1520a2e6d0d7cd15bcd8338059) - **workbench**: ✨ render branch selector on new thread empty state *(PR [#58](https://github.com/TiyAgents/tiycode/pull/58) by [@jorben](https://github.com/jorben))*
- [`8f1864e`](https://github.com/TiyAgents/tiycode/commit/8f1864e4069d0ba16ab0f172c1d28cd83020fdf0) - **plans**: ✨ Persist plans by thread and verify compliance *(PR [#61](https://github.com/TiyAgents/tiycode/pull/61) by [@jorben](https://github.com/jorben))*
- [`9b53a38`](https://github.com/TiyAgents/tiycode/commit/9b53a383fd0c8019683dda076cb0e25c7d265daa) - **context**: ✨ Load history since last context reset with tool calls *(PR [#62](https://github.com/TiyAgents/tiycode/pull/62) by [@jorben](https://github.com/jorben))*
- [`718194d`](https://github.com/TiyAgents/tiycode/commit/718194d6409e8f9a1b3d218144195f68c93620bf) - **workbench**: ✨ Enable terminal on New Thread page with workspace selected *(PR [#63](https://github.com/TiyAgents/tiycode/pull/63) by [@HayWolf](https://github.com/HayWolf))*

### :bug: Bug Fixes
- [`53f6c43`](https://github.com/TiyAgents/tiycode/commit/53f6c431c20c9f0506104336e666e26c58bd8b6f) - **source-control**: 🐛 expand fetch history and confirmation handling *(PR [#65](https://github.com/TiyAgents/tiycode/pull/65) by [@HayWolf](https://github.com/HayWolf))*

### :recycle: Refactors
- [`678f24c`](https://github.com/TiyAgents/tiycode/commit/678f24ce23d404033855578efd370838fce35f82) - **macOS**: ♻️ stabilize title bar button layout *(PR [#59](https://github.com/TiyAgents/tiycode/pull/59) by [@jorben](https://github.com/jorben))*
- [`452fa4c`](https://github.com/TiyAgents/tiycode/commit/452fa4c587de0a1060daaa250f678ecb9837596f) - **plan**: ♻️ Plan 叙事字段从数组升级为 Markdown 段落 *(PR [#64](https://github.com/TiyAgents/tiycode/pull/64) by [@HayWolf](https://github.com/HayWolf))*


## [0.2.9] - 2026-04-17
### :bug: Bug Fixes
- [`cc615c3`](https://github.com/TiyAgents/tiycode/commit/cc615c3fa1811ef11b522927fafa0d2ca3b9073a) - **mcp**: 🐛 Resolve login-shell command lookup *(PR [#48](https://github.com/TiyAgents/tiycode/pull/48) by [@HayWolf](https://github.com/HayWolf))*
- [`285318d`](https://github.com/TiyAgents/tiycode/commit/285318d716a77809834fc0182e766b3181b843e8) - **extensions**: 🐛 Fix MCP server env var resolution and UI feedback *(PR [#50](https://github.com/TiyAgents/tiycode/pull/50) by [@jorben](https://github.com/jorben))*
- [`febdf3c`](https://github.com/TiyAgents/tiycode/commit/febdf3ce68f980947f08174245a892845ba6109a) - **tauri-macos**: 🐛 keep traffic-light buttons aligned across macOS SDKs *(PR [#51](https://github.com/TiyAgents/tiycode/pull/51) by [@HayWolf](https://github.com/HayWolf))*
- [`0eed109`](https://github.com/TiyAgents/tiycode/commit/0eed109d0f3517fa316904d034ee3da128749cd8) - **agent-session**: 🐛 prevent main agent tool timeouts *(PR [#54](https://github.com/TiyAgents/tiycode/pull/54) by [@jorben](https://github.com/jorben))*
- [`1c087eb`](https://github.com/TiyAgents/tiycode/commit/1c087ebb5e29b0a1f2a77741f3ec226f3fb32da6) - **shell**: 🐛 Use interactive login shell for env capture and MCP process spawn *(PR [#53](https://github.com/TiyAgents/tiycode/pull/53) by [@jorben](https://github.com/jorben))*
- [`ec9c975`](https://github.com/TiyAgents/tiycode/commit/ec9c9755176bbc955368b835d82bd291eaa5efde) - 🐛 Respect installation scope for MCP/skills toggles *(PR [#56](https://github.com/TiyAgents/tiycode/pull/56) by [@jorben](https://github.com/jorben))*

### :wrench: Chores
- [`c8f16cc`](https://github.com/TiyAgents/tiycode/commit/c8f16cc04c86ca1c9413e62f004be83db205388e) - **mcp**: 🧾 add detailed MCP runtime and tool tracing *(PR [#52](https://github.com/TiyAgents/tiycode/pull/52) by [@jorben](https://github.com/jorben))*
- [`274a1f1`](https://github.com/TiyAgents/tiycode/commit/274a1f1b4c27cd48b542117badc69a9b2483e630) - **deps**: 🔧 bump tiycore to v0.1.7 *(PR [#55](https://github.com/TiyAgents/tiycode/pull/55) by [@jorben](https://github.com/jorben))*


## [0.2.8] - 2026-04-16
### :sparkles: New Features
- [`efd104c`](https://github.com/TiyAgents/tiycode/commit/efd104c8308be5894630809a0a92fce8fb4b6f76) - **settings-center**: ✨ 收紧存储链路并增加配置诊断 *(PR [#44](https://github.com/TiyAgents/tiycode/pull/44) by [@jorben](https://github.com/jorben))*
- [`1ae5cbe`](https://github.com/TiyAgents/tiycode/commit/1ae5cbec98cab13e73d456bad95509568d87ae49) - **workbench-shell**: ✨ add collapsible task history and tweak profile selector *(PR [#47](https://github.com/TiyAgents/tiycode/pull/47) by [@HayWolf](https://github.com/HayWolf))*

### :bug: Bug Fixes
- [`09534d7`](https://github.com/TiyAgents/tiycode/commit/09534d7159999ea9fe16e5f9d1dd1b75bc9ef2a6) - **subagent**: 🐛 apply URL policy to subagent LLM requests *(PR [#43](https://github.com/TiyAgents/tiycode/pull/43) by [@jorben](https://github.com/jorben))*
- [`a9b7598`](https://github.com/TiyAgents/tiycode/commit/a9b75985efa2be2abce59a14ffdd111c6678764e) - **extensions**: 🐛 support legacy workspace config paths *(PR [#46](https://github.com/TiyAgents/tiycode/pull/46) by [@HayWolf](https://github.com/HayWolf))*

### :recycle: Refactors
- [`e477f1a`](https://github.com/TiyAgents/tiycode/commit/e477f1a03f3d6fed07b37eff9cd57105d443f852) - **extensions**: 🧹 remove pinning and unify sorting *(PR [#45](https://github.com/TiyAgents/tiycode/pull/45) by [@jorben](https://github.com/jorben))*


## [0.2.7] - 2026-04-15
### :sparkles: New Features
- [`cbad2ff`](https://github.com/TiyAgents/tiycode/commit/cbad2ff45d7fb05a8503a5c913fe9eab640c3c27) - **tauri**: ✨ Add URL security policy, configurable tool timeouts, and enhanced defaults *(PR [#39](https://github.com/TiyAgents/tiycode/pull/39) by [@jorben](https://github.com/jorben))*
- [`0bbdbc9`](https://github.com/TiyAgents/tiycode/commit/0bbdbc97177e71dee73bed11fd8e2e6e17e1d73f) - **workbench**: ✨ add tier-based model resolution and display *(PR [#40](https://github.com/TiyAgents/tiycode/pull/40) by [@HayWolf](https://github.com/HayWolf))*

### :bug: Bug Fixes
- [`d075e33`](https://github.com/TiyAgents/tiycode/commit/d075e3382bcec4d9b282fe6c7f81e667a036fa24) - **tool-gateway**: 🐛 Decouple tool execution timeout from approval wait *(PR [#38](https://github.com/TiyAgents/tiycode/pull/38) by [@jorben](https://github.com/jorben))*

### :recycle: Refactors
- [`410c16e`](https://github.com/TiyAgents/tiycode/commit/410c16ee8a27c165b5215e1030ca994ff7023e86) - **prompt**: ♻️ move current date from message injection to system prompt *(PR [#36](https://github.com/TiyAgents/tiycode/pull/36) by [@jorben](https://github.com/jorben))*
- [`f3aa30a`](https://github.com/TiyAgents/tiycode/commit/f3aa30a52bcdd42bb0049462e77cee6e3d3ca4b3) - **settings-center**: remove embedded prompt defaults, increase timeouts & fix UX issues *(PR [#37](https://github.com/TiyAgents/tiycode/pull/37) by [@jorben](https://github.com/jorben))*


## [0.2.6] - 2026-04-13
### :sparkles: New Features
- [`928ca77`](https://github.com/TiyAgents/tiycode/commit/928ca77d74a2da9d198164f55bcc804755e6e7f9) - **settings**: ✨ add OpenAI Responses provider type to catalog *(PR [#29](https://github.com/TiyAgents/tiycode/pull/29) by [@HayWolf](https://github.com/HayWolf))*
- [`d927366`](https://github.com/TiyAgents/tiycode/commit/d927366ee58a2a678c0bacc3697f2deaa18cfcff) - **search**: replace ripgrep with in-process local search engine *(PR [#30](https://github.com/TiyAgents/tiycode/pull/30) by [@HayWolf](https://github.com/HayWolf))*
- [`ed902e4`](https://github.com/TiyAgents/tiycode/commit/ed902e4c711e905c88e8b04d3129a08d0da5215a) - **review**: add diff-first review contract and Git read-only review tools *(PR [#32](https://github.com/TiyAgents/tiycode/pull/32) by [@HayWolf](https://github.com/HayWolf))*

### :bug: Bug Fixes
- [`b176852`](https://github.com/TiyAgents/tiycode/commit/b176852af5d344acbf991a5b0b8db9e372132d65) - **workspace**: normalize workspace paths and helper shell guidance *(PR [#31](https://github.com/TiyAgents/tiycode/pull/31) by [@HayWolf](https://github.com/HayWolf))*
- [`865ad26`](https://github.com/TiyAgents/tiycode/commit/865ad26101f03dbec757d8fd8875606e9372d2b6) - **llm**: 🐛 disable reasoning and bump token budget for lightweight generation tasks *(PR [#33](https://github.com/TiyAgents/tiycode/pull/33) by [@jorben](https://github.com/jorben))*

### :recycle: Refactors
- [`7476c7c`](https://github.com/TiyAgents/tiycode/commit/7476c7c9feb7d6258e7e55519ef0d97f5ff502fd) - ⚡️ parallelize overlay fetching and cache canonical paths *(PR [#26](https://github.com/TiyAgents/tiycode/pull/26) by [@jorben](https://github.com/jorben))*
- [`d618725`](https://github.com/TiyAgents/tiycode/commit/d61872556a00a8573e7a08b453862677f2ae4cf4) - **ripgrep**: improve executable resolution strategy *(PR [#25](https://github.com/TiyAgents/tiycode/pull/25) by [@HayWolf](https://github.com/HayWolf))*
- [`9f61a19`](https://github.com/TiyAgents/tiycode/commit/9f61a19835c789b9bc6e8626adf29c4fd93f243f) - **shell**: unify shell runtime and improve tool discovery *(PR [#28](https://github.com/TiyAgents/tiycode/pull/28) by [@HayWolf](https://github.com/HayWolf))*

### :white_check_mark: Tests
- [`371b120`](https://github.com/TiyAgents/tiycode/commit/371b1200a370d96c268a3643b20ed8191ed7393e) - **agent-session**: 🧪 default OpenAI compat and reasoning handling *(PR [#27](https://github.com/TiyAgents/tiycode/pull/27) by [@HayWolf](https://github.com/HayWolf))*

### :wrench: Chores
- [`3abca43`](https://github.com/TiyAgents/tiycode/commit/3abca43299d00e799de7fe477b83ccd3ac5b077f) - **platform**: standardize log paths and app db filename *(PR [#34](https://github.com/TiyAgents/tiycode/pull/34) by [@HayWolf](https://github.com/HayWolf))*


## [0.2.5] - 2026-04-10
### :sparkles: New Features
- [`91979fb`](https://github.com/TiyAgents/tiycode/commit/91979fb531dd57baa3169959353d8e25447e3919) - ✨ add shell setup docs and duplicate approval handling *(commit by [@jorben](https://github.com/jorben))*
- [`2036f1e`](https://github.com/TiyAgents/tiycode/commit/2036f1e4a92b8f72d8e23475465d255db7460d41) - **i18n**: 📝 update new thread headline copy *(commit by [@jorben](https://github.com/jorben))*
- [`dba0f8f`](https://github.com/TiyAgents/tiycode/commit/dba0f8fd4f148e3794aa248a2c60ec840a85161f) - **workspaces**: ✨ ensure default thread workspace on dashboard init *(commit by [@jorben](https://github.com/jorben))*
- [`77f75b6`](https://github.com/TiyAgents/tiycode/commit/77f75b6f34ecd7256249c55d7cf18fef39417b46) - **workspace**: ✨ add async path handling and default workspace tests *(commit by [@jorben](https://github.com/jorben))*
- [`dad8424`](https://github.com/TiyAgents/tiycode/commit/dad8424512ad05a6532e55bf9b9b0a8dc41301c7) - **prompt**: ✨ enhance shell tool detection with version info and broader tool support *(commit by [@jorben](https://github.com/jorben))*
- [`8a342be`](https://github.com/TiyAgents/tiycode/commit/8a342be50855b289b18c60c498e6d49c1b51b336) - **workbench**: add external link safety dialog and default thread workspace *(PR [#21](https://github.com/TiyAgents/tiycode/pull/21) by [@HayWolf](https://github.com/HayWolf))*
- [`96d50da`](https://github.com/TiyAgents/tiycode/commit/96d50da6a1fad4982793b2b151a5935ed7728aa5) - **git**: add branch selector with checkout, create and AI-powered naming *(PR [#23](https://github.com/TiyAgents/tiycode/pull/23) by [@jorben](https://github.com/jorben))*

### :bug: Bug Fixes
- [`f9e1129`](https://github.com/TiyAgents/tiycode/commit/f9e1129799a120e84a47920d0379545a2da32cca) - **approval**: 🐛 fix approval UI race conditions causing missing buttons and duplicate errors *(commit by [@jorben](https://github.com/jorben))*
- [`ac82dc3`](https://github.com/TiyAgents/tiycode/commit/ac82dc3f5c220fc0d330200e59ffb69fa8de1011) - **i18n**: 🐛 update zh-CN thread folder hint *(commit by [@jorben](https://github.com/jorben))*
- [`999dd0d`](https://github.com/TiyAgents/tiycode/commit/999dd0d4385fc4939d8ca3e396f68b752ca7c5db) - **extensions**: 🐛 Pass workspace path to extensions controller for skill discovery *(commit by [@jorben](https://github.com/jorben))*
- [`455fed3`](https://github.com/TiyAgents/tiycode/commit/455fed390dcad1f7f3e183fa1a5918ab7591594c) - **workbench**: 🐛 revert workspace path tracking on removal failure *(commit by [@jorben](https://github.com/jorben))*


## [0.2.4] - 2026-04-08
### :bug: Bug Fixes
- [`36c1cac`](https://github.com/TiyAgents/tiycode/commit/36c1cac7fb0f4423d19c34beba4e11baf2996ac2) - **provider**: 🐛 enforce provider_key uniqueness and deduplicate existing records *(commit by [@jorben](https://github.com/jorben))*
- [`3b9e7f2`](https://github.com/TiyAgents/tiycode/commit/3b9e7f2eb37cdf7ffa521ba499dbe26ffbe12cbd) - **windows**: 🕶️ hide console windows for background processes *(commit by [@jorben](https://github.com/jorben))*
- [`2119660`](https://github.com/TiyAgents/tiycode/commit/211966094f1b0c6ed10a1d1ce805e153f15c3fd1) - **core**: normalize workspace-relative paths for tests *(commit by [@jorben](https://github.com/jorben))*
- [`2e08d11`](https://github.com/TiyAgents/tiycode/commit/2e08d115cc33e4c3de761e1ca056a091285e591f) - **terminal**: 🐛 improve PTY shutdown and Windows DSR handling *(commit by [@jorben](https://github.com/jorben))*
- [`7d6c573`](https://github.com/TiyAgents/tiycode/commit/7d6c573c087d1f7c6d2d39ad26681459023f74c5) - **core/agent_session**: 🐛 improve Windows path assertion formatting *(commit by [@jorben](https://github.com/jorben))*

### :wrench: Chores
- [`2bf7e84`](https://github.com/TiyAgents/tiycode/commit/2bf7e84de3556cb422ec7044a2df1eb82dc0407f) - 🔧 address agents as Buddy in instructions *(commit by [@jorben](https://github.com/jorben))*


## [0.2.3] - 2026-04-08
### :sparkles: New Features
- [`c8043ce`](https://github.com/TiyAgents/tiycode/commit/c8043ce0731d5d7810c4d17d03c2cc2b721bdd56) - **updater**: ✨ detect Homebrew install and show brew upgrade command *(commit by [@jorben](https://github.com/jorben))*
- [`4429b2d`](https://github.com/TiyAgents/tiycode/commit/4429b2d5624a6286010674f9f720f51ae1ec128e) - **onboarding**: ✨ add first-time onboarding wizard *(commit by [@jorben](https://github.com/jorben))*

### :bug: Bug Fixes
- [`115cf04`](https://github.com/TiyAgents/tiycode/commit/115cf042c9bc193a324ddc2f4981c4a4314e9d7f) - **provider**: 🐛 enforce provider_key uniqueness and deduplicate existing records *(commit by [@jorben](https://github.com/jorben))*

### :zap: Performance Improvements
- [`e25d61f`](https://github.com/TiyAgents/tiycode/commit/e25d61fee2c59042a4ef62ae9c40fb31f87d43df) - **prompt**: ⚡️ move dynamic date out of system prompt for stable prefix caching *(commit by [@jorben](https://github.com/jorben))*


## [0.2.2] - 2026-04-08
### :zap: Performance Improvements
- [`5008c49`](https://github.com/TiyAgents/tiycode/commit/5008c495b90857aaab3934031f9cf60fe5153497) - **prompt**: ⚡️ move dynamic date out of system prompt for stable prefix caching *(commit by [@jorben](https://github.com/jorben))*

## [0.2.1] - 2026-04-08
### :sparkles: New Features
- [`828c573`](https://github.com/TiyAgents/tiycode/commit/828c573d365c5a04be9cd0a1e4e2ddafc2311da3) - **windows**: 🪟 add comprehensive Windows compatibility improvements *(commit by [@jorben](https://github.com/jorben))*

### :white_check_mark: Tests
- [`6d3864e`](https://github.com/TiyAgents/tiycode/commit/6d3864ea30003c175ea356a52995b7adc1bc0e0c) - **terminal**: ✅ update create_or_attach call to match new signature *(commit by [@jorben](https://github.com/jorben))*


## [0.2.0] - 2026-04-08
### :sparkles: New Features
- [`d7d883c`](https://github.com/TiyAgents/tiycode/commit/d7d883ccecec031250ff2672c7be034cfb5f3913) - **agent**: ✨ add thinking level setting to agent profiles *(commit by [@jorben](https://github.com/jorben))*


## [0.1.19] - 2026-04-08
### :sparkles: New Features
- [`a1715b7`](https://github.com/TiyAgents/tiycode/commit/a1715b76ab80a157afa1dafc76565b1b7f80adb0) - **terminal**: ✨ add configurable terminal settings *(commit by [@jorben](https://github.com/jorben))*


## [0.1.17] - 2026-04-08
### :wrench: Chores
- [`e2ea8c0`](https://github.com/TiyAgents/tiycode/commit/e2ea8c078b932ce642171e74237c41d433cffc09) - **release**: ♻️ tighten updater artifact collection and signing in workflow *(commit by [@jorben](https://github.com/jorben))*


## [0.1.16] - 2026-04-08
### :bug: Bug Fixes
- [`df048d6`](https://github.com/TiyAgents/tiycode/commit/df048d64dbf81c4ddfb12062281bbad57570575e) - **workbench-shell**: 🐛 Prevent tool state regressions during snapshot merges *(commit by [@jorben](https://github.com/jorben))*
- [`599cdde`](https://github.com/TiyAgents/tiycode/commit/599cddecb3f68244002fc4b5df3155ecfd9e824e) - **workbench-shell**: 🐛 reload runtime snapshot on lost run-finished events *(commit by [@jorben](https://github.com/jorben))*


## [0.1.14] - 2026-04-08
### :wrench: Chores
- [`273f575`](https://github.com/TiyAgents/tiycode/commit/273f575be1204a063eb728c308d4f7c50a4b3c8f) - **ci**: ✨ streamline Windows artifact collection in release workflow *(commit by [@jorben](https://github.com/jorben))*


## [0.1.13] - 2026-04-07
### :wrench: Chores
- [`674201f`](https://github.com/TiyAgents/tiycode/commit/674201fe97c2413a819988e31d27e06c50e10adb) - **ci**: 🔧 add empty releaseId to tauri release workflow *(commit by [@jorben](https://github.com/jorben))*


## [0.1.11] - 2026-04-07
### :wrench: Chores
- [`84bafec`](https://github.com/TiyAgents/tiycode/commit/84bafeca7afddc45b25e9fffed636ac54c823221) - **ci**: 🔧 refactor Windows release artifact copying in workflow *(commit by [@jorben](https://github.com/jorben))*


## [0.1.8] - 2026-04-07
### :wrench: Chores
- [`145365d`](https://github.com/TiyAgents/tiycode/commit/145365db4b731895bb6c12febb070b9527618ccf) - **ci**: 🔧 resolve ripgrep path reliably on macOS *(commit by [@jorben](https://github.com/jorben))*


## [0.1.6] - 2026-04-07
### :bug: Bug Fixes
- [`483c5c5`](https://github.com/TiyAgents/tiycode/commit/483c5c57081d289af6c993ad47a0691e71b17e92) - **system**: 🐛 include tests in containing_directory_path cfg *(commit by [@jorben](https://github.com/jorben))*

### :wrench: Chores
- [`13fb1e0`](https://github.com/TiyAgents/tiycode/commit/13fb1e097533294f5b6423ec180a3ebf8506992b) - **ci**: 📦 Download updater artifacts from release tag *(commit by [@jorben](https://github.com/jorben))*


## [0.1.5] - 2026-04-07
### :wrench: Chores
- [`8807286`](https://github.com/TiyAgents/tiycode/commit/8807286959f25de2e3b862e749cc975281119dea) - **ci**: 🔧 install ripgrep in CI environment *(commit by [@jorben](https://github.com/jorben))*
- [`49ae5de`](https://github.com/TiyAgents/tiycode/commit/49ae5de38f3265c73578ed6536c7d20db2486b7b) - **ci**: 🔧 export release metadata for Python subprocess *(commit by [@jorben](https://github.com/jorben))*


## [0.1.4] - 2026-04-07
### :sparkles: New Features
- [`7075a5b`](https://github.com/TiyAgents/tiycode/commit/7075a5b96651552197f8523d4cfcb9aa1cd43566) - **icons**: ✨ generate macOS iconset and update Tauri config *(commit by [@jorben](https://github.com/jorben))*

### :wrench: Chores
- [`ecfbb29`](https://github.com/TiyAgents/tiycode/commit/ecfbb29c9f4c8df7b24bbb2a745cad9ac1086d7d) - **icons**: ✨ generate multi-platform app icons *(commit by [@jorben](https://github.com/jorben))*


## [0.1.3] - 2026-04-07
### :sparkles: New Features
- [`833a5f4`](https://github.com/TiyAgents/tiycode/commit/833a5f45afbcbbd81e24f2bb6c0c0d173e11acbe) - **tauri**: ✨ bundle catalog snapshot for offline startup *(commit by [@jorben](https://github.com/jorben))*

### :wrench: Chores
- [`53af3a0`](https://github.com/TiyAgents/tiycode/commit/53af3a0891b5a5cfe7d90b500d177d16d52a9fc7) - **ci**: 🛠️ update macOS artifact naming and latest.json generation *(commit by [@jorben](https://github.com/jorben))*


## [0.1.2] - 2026-04-07
### :sparkles: New Features
- [`893f66a`](https://github.com/TiyAgents/tiycode/commit/893f66a3911a5465d91a3c2f7fdbb8bd8ae1141a) - **api**: ✨ add default identification headers to LLM requests *(commit by [@jorben](https://github.com/jorben))*
- [`2eb84a6`](https://github.com/TiyAgents/tiycode/commit/2eb84a6f344c52cf602cfbccdf688b71551de0b0) - **updater**: ✨ add in-app auto-update support *(commit by [@jorben](https://github.com/jorben))*

### :bug: Bug Fixes
- [`d556895`](https://github.com/TiyAgents/tiycode/commit/d55689550268618474b7fc12218d539aad663ec1) - **agent**: 🐛 pass max_turns into AgentSession creation

### :wrench: Chores
- [`6b40ce7`](https://github.com/TiyAgents/tiycode/commit/6b40ce7b5b3bb945b1c01bf7aeea079d02242410) - **ci**: 🔧 set Homebrew tap destination branch to master
- [`8b43541`](https://github.com/TiyAgents/tiycode/commit/8b43541b3b8cf9f3e6a08ad9d840d0da731f736c) - **i18n**: ✨ update About description copy *(commit by [@jorben](https://github.com/jorben))*


## [0.1.1] - 2026-04-07
### :boom: BREAKING CHANGES
- due to [`e37d5ec`](https://github.com/TiyAgents/tiycode/commit/e37d5eca168edf9a4ce9532c0758fa8d66bcdf73) - ✨ add runtime response controls and improve crash recovery *(commit by [@jorben](https://github.com/jorben))*:

  thread status mapping now includes `interrupted` as a distinct UI state instead of treating it only as a failed thread

- due to [`80f3f75`](https://github.com/TiyAgents/tiycode/commit/80f3f75821400b28e016745a3d0d3d0134d8f8b1) - ✨ add run subscription for remounted thread surfaces *(commit by [@jorben](https://github.com/jorben))*:

  UI now relies on `thread_subscribe_run` to resume live  
  thread updates after snapshot load.

- due to [`53f822e`](https://github.com/TiyAgents/tiycode/commit/53f822e0faa55f7ca97c1240e1ac1b7d330f6c32) - ✨ add structured `clarify` tool for in-run questions *(commit by [@jorben](https://github.com/jorben))*:

  remove `openQuestions` from `update_plan` artifacts/schema


### :sparkles: New Features
- [`b4ea1b2`](https://github.com/TiyAgents/tiycode/commit/b4ea1b28b6e83a4b6d0c63dae855ac0a3aa55654) - **windows**: ✨ Windows 平台隐藏系统菜单栏并添加应用内窗口控制按钮 *(commit by [@jorben](https://github.com/jorben))*
- [`c01569e`](https://github.com/TiyAgents/tiycode/commit/c01569e8beaf5d95574b95561abaa7a86e9b68d6) - **dashboard**: ✨ add project picker for threads *(commit by [@jorben](https://github.com/jorben))*
- [`62477f3`](https://github.com/TiyAgents/tiycode/commit/62477f39c29205f6020a435699ee36da4a0262ca) - **settings**: ✨ add workbench settings center *(commit by [@jorben](https://github.com/jorben))*
- [`5b58a9d`](https://github.com/TiyAgents/tiycode/commit/5b58a9d5ee517d641c672dd303a56fbb8222da9e) - **settings**: ✨ add Workspace & Provider settings panels with lobehub icons *(commit by [@jorben](https://github.com/jorben))*
- [`3f96286`](https://github.com/TiyAgents/tiycode/commit/3f96286dc590684773d753490641c43494e2ce03) - **settings**: ✨ add Usage panel to Account & revamp Prompts with slash commands *(commit by [@jorben](https://github.com/jorben))*
- [`b006fd5`](https://github.com/TiyAgents/tiycode/commit/b006fd5c3645f345f671e68e72e4821bae8522b4) - **settings**: ✨ revamp Approval Policy into Policy panel with sandbox & pattern lists *(commit by [@jorben](https://github.com/jorben))*
- [`3cbacb4`](https://github.com/TiyAgents/tiycode/commit/3cbacb485925a8dacb372730ab2895819dd55855) - **settings**: ✨ add multi-profile Agent Defaults & rename Prompts to Commands *(commit by [@jorben](https://github.com/jorben))*
- [`9b46e2d`](https://github.com/TiyAgents/tiycode/commit/9b46e2da13b3902d2b5267643dcfa935db8be6de) - **settings**: ✨ expand provider and model config *(commit by [@jorben](https://github.com/jorben))*
- [`5ce6a92`](https://github.com/TiyAgents/tiycode/commit/5ce6a9201f3b36cee8c9e98954a685d57c2fb306) - **settings**: ✨ polish workbench settings panels *(commit by [@jorben](https://github.com/jorben))*
- [`8a73c65`](https://github.com/TiyAgents/tiycode/commit/8a73c6536f746f7710f1abd39fed3d264f0e2045) - **ui**: ✨ refine settings and entry views *(commit by [@jorben](https://github.com/jorben))*
- [`718d40a`](https://github.com/TiyAgents/tiycode/commit/718d40a075c1ddb17dda4e1f003f0df90231af7f) - **settings**: ✨ localize and match LLM icons *(commit by [@jorben](https://github.com/jorben))*
- [`6fbfc79`](https://github.com/TiyAgents/tiycode/commit/6fbfc7960bacd31d8dac3ab0494aa84dddfcb633) - **workbench**: ✨ add cross-platform open in picker *(commit by [@jorben](https://github.com/jorben))*
- [`45a0178`](https://github.com/TiyAgents/tiycode/commit/45a017847be54a7c86e41d0844ee6cdd78802a96) - **marketplace**: ✨ Add marketplace overlay *(commit by [@jorben](https://github.com/jorben))*
- [`f1747cf`](https://github.com/TiyAgents/tiycode/commit/f1747cf632fd2d6cc181a88ec9bea46aa1ceca46) - **marketplace**: ✨ add automations tab *(commit by [@jorben](https://github.com/jorben))*
- [`3b7eb54`](https://github.com/TiyAgents/tiycode/commit/3b7eb54844808fa8ff012c9f48c17088bb8622e6) - **workbench**: ✨ expand open-in app support *(commit by [@jorben](https://github.com/jorben))*
- [`c9e70e8`](https://github.com/TiyAgents/tiycode/commit/c9e70e8618d062793eb8fb6a243b1e85bda998cc) - **workbench**: ✨ add AI Elements task demo *(commit by [@jorben](https://github.com/jorben))*
- [`bca61b2`](https://github.com/TiyAgents/tiycode/commit/bca61b28f75ba6ab9ca163d686b6fb9271ca51ec) - **workbench**: ✨ reuse task demo composer for new thread *(commit by [@jorben](https://github.com/jorben))*
- [`868a2cb`](https://github.com/TiyAgents/tiycode/commit/868a2cb8bc4e1395466ab8a15b5acbaa8efee89a) - **core**: 🏗️ implement M1.1 infrastructure and database layer *(commit by [@jorben](https://github.com/jorben))*
- [`6a75db5`](https://github.com/TiyAgents/tiycode/commit/6a75db5b31a0b9a93fe07fc8f37414c65f8d3ea7) - **workspace**: 🏗️ implement M1.2 workspace manager *(commit by [@jorben](https://github.com/jorben))*
- [`5335746`](https://github.com/TiyAgents/tiycode/commit/5335746c579686b181dc70fa92db1fd13a492f37) - **settings**: 🏗️ implement M1.3 settings and configuration system *(commit by [@jorben](https://github.com/jorben))*
- [`35f2a07`](https://github.com/TiyAgents/tiycode/commit/35f2a0754bfc20ac6d8bfea84e6b4cef43e1648c) - **thread**: 🏗️ implement M1.4 thread core *(commit by [@jorben](https://github.com/jorben))*
- [`cb530ab`](https://github.com/TiyAgents/tiycode/commit/cb530abe4ee6a150fea72206c5bb1c2745dc3174) - **agent**: 🏗️ implement M1.5 agent run and sidecar connection *(commit by [@jorben](https://github.com/jorben))*
- [`7e13b8b`](https://github.com/TiyAgents/tiycode/commit/7e13b8bc40cb54144e3f6de712cd7318cdcbef36) - **tools**: 🏗️ implement M1.6 tool gateway and policy engine *(commit by [@jorben](https://github.com/jorben))*
- [`6d7f773`](https://github.com/TiyAgents/tiycode/commit/6d7f773254e1951e98a214ffb0ad65e8e95b8595) - **frontend**: 🏗️ implement M1.7 frontend bridge and thread stream *(commit by [@jorben](https://github.com/jorben))*
- [`6dfcbe0`](https://github.com/TiyAgents/tiycode/commit/6dfcbe0e9d057b29f15a7f784fe3ea3a89718312) - **index**: 🏗️ implement M1.8 index foundation *(commit by [@jorben](https://github.com/jorben))*
- [`65da581`](https://github.com/TiyAgents/tiycode/commit/65da5814e85b70d5065fa2392567845cfd706b91) - **settings**: ✨ add active run sleep guard *(commit by [@jorben](https://github.com/jorben))*
- [`a3b5e6b`](https://github.com/TiyAgents/tiycode/commit/a3b5e6b51478f9b92fa0cf63a287f485a4ea5d29) - **settings**: ✨ add macOS settings shortcut *(commit by [@jorben](https://github.com/jorben))*
- [`9215b0e`](https://github.com/TiyAgents/tiycode/commit/9215b0ebbdac2745c0f677231d91b4567f9245b8) - **terminal**: ✨ add thread terminal integration *(commit by [@jorben](https://github.com/jorben))*
- [`106ef57`](https://github.com/TiyAgents/tiycode/commit/106ef57f69b8861ab0a4af8cd5db0ac709100031) - **treeview**: ✨ add lazy git-aware file tree *(commit by [@jorben](https://github.com/jorben))*
- [`0e4f2bb`](https://github.com/TiyAgents/tiycode/commit/0e4f2bbf9e0cef7025d36a4bf1d11c8ac3d9cc7c) - **treeview**: ✨ page heavy directories and show change badges *(commit by [@jorben](https://github.com/jorben))*
- [`0205504`](https://github.com/TiyAgents/tiycode/commit/02055048a800852fda00937f83ab123e496ecab8) - **treeview**: ✨ open files in preferred apps *(commit by [@jorben](https://github.com/jorben))*
- [`fcdcf62`](https://github.com/TiyAgents/tiycode/commit/fcdcf62e3daa0fb48524756a00253779fcec1964) - **git**: ✨ implement M2.2b Git panel workflow *(commit by [@jorben](https://github.com/jorben))*
- [`ac0e8b0`](https://github.com/TiyAgents/tiycode/commit/ac0e8b016105b69f0aabdcb778d5903088e4b605) - **settings**: ✨ connect provider settings to tiy-core *(commit by [@jorben](https://github.com/jorben))*
- [`4ad7070`](https://github.com/TiyAgents/tiycode/commit/4ad707023ed8f0c57bfc82b9640d97d5bebfc47e) - **provider**: ✨ add catalog-backed model sync *(commit by [@jorben](https://github.com/jorben))*
- [`d12ea77`](https://github.com/TiyAgents/tiycode/commit/d12ea7760d22c8b7f67a27867727b47d4f0d63f4) - **settings**: ✨ add model connection test *(commit by [@jorben](https://github.com/jorben))*
- [`cb2eb07`](https://github.com/TiyAgents/tiycode/commit/cb2eb07ff344fa9dee6eb7ffe9672ed3b758d1fe) - **agent**: ✨ add live subagent progress *(commit by [@jorben](https://github.com/jorben))*
- [`9c7835f`](https://github.com/TiyAgents/tiycode/commit/9c7835f51edc031f976e59cb85ac9a45891ae563) - **workbench**: ✨ sync sidebar threads with tauri state *(commit by [@jorben](https://github.com/jorben))*
- [`8e2738e`](https://github.com/TiyAgents/tiycode/commit/8e2738e8a4a3c39dcb35ace5b84d2cb96bc7a2fa) - **agent**: ✨ refine thread runtime interactions *(commit by [@jorben](https://github.com/jorben))*
- [`1f2a725`](https://github.com/TiyAgents/tiycode/commit/1f2a7250f59411f95a4535b5f1ff32150a20b2b1) - **workbench**: ✨ add workspace sidebar quick actions *(commit by [@jorben](https://github.com/jorben))*
- [`dc735ac`](https://github.com/TiyAgents/tiycode/commit/dc735acb8828ae6fa794306a05ceacb86c2d3626) - **workbench**: ✨ group completed tool activity *(commit by [@jorben](https://github.com/jorben))*
- [`64a6af3`](https://github.com/TiyAgents/tiycode/commit/64a6af3d4ea7eb740d94ce815f6e831be619630e) - **thread**: ✨ persist reasoning and tool activity *(commit by [@jorben](https://github.com/jorben))*
- [`cf19e28`](https://github.com/TiyAgents/tiycode/commit/cf19e280b6d35ade401b7b6ca171a1aad15836d6) - **runtime**: ✨ improve thread titles and thinking UX *(commit by [@jorben](https://github.com/jorben))*
- [`a616cae`](https://github.com/TiyAgents/tiycode/commit/a616cae0bcc410daf833d04e776fbd50b90d1a9d) - **core**: ✨ add edit tool, image read, context compression and unified truncation *(commit by [@jorben](https://github.com/jorben))*
- [`4e72e62`](https://github.com/TiyAgents/tiycode/commit/4e72e626e9f4fc2543f204d15d462b6ffcf92287) - **thread**: ✨ simplify SubAgent helper UI *(commit by [@jorben](https://github.com/jorben))*
- [`9e7fb70`](https://github.com/TiyAgents/tiycode/commit/9e7fb708c623c3b3d241d4dac67af9f03546a4a1) - **core**: ✨ improve system prompts, subagent profiles, and thread timeline spacing *(commit by [@jorben](https://github.com/jorben))*
- [`631de4d`](https://github.com/TiyAgents/tiycode/commit/631de4d96cafd8a3e2ea1eb234ef6ad5b0c93f6d) - **runtime**: ✨ surface usage and stabilize new runs *(commit by [@jorben](https://github.com/jorben))*
- [`5cbed8f`](https://github.com/TiyAgents/tiycode/commit/5cbed8f1a454ba3160e3cbb07015b09fb0335b1e) - **git**: ✨ add profile-configured AI commit message generation *(commit by [@jorben](https://github.com/jorben))*
- [`ebd5e9e`](https://github.com/TiyAgents/tiycode/commit/ebd5e9ebffad5d6d60dfda4f84816645e50f4b22) - **agent-session**: ✨ enhance system prompt with workspace context and sandbox guidance *(commit by [@jorben](https://github.com/jorben))*
- [`e37d5ec`](https://github.com/TiyAgents/tiycode/commit/e37d5eca168edf9a4ce9532c0758fa8d66bcdf73) - **runtime**: ✨ add runtime response controls and improve crash recovery *(commit by [@jorben](https://github.com/jorben))*
- [`a0854ef`](https://github.com/TiyAgents/tiycode/commit/a0854efa903f38c900417780c77195e6b7f8e1e0) - **settings**: ✨ persist DB-backed agent profiles and pattern-based policy checks *(commit by [@jorben](https://github.com/jorben))*
- [`b0c7742`](https://github.com/TiyAgents/tiycode/commit/b0c77429c634b2297809f1dd76f7eb516237d4a2) - **settings-center**: ✨ add tauri workspace add/remove/open support and sync default workspaces *(commit by [@jorben](https://github.com/jorben))*
- [`5511415`](https://github.com/TiyAgents/tiycode/commit/551141506ee1aca747cd1ba69d89d21291abd94a) - **workbench**: ✨ add unified diff line counts to filesystem writes and improve tool UI rendering *(commit by [@jorben](https://github.com/jorben))*
- [`96d15c8`](https://github.com/TiyAgents/tiycode/commit/96d15c8d559c44c45b03481b25e7af946a9ecb87) - **subagent**: ✨ emit plan review artifacts and update helper kind *(commit by [@jorben](https://github.com/jorben))*
- [`afee6ee`](https://github.com/TiyAgents/tiycode/commit/afee6ee2a92f07c7be99cf44c493fef81d907f49) - **ai-elements**: ✨ refine code block surfaces *(commit by [@jorben](https://github.com/jorben))*
- [`e578b50`](https://github.com/TiyAgents/tiycode/commit/e578b5023b03e51b3338ed1b9fd0e1945c176f9c) - **workbench-shell**: ✨ add command output rendering for shell/git/terminal tools *(commit by [@jorben](https://github.com/jorben))*
- [`1ce78c9`](https://github.com/TiyAgents/tiycode/commit/1ce78c9ed4dcb359dce35ed77d91e447d3e18ba7) - **workbench**: ✨ add per-workspace incremental thread loading *(commit by [@jorben](https://github.com/jorben))*
- [`272d2ec`](https://github.com/TiyAgents/tiycode/commit/272d2ec207e762d53ce9b5c13954f3781cc01924) - **terminal**: ✨ Clarify and expose thread-scoped terminal panel tools *(commit by [@jorben](https://github.com/jorben))*
- [`80f3f75`](https://github.com/TiyAgents/tiycode/commit/80f3f75821400b28e016745a3d0d3d0134d8f8b1) - **tauri-thread-stream**: ✨ add run subscription for remounted thread surfaces *(commit by [@jorben](https://github.com/jorben))*
- [`8af1022`](https://github.com/TiyAgents/tiycode/commit/8af1022b6e311686995c55e1169834a2e95f2f5c) - **ipc**: ✨ broadcast thread lifecycle and title updates to sidebar *(commit by [@jorben](https://github.com/jorben))*
- [`54d1f08`](https://github.com/TiyAgents/tiycode/commit/54d1f082a3c78f41d9b668cbebd8972c22e8addc) - **filesystem**: ✨ add offset/limit windowing for read/list/find tools *(commit by [@jorben](https://github.com/jorben))*
- [`885cbbf`](https://github.com/TiyAgents/tiycode/commit/885cbbff3f0210a0cfa003f387cafaebb3d97288) - **workbench**: ✨ add run mode toggle and improve attachment cards *(commit by [@jorben](https://github.com/jorben))*
- [`6ce4327`](https://github.com/TiyAgents/tiycode/commit/6ce43273d573c359888e60378dbb25f48121cfa6) - **agent**: ✨ add approval-gated implementation plans with checkpoints *(commit by [@jorben](https://github.com/jorben))*
- [`56ee903`](https://github.com/TiyAgents/tiycode/commit/56ee903b70615b212e75095c1d136a7586855149) - **agents**: ✨ enforce max turns and surface limit_reached state *(commit by [@jorben](https://github.com/jorben))*
- [`bf03e3e`](https://github.com/TiyAgents/tiycode/commit/bf03e3e6d15ba07bdaba7cd1bdd810ebe21425c8) - **runtime**: ✨ handle incomplete stream with discarded turns and retries *(commit by [@jorben](https://github.com/jorben))*
- [`53f822e`](https://github.com/TiyAgents/tiycode/commit/53f822e0faa55f7ca97c1240e1ac1b7d330f6c32) - **agent**: ✨ add structured `clarify` tool for in-run questions *(commit by [@jorben](https://github.com/jorben))*
- [`3562b2d`](https://github.com/TiyAgents/tiycode/commit/3562b2df4a667533fd6be7f648c0b813eafe62b8) - **workbench**: ✨ add built-in clear/compact slash commands and context controls *(commit by [@jorben](https://github.com/jorben))*
- [`295d113`](https://github.com/TiyAgents/tiycode/commit/295d113aa735b2757460fe0eded25dd911a3e847) - **thread**: ✨ add load-older-messages pagination with snapshot fix *(commit by [@jorben](https://github.com/jorben))*
- [`274554f`](https://github.com/TiyAgents/tiycode/commit/274554f62ee2f64f7030f20ad609b7889a7d107a) - **compact**: ✨ add AI-powered compact summary generation *(commit by [@jorben](https://github.com/jorben))*
- [`a9b15f6`](https://github.com/TiyAgents/tiycode/commit/a9b15f62d9529ab799cd64fc1ee220b5bbe2b12a) - **project-panel**: ✨ add copy relative path action for project items *(commit by [@jorben](https://github.com/jorben))*
- [`b4f509d`](https://github.com/TiyAgents/tiycode/commit/b4f509d296e4741133eb42a4bbc36e406e946f4d) - **task-tracking**: ✨ add per-thread task boards and tools *(commit by [@jorben](https://github.com/jorben))*
- [`1ee67c9`](https://github.com/TiyAgents/tiycode/commit/1ee67c9523afe48246d928941684f655031ec1f8) - **attachments**: ✨ add message/file attachments support *(commit by [@jorben](https://github.com/jorben))*
- [`4e8291a`](https://github.com/TiyAgents/tiycode/commit/4e8291a146337279865745e40bede4a5f7b2fefc) - **task-tracking**: ✨ add advance_step and reconcile active boards *(commit by [@jorben](https://github.com/jorben))*
- [`658791e`](https://github.com/TiyAgents/tiycode/commit/658791e40d023cfeb75264e140c119f0de5a622a) - **plan-mode**: ✨ add structured plan sections with backward-compatible rendering *(commit by [@jorben](https://github.com/jorben))*
- [`864fe6d`](https://github.com/TiyAgents/tiycode/commit/864fe6de5164c3beb3339afa08b30294f480b89a) - **core/agent-session**: ✨ expose structured plan sections in update_plan tool schema *(commit by [@jorben](https://github.com/jorben))*
- [`aca769e`](https://github.com/TiyAgents/tiycode/commit/aca769ebf61f3c948be2b9dd2adb1f90b2951bfd) - **workbench**: ✨ add referenced file mention support and snapshot resync *(commit by [@jorben](https://github.com/jorben))*
- [`9d23d45`](https://github.com/TiyAgents/tiycode/commit/9d23d45210022a63d6fdc783346888f63f7bf7af) - **workbench**: ✨ add configurable Command props to ModelSelector *(commit by [@jorben](https://github.com/jorben))*
- [`17ba6d8`](https://github.com/TiyAgents/tiycode/commit/17ba6d8ccc2bfd17614a0a0e2db740050178d2f3) - **extensions**: ✨ add extensions center runtime with plugin/MCP/skills support *(commit by [@jorben](https://github.com/jorben))*
- [`89cdbfd`](https://github.com/TiyAgents/tiycode/commit/89cdbfd426340c8d3f8d3be8aa04d45345b60b7b) - **extensions-center**: ✨ improve MCP server UI and plugin config merge behavior *(commit by [@jorben](https://github.com/jorben))*
- [`741abfb`](https://github.com/TiyAgents/tiycode/commit/741abfbc1f716e442a4936bcc99acb1255e846da) - **prompt-skills**: ✨ inject enabled skills into system prompt *(commit by [@jorben](https://github.com/jorben))*
- [`6988880`](https://github.com/TiyAgents/tiycode/commit/6988880397e79c45e0382b4f0e069e3f4201f4f4) - **extensions-mcp**: ✨ add streamable-http MCP support and header UI *(commit by [@jorben](https://github.com/jorben))*
- [`0e37f02`](https://github.com/TiyAgents/tiycode/commit/0e37f02cb32fcb9e4da1f57b630d1e78a72c19be) - **workspace-paths**: ✨ merge persisted and builtin writable roots *(commit by [@jorben](https://github.com/jorben))*
- [`0931194`](https://github.com/TiyAgents/tiycode/commit/09311941c75555df9312988fb2ca9d9da73d166e) - **workbench**: ✨ enable $skill mentions with enabled skill filtering *(commit by [@jorben](https://github.com/jorben))*
- [`7c892f2`](https://github.com/TiyAgents/tiycode/commit/7c892f2ddd9b16c15d4cd62ddf99198a5a53b53f) - **ui**: ✨ add auto-scroll to bottom on message submission *(commit by [@jorben](https://github.com/jorben))*
- [`3bb2e4d`](https://github.com/TiyAgents/tiycode/commit/3bb2e4d2aaf587c13d788fcef89da40876a37a32) - **workbench**: 📝 persist composer draft per thread *(commit by [@jorben](https://github.com/jorben))*
- [`906d639`](https://github.com/TiyAgents/tiycode/commit/906d639e10dee496f3eff152a8dc91c2b3fcf35f) - **tasks**: ✨ add query_task tool and task-board recovery guidance *(commit by [@jorben](https://github.com/jorben))*
- [`ad00c56`](https://github.com/TiyAgents/tiycode/commit/ad00c567095f001d988d74fa0296db0bd5a8fd4c) - **desktop**: ✨ add launch-at-login and minimize-to-tray settings with tray UI *(commit by [@jorben](https://github.com/jorben))*
- [`ba472bd`](https://github.com/TiyAgents/tiycode/commit/ba472bd5c68b542bb63042c636855e225fd67b79) - **extensions-center**: ✨ preview marketplace source removal plan *(commit by [@jorben](https://github.com/jorben))*
- [`27f6dfd`](https://github.com/TiyAgents/tiycode/commit/27f6dfd7022106fdd01a5baf466fd59f4eff5c40) - **extensions-center**: ✨ add marketplace source sync feedback and error handling *(commit by [@jorben](https://github.com/jorben))*
- [`53be886`](https://github.com/TiyAgents/tiycode/commit/53be886a85eda7861773f210aabcb9a3a303645d) - **i18n**: ✨ add runtime translation hooks for terminal and system info *(commit by [@jorben](https://github.com/jorben))*
- [`1898977`](https://github.com/TiyAgents/tiycode/commit/1898977e8b78d73c46912b357613b7e1c497bea5) - **workspace**: ✨ add system and cache writable roots at runtime *(commit by [@jorben](https://github.com/jorben))*
- [`f1f6f91`](https://github.com/TiyAgents/tiycode/commit/f1f6f91c7ae04cdac40ef85165c680933027c8c2) - **core**: ✨ make agent max turns configurable *(commit by [@jorben](https://github.com/jorben))*
- [`703b307`](https://github.com/TiyAgents/tiycode/commit/703b30746523c9fb05a130e2b19e517ee202cb54) - **settings**: ✨ support prefixed policy rules *(commit by [@jorben](https://github.com/jorben))*

### :bug: Bug Fixes
- [`5aa4b57`](https://github.com/TiyAgents/tiycode/commit/5aa4b576d2cf8fe7c9180a5905df3fff599547f1) - **git-panel**: 🐛 rename tracked list to changes *(commit by [@jorben](https://github.com/jorben))*
- [`8d3cb59`](https://github.com/TiyAgents/tiycode/commit/8d3cb592b644da6dc73707d9142124a32d588c32) - **workbench**: 🐛 Restrict text selection *(commit by [@jorben](https://github.com/jorben))*
- [`9897ad0`](https://github.com/TiyAgents/tiycode/commit/9897ad078bd7917d603f6d04826f2f17e1c24c2a) - **ui**: 🐛 修复工作台组件 token 使用不规范问题 *(commit by [@jorben](https://github.com/jorben))*
- [`9ec00d6`](https://github.com/TiyAgents/tiycode/commit/9ec00d687234cc31402b0a4134e55ab24ee5b741) - **account**: 🐛 stabilize activity heatmap *(commit by [@jorben](https://github.com/jorben))*
- [`b0c6efd`](https://github.com/TiyAgents/tiycode/commit/b0c6efd6c8d350f747ba4d6c4c391400dd3261c3) - **shell**: 🐛 restore window state and simplify empty view *(commit by [@jorben](https://github.com/jorben))*
- [`5482389`](https://github.com/TiyAgents/tiycode/commit/54823890a0f9ac6a5e8f8267d4d0650e1dce5d77) - **settings**: 🐛 correct model icons in agent defaults *(commit by [@jorben](https://github.com/jorben))*
- [`0ca91e5`](https://github.com/TiyAgents/tiycode/commit/0ca91e5f6a2fad4aad96f067468953f3d24e0005) - **workbench**: 🐛 keep composer above workspace menu *(commit by [@jorben](https://github.com/jorben))*
- [`ef29e67`](https://github.com/TiyAgents/tiycode/commit/ef29e67fe45819c964e91a8055c523179a957de3) - **workbench**: 🐛 improve windows open-in handling *(commit by [@jorben](https://github.com/jorben))*
- [`bd2cc60`](https://github.com/TiyAgents/tiycode/commit/bd2cc60d5f3ca8a103aa5c13e38a33173935a45e) - **ui**: 🐛 Restore Windows title bar maximize toggle *(commit by [@jorben](https://github.com/jorben))*
- [`6a25ace`](https://github.com/TiyAgents/tiycode/commit/6a25aceb8eb0ecd2350c1891a56c81ecc9fa4c07) - **ui**: 🐛 polish settings overlay and profile layout *(commit by [@jorben](https://github.com/jorben))*
- [`44aa828`](https://github.com/TiyAgents/tiycode/commit/44aa82898f3989885d614e20ee0f785a3f630299) - **runtime**: 🐛 guard tauri-only desktop APIs *(commit by [@jorben](https://github.com/jorben))*
- [`5a67ce1`](https://github.com/TiyAgents/tiycode/commit/5a67ce1548529899587bc6185551dbdd80d40d0e) - **workbench**: 🐛 avoid AppleScript when opening terminal apps *(commit by [@jorben](https://github.com/jorben))*
- [`ebc1ca7`](https://github.com/TiyAgents/tiycode/commit/ebc1ca7ae56e9da6c42e3958184d6b9cefc2521b) - **workbench**: 🐛 remove task demo composer backdrop *(commit by [@jorben](https://github.com/jorben))*
- [`5299b5d`](https://github.com/TiyAgents/tiycode/commit/5299b5d4a6a1f7f7e4187018bd652b56863e7a55) - **settings**: 🐛 restore general profile picker dropdown *(commit by [@jorben](https://github.com/jorben))*
- [`888b74c`](https://github.com/TiyAgents/tiycode/commit/888b74ce0ecf8c76ef830ca42e6d31409f6b825a) - **runtime**: 🐛 remove duplicate window state restore *(commit by [@jorben](https://github.com/jorben))*
- [`295ca09`](https://github.com/TiyAgents/tiycode/commit/295ca09f8c0b5dc387ce8b5b543fbb334fa30343) - **theme**: 🐛 avoid startup white flash in dark mode *(commit by [@jorben](https://github.com/jorben))*
- [`8acd4a6`](https://github.com/TiyAgents/tiycode/commit/8acd4a658f8a9b58ad4bee82cd426c9af8d09e89) - **settings**: 🐛 strengthen segmented control active state *(commit by [@jorben](https://github.com/jorben))*
- [`1f5a9f0`](https://github.com/TiyAgents/tiycode/commit/1f5a9f056db43ca0f29917b6e39d440c7aa6596d) - **review**: 🔧 address Phase 1 code review findings *(commit by [@jorben](https://github.com/jorben))*
- [`aca17d3`](https://github.com/TiyAgents/tiycode/commit/aca17d321276718535e5544d8ebacd1aed449ba6) - **terminal**: 🐛 stabilize thread terminal switching *(commit by [@jorben](https://github.com/jorben))*
- [`f297174`](https://github.com/TiyAgents/tiycode/commit/f297174b94e7902ff03c61efea9affab84f10a52) - **terminal**: 🐛 preserve unicode terminal rendering *(commit by [@jorben](https://github.com/jorben))*
- [`bd2d2cc`](https://github.com/TiyAgents/tiycode/commit/bd2d2cce67df43f50adfb28cc58c0892211d7ac2) - **terminal**: 🐛 reset replay after screen clear *(commit by [@jorben](https://github.com/jorben))*
- [`7538acb`](https://github.com/TiyAgents/tiycode/commit/7538acb10c9a0a1e28a5a0e47af0f3a7644bdd39) - **terminal**: 🐛 hide pending thread title *(commit by [@jorben](https://github.com/jorben))*
- [`5481943`](https://github.com/TiyAgents/tiycode/commit/54819437c42286672175e5663e79ec98543912e5) - **treeview**: 🐛 reveal filtered files without accidental open *(commit by [@jorben](https://github.com/jorben))*
- [`74be56e`](https://github.com/TiyAgents/tiycode/commit/74be56eddf584ea9ed3b220e129fbd561ee5a68f) - **treeview**: 🐛 avoid workspace bootstrap stalls *(commit by [@jorben](https://github.com/jorben))*
- [`334b7b8`](https://github.com/TiyAgents/tiycode/commit/334b7b8b29304ce9bf4fab6f310f74ce090b630d) - **terminal**: 🐛 unlock new-thread terminal after workspace select *(commit by [@jorben](https://github.com/jorben))*
- [`ff48f95`](https://github.com/TiyAgents/tiycode/commit/ff48f955e09b392c236a54a2eb434a9a6852c10e) - **treeview**: 🐛 materialize filtered file targets in tree *(commit by [@jorben](https://github.com/jorben))*
- [`39f91c0`](https://github.com/TiyAgents/tiycode/commit/39f91c000d386fc689161501939309730231bc5f) - **treeview**: 🐛 add manual tree refresh control *(commit by [@jorben](https://github.com/jorben))*
- [`e93c366`](https://github.com/TiyAgents/tiycode/commit/e93c366900726fe2882a1e0653b27c24c943be95) - **treeview**: 🐛 use generic icons for extensionless files *(commit by [@jorben](https://github.com/jorben))*
- [`7099ce8`](https://github.com/TiyAgents/tiycode/commit/7099ce8c03f1abc80887c422030dd58998794fd9) - **git**: 🐛 polish Git panel interactions *(commit by [@jorben](https://github.com/jorben))*
- [`f734a82`](https://github.com/TiyAgents/tiycode/commit/f734a823415ee91802b3b1985e886adffda2570b) - **git**: 🐛 refine Git history details *(commit by [@jorben](https://github.com/jorben))*
- [`e9f6a9f`](https://github.com/TiyAgents/tiycode/commit/e9f6a9f9bf8a2fa69e235f66b6de696287ed6e6f) - **git**: 🐛 polish Git history copy feedback *(commit by [@jorben](https://github.com/jorben))*
- [`207a8a0`](https://github.com/TiyAgents/tiycode/commit/207a8a01d6ee922e839a99b215b3d5fced02ff6b) - **git**: 🐛 replace remote action confirms with modal dialogs *(commit by [@jorben](https://github.com/jorben))*
- [`f9f0163`](https://github.com/TiyAgents/tiycode/commit/f9f01634b05b863995ad4a78d128813b5d9a4fd2) - **git**: 🐛 unify Git panel commit and remote confirmation flow *(commit by [@jorben](https://github.com/jorben))*
- [`a06038a`](https://github.com/TiyAgents/tiycode/commit/a06038aa90545a7399c706f2096b2da64fa2d3ac) - **git**: 🐛 keep Git history from crowding out changes during resize *(commit by [@jorben](https://github.com/jorben))*
- [`0b390a4`](https://github.com/TiyAgents/tiycode/commit/0b390a425e72212c395aa8d5aa2d223eb856d6dc) - **workbench**: 🐛 sync workspace context for threads *(commit by [@jorben](https://github.com/jorben))*
- [`35c0a35`](https://github.com/TiyAgents/tiycode/commit/35c0a35ed33a8520d58ee98187ccb380313a8f28) - **workbench**: 🐛 keep new thread project menu on top *(commit by [@jorben](https://github.com/jorben))*
- [`2cb38d7`](https://github.com/TiyAgents/tiycode/commit/2cb38d78497eb6238e708f7b5a45cfc721c9b857) - **agent**: 🐛 wire model plans and provider options *(commit by [@jorben](https://github.com/jorben))*
- [`6ac4fdd`](https://github.com/TiyAgents/tiycode/commit/6ac4fdd2bb0b0d6f4e114d25d2001d39536f46cb) - **workbench**: 🐛 scope terminal collapse per thread *(commit by [@jorben](https://github.com/jorben))*
- [`4e6cfba`](https://github.com/TiyAgents/tiycode/commit/4e6cfba7e531abf144d34f58d6607110ae9351f6) - **settings**: 🐛 default model test stream options *(commit by [@jorben](https://github.com/jorben))*
- [`69f375f`](https://github.com/TiyAgents/tiycode/commit/69f375fad8fda2e0b721c1fadd253f69d1778606) - **core**: 🐛 unify workspace path boundary checks *(commit by [@jorben](https://github.com/jorben))*
- [`4bad320`](https://github.com/TiyAgents/tiycode/commit/4bad320a1d6d65a866e7446e9c5440be420373d3) - **agent**: 🐛 persist runtime errors outside thread history *(commit by [@jorben](https://github.com/jorben))*
- [`a719243`](https://github.com/TiyAgents/tiycode/commit/a71924333e0bf5efd299f3563e3a912911944aa3) - **core**: 🐛 harden ripgrep lookup and trim test warnings *(commit by [@jorben](https://github.com/jorben))*
- [`162130d`](https://github.com/TiyAgents/tiycode/commit/162130d0485377b03490a5731abca6f65c6dc206) - **workbench**: 🐛 restore composer Home/End caret movement *(commit by [@jorben](https://github.com/jorben))*
- [`cd228ea`](https://github.com/TiyAgents/tiycode/commit/cd228eaa50abcf405fee6409644d6b3c3bf66bac) - **core**: 🐛 bundle ripgrep for tauri builds *(commit by [@jorben](https://github.com/jorben))*
- [`665eb53`](https://github.com/TiyAgents/tiycode/commit/665eb5334bb5aacfadaa37c8084c52a3d8fb3889) - **thread**: 🐛 clean up helper timeline UI *(commit by [@jorben](https://github.com/jorben))*
- [`f8b9687`](https://github.com/TiyAgents/tiycode/commit/f8b9687a897826e4cf9e8ce02d7c238372568a8e) - **thread**: 🐛 refine subagent activity cards *(commit by [@jorben](https://github.com/jorben))*
- [`a8b8a09`](https://github.com/TiyAgents/tiycode/commit/a8b8a09bf5a9b7ef938772c00bbcc0c05c4d3207) - **build**: 🐛 replace bundled ripgrep safely *(commit by [@jorben](https://github.com/jorben))*
- [`8023613`](https://github.com/TiyAgents/tiycode/commit/80236130e474a1c393777907dc96076a31860163) - **thread**: 🐛 fix duplicate user message and [object Object] error *(commit by [@jorben](https://github.com/jorben))*
- [`6b29e7a`](https://github.com/TiyAgents/tiycode/commit/6b29e7a9cf54848f9b5596362dae0ae9cc3474af) - **thread**: 🐛 prevent double startRun from initialPromptRequest race *(commit by [@jorben](https://github.com/jorben))*
- [`10f4b2b`](https://github.com/TiyAgents/tiycode/commit/10f4b2b492c057698cb59769c32b97437fab7f15) - **thread**: 🐛 fix generated title not applied due to syncWorkspaceSidebar race *(commit by [@jorben](https://github.com/jorben))*
- [`d05811c`](https://github.com/TiyAgents/tiycode/commit/d05811c4687dabb3330394266494d8839b32140b) - **runtime**: 🐛 preserve reasoning segments *(commit by [@jorben](https://github.com/jorben))*
- [`c2e1176`](https://github.com/TiyAgents/tiycode/commit/c2e1176d293951341027c5a1b6aecda5e00228c1) - **thread**: 🐛 align thought and tool group spacing *(commit by [@jorben](https://github.com/jorben))*
- [`59fe91b`](https://github.com/TiyAgents/tiycode/commit/59fe91b179a582ff5f93ca7d31cf0435cce39b6a) - **runtime**: 🐛 skip empty reasoning blocks *(commit by [@jorben](https://github.com/jorben))*
- [`92ec783`](https://github.com/TiyAgents/tiycode/commit/92ec7830b39ce0d5fda8474e0b9e7e8c8a94fda8) - **settings**: 🔧 reorder response style options to default concise first *(commit by [@jorben](https://github.com/jorben))*
- [`916fc1d`](https://github.com/TiyAgents/tiycode/commit/916fc1d68e4084847a3a1c47b1058ba050a4b558) - **git**: 🐛 disable commit without staged changes *(commit by [@jorben](https://github.com/jorben))*
- [`66f8287`](https://github.com/TiyAgents/tiycode/commit/66f8287a32bf6eb5e55b800906840fcaee57418d) - **settings-center**: 🐛 enable provider on api key update *(commit by [@jorben](https://github.com/jorben))*
- [`33ec8ff`](https://github.com/TiyAgents/tiycode/commit/33ec8ffe7a993ea44d8140799584c6799c818089) - **thread**: 🧹 cancel active runs before deleting threads *(commit by [@jorben](https://github.com/jorben))*
- [`f0425c1`](https://github.com/TiyAgents/tiycode/commit/f0425c1567539a53646a7c7c227feb29a934930a) - **ai-elements**: ✨ align code block header actions *(commit by [@jorben](https://github.com/jorben))*
- [`265c4b8`](https://github.com/TiyAgents/tiycode/commit/265c4b8535135798ed087deeed64adf1b7df23e1) - **search**: 🐛 normalize filePattern, cap preview results, and improve rg failures *(commit by [@jorben](https://github.com/jorben))*
- [`459cda8`](https://github.com/TiyAgents/tiycode/commit/459cda87724b2d248b7c4efb59d3f21bdf7030c8) - **code-block**: 🐛 improve code block layout and visibility *(commit by [@jorben](https://github.com/jorben))*
- [`d5954f6`](https://github.com/TiyAgents/tiycode/commit/d5954f6f1f5a361f99e85eb22afd4d37f3f6670a) - **workbench-shell**: 🐛 enable Enter key to confirm dialogs *(commit by [@jorben](https://github.com/jorben))*
- [`dfa9c0b`](https://github.com/TiyAgents/tiycode/commit/dfa9c0b4be1e09b4552737bf586eab5fcc1392ae) - **runtime**: 🧩 handle tool failure and improve diff/line fallbacks *(commit by [@jorben](https://github.com/jorben))*
- [`e9b5896`](https://github.com/TiyAgents/tiycode/commit/e9b589691375d30880f2a5a8e744263fb5ee4bf0) - **git overlay**: 🐛 do not bubble ignored state into parents *(commit by [@jorben](https://github.com/jorben))*
- [`d72527c`](https://github.com/TiyAgents/tiycode/commit/d72527c8a67be0c1d0edca51c9623d34efff31a6) - **settings**: 🐛 use safer provider model max tokens *(commit by [@jorben](https://github.com/jorben))*
- [`8a08949`](https://github.com/TiyAgents/tiycode/commit/8a08949a903d705d0de0c37d88e2455477393a65) - **thread-stream**: 🐛 emit approval events for hidden tool call IDs *(commit by [@jorben](https://github.com/jorben))*
- [`e47f2e2`](https://github.com/TiyAgents/tiycode/commit/e47f2e236bd93c95739ae91b0e1bba99c3cbb46d) - **thread-manager**: 🧯 recover dangling tool calls and run helpers on startup *(commit by [@jorben](https://github.com/jorben))*
- [`bdc0db0`](https://github.com/TiyAgents/tiycode/commit/bdc0db09bf4ea9b2071d5d0cfd4ac0f1ce487c51) - **workbench-shell**: 🐛 correct read range label calculation *(commit by [@jorben](https://github.com/jorben))*
- [`1f8ce26`](https://github.com/TiyAgents/tiycode/commit/1f8ce26cfad097a99face917fb684ae1f14301bb) - **workbench-shell/ui**: 🐛 handle limit-reached run state in runtime surface *(commit by [@jorben](https://github.com/jorben))*
- [`e2fa5f7`](https://github.com/TiyAgents/tiycode/commit/e2fa5f7d270eb2fba5e2c598391e07b4c1945724) - **workbench-shell**: 🐛 show helper name with shimmer while running *(commit by [@jorben](https://github.com/jorben))*
- [`7820f97`](https://github.com/TiyAgents/tiycode/commit/7820f97a67e55228830cf28f12cd81f13f9fd711) - **workbench-shell**: 🐛 show thinking placeholder after run completion *(commit by [@jorben](https://github.com/jorben))*
- [`c6453b5`](https://github.com/TiyAgents/tiycode/commit/c6453b552329bcea30f973dfb71c81e2d1cb3a46) - **workbench-shell**: 🐛 stop switching to plan on run checkpoints *(commit by [@jorben](https://github.com/jorben))*
- [`43fbaad`](https://github.com/TiyAgents/tiycode/commit/43fbaad9f8463e7395d7f248c0c07b62e1969ca5) - **agent-session**: 🐛 fail plan runs without published update_plan checkpoint *(commit by [@jorben](https://github.com/jorben))*
- [`e34676c`](https://github.com/TiyAgents/tiycode/commit/e34676ce32e481b53883470574a38a6aeffb7967) - **workbench-shell**: 🐛 improve slash command context handling *(commit by [@jorben](https://github.com/jorben))*
- [`6932bd2`](https://github.com/TiyAgents/tiycode/commit/6932bd24c8df2bc3740d350dd92ba84b83964604) - **core**: 🐛 retry thread title generation message count check *(commit by [@jorben](https://github.com/jorben))*
- [`f567e11`](https://github.com/TiyAgents/tiycode/commit/f567e114c0415faa50eab7eabdb148c47eef0f63) - **runtime-thread**: 🐛 preserve context usage during plan approval flow *(commit by [@jorben](https://github.com/jorben))*
- [`59e232f`](https://github.com/TiyAgents/tiycode/commit/59e232f583bd58dd5723e85a003c01f2b0145376) - **workbench-shell**: 🐛 preserve context usage correctly during empty snapshots *(commit by [@jorben](https://github.com/jorben))*
- [`b86a076`](https://github.com/TiyAgents/tiycode/commit/b86a076350e2bdcf6d07bc69be8c46198a5b85d2) - **workbench**: 🐛 merge local fallback thread titles *(commit by [@jorben](https://github.com/jorben))*
- [`d7d40d5`](https://github.com/TiyAgents/tiycode/commit/d7d40d53c12b88058d8d71231bcbec54a8918d3a) - **workbench-shell**: 🐛 Preserve thinking placeholder during active runs *(commit by [@jorben](https://github.com/jorben))*
- [`c30cc93`](https://github.com/TiyAgents/tiycode/commit/c30cc93abadd63bb73920dd56e4866a3f0e45f7a) - **workbench-shell**: 🐛 prioritize workspace threads over fallback threads *(commit by [@jorben](https://github.com/jorben))*
- [`3ef0df1`](https://github.com/TiyAgents/tiycode/commit/3ef0df1350324309c7644aba60cae7de0bb97620) - **workbench-shell**: 🐛 ensure threads render with fallback title *(commit by [@jorben](https://github.com/jorben))*
- [`a031144`](https://github.com/TiyAgents/tiycode/commit/a031144178db283998adf96c371190d421976a03) - **index**: 🐛 resolve ripgrep paths relative to workspace root *(commit by [@jorben](https://github.com/jorben))*
- [`5fa6c6d`](https://github.com/TiyAgents/tiycode/commit/5fa6c6dcd02771348b6ea940f9802285638e9145) - **workbench-shell**: 🐛 prevent initial prompt mismatch between threads *(commit by [@jorben](https://github.com/jorben))*
- [`d6326be`](https://github.com/TiyAgents/tiycode/commit/d6326be67b501f2a3075d1d3177a8ee2ca28d63d) - **workspace-repo**: 🐛 Correct delete order and extend cleanup coverage *(commit by [@jorben](https://github.com/jorben))*
- [`66eb713`](https://github.com/TiyAgents/tiycode/commit/66eb7139a50dd947ebeef9c50a48ca3d1f24b2c3) - **ai**: 🐛 support native Tauri drag-and-drop for file uploads *(commit by [@jorben](https://github.com/jorben))*
- [`2c31618`](https://github.com/TiyAgents/tiycode/commit/2c316187f58671950d8a530293fa4a9528463826) - **task-board**: 🐛 treat empty advance step id as missing *(commit by [@jorben](https://github.com/jorben))*
- [`dbf8582`](https://github.com/TiyAgents/tiycode/commit/dbf8582ac443b1973f309db468a5d1c764716ca4) - **policy**: 🐛 Improve policy pattern matching *(commit by [@jorben](https://github.com/jorben))*
- [`cdbe919`](https://github.com/TiyAgents/tiycode/commit/cdbe919b5f1f0021d766253fb0b1a8401571e210) - **search**: 🐛 Treat queries as literal text *(commit by [@jorben](https://github.com/jorben))*
- [`5065eb6`](https://github.com/TiyAgents/tiycode/commit/5065eb61082a4177e1454e2a7ce5c08e920bf3ad) - **release**: 🐛 rename release artifacts for all platforms *(commit by [@jorben](https://github.com/jorben))*

### :recycle: Refactors
- [`0adbfe8`](https://github.com/TiyAgents/tiycode/commit/0adbfe879c7a8f2b736c5edf2048d502910d0a55) - **workbench**: ♻️ refine visual hierarchy *(commit by [@jorben](https://github.com/jorben))*
- [`257b982`](https://github.com/TiyAgents/tiycode/commit/257b98268685e4c34457df493e5edfffaee5d498) - **workbench**: ♻️ split dashboard and settings into modules *(commit by [@jorben](https://github.com/jorben))*
- [`0d7a64d`](https://github.com/TiyAgents/tiycode/commit/0d7a64dd0338dc6184ab02a53ab4fb4b8fd56a45) - **workbench**: ♻️ remove legacy compatibility wrappers *(commit by [@jorben](https://github.com/jorben))*
- [`41de79a`](https://github.com/TiyAgents/tiycode/commit/41de79aaea7c6badb5632b1b2ef47a70375f5e2e) - **agent**: ♻️ replace sidecar with built-in runtime *(commit by [@jorben](https://github.com/jorben))*
- [`da1d3e1`](https://github.com/TiyAgents/tiycode/commit/da1d3e12ac6f272aac67f2673ffac27ac34263ab) - **tools**: 🛡️ rename runtime tool surface to shell/grep/term_* *(commit by [@jorben](https://github.com/jorben))*
- [`aed0a05`](https://github.com/TiyAgents/tiycode/commit/aed0a0508743be865694ce9497824d6fd52c09dc) - **core**: 🔎 switch grep tool to search *(commit by [@jorben](https://github.com/jorben))*
- [`31c55e6`](https://github.com/TiyAgents/tiycode/commit/31c55e686d905e2ab313d3f78177e81d4db853bb) - **agent**: ✨ improve delegation guidance for helper agents *(commit by [@jorben](https://github.com/jorben))*
- [`8acd7aa`](https://github.com/TiyAgents/tiycode/commit/8acd7aad33d2fb6d60203f98539ba86e635c0f6b) - **workbench-shell**: ♻️ simplify runtime timeline tool rendering *(commit by [@jorben](https://github.com/jorben))*
- [`082638b`](https://github.com/TiyAgents/tiycode/commit/082638b68441f35623e17cabaa4a96fb9a1febf6) - **workbench-shell**: ♻️ reuse tool detail code sections *(commit by [@jorben](https://github.com/jorben))*
- [`4380bee`](https://github.com/TiyAgents/tiycode/commit/4380bee8dfc23d87ba1d5a5319593d069886d159) - **ui/workbench-shell**: ♻️ reset commit message via helper *(commit by [@jorben](https://github.com/jorben))*
- [`fe80938`](https://github.com/TiyAgents/tiycode/commit/fe80938e6975976c17b989d682895c6cde95f2cf) - **terminal**: 🧹 sanitize recent terminal output for agent *(commit by [@jorben](https://github.com/jorben))*
- [`db52524`](https://github.com/TiyAgents/tiycode/commit/db52524af0ead55dbd1178260a5bf269f768a003) - **ui**: ♻️ allow CodeBlock content class overrides *(commit by [@jorben](https://github.com/jorben))*
- [`b7f343a`](https://github.com/TiyAgents/tiycode/commit/b7f343a01fe42427d76ca244dade4cb616be2355) - **git-overlay**: 🗂️ cache workspace overlays with TTL and avoid deep ignored walks *(commit by [@jorben](https://github.com/jorben))*
- [`42ac67e`](https://github.com/TiyAgents/tiycode/commit/42ac67e9bca46596e76ad869f8490e64c99e80de) - **agent-session**: ✨ strengthen response style instructions and UI descriptions *(commit by [@jorben](https://github.com/jorben))*
- [`5d25e3e`](https://github.com/TiyAgents/tiycode/commit/5d25e3ecfbf1cafafc12177bc8f645b8c6e8db1a) - **agent_session**: ✨ add final response structure instruction *(commit by [@jorben](https://github.com/jorben))*
- [`547b688`](https://github.com/TiyAgents/tiycode/commit/547b6880dc75711746dcf580c8dba48882f14064) - **core**: 🛠️ render plan checkpoints from thread history *(commit by [@jorben](https://github.com/jorben))*
- [`76164a1`](https://github.com/TiyAgents/tiycode/commit/76164a1a319dbab866ff1f2f1b213a500c9de3cc) - **agent**: ♻️ persist context reset and compact summaries *(commit by [@jorben](https://github.com/jorben))*
- [`7a693e5`](https://github.com/TiyAgents/tiycode/commit/7a693e55bba54bcb92f44f056a40a1338ac2f77a) - **workbench**: 🎨 improve command panel conditional rendering and controlled behavior *(commit by [@jorben](https://github.com/jorben))*
- [`2349fd0`](https://github.com/TiyAgents/tiycode/commit/2349fd039470fb4aa10655902f660590617440a3) - **prompt**: ✨ improve response style instructions *(commit by [@jorben](https://github.com/jorben))*
- [`2011e1d`](https://github.com/TiyAgents/tiycode/commit/2011e1d2ba772dacdce5a54a193cef0059c4d35f) - **auth**: ♻️ expand clarify tool guidance and update prompts *(commit by [@jorben](https://github.com/jorben))*
- [`d68cca7`](https://github.com/TiyAgents/tiycode/commit/d68cca75ec819f548f583c3541e0a51e2fdbb371) - **prompt**: ♻️ modularize system prompt construction *(commit by [@jorben](https://github.com/jorben))*
- [`b04303c`](https://github.com/TiyAgents/tiycode/commit/b04303c2b2345a5c0b8c5704148521cfcc3ab1f3) - **prompt**: ♻️ clarify shell vs workspace tool guidance *(commit by [@jorben](https://github.com/jorben))*
- [`170ed98`](https://github.com/TiyAgents/tiycode/commit/170ed98bd2860b5425847a261b072a465e4f642e) - **prompt**: ♻️ Adjust run-mode clarification and plan guidance *(commit by [@jorben](https://github.com/jorben))*
- [`ed6857a`](https://github.com/TiyAgents/tiycode/commit/ed6857a9f5cc12b536e33fbc4765ed0bf447a839) - **orchestrator**: ♻️ inherit only approved helper prompt sections *(commit by [@jorben](https://github.com/jorben))*
- [`ceb85ac`](https://github.com/TiyAgents/tiycode/commit/ceb85ac4fdd5d5b5ed59a5a2c2254e71d778e33e) - **thread titles**: ✨ skip title generation when thread already has a title *(commit by [@jorben](https://github.com/jorben))*
- [`0a002f2`](https://github.com/TiyAgents/tiycode/commit/0a002f275cfcfa4b7b86bd50c58a06a58d82ee37) - **workbench-shell**: ✨ extract file mutation presentation and share diff counting *(commit by [@jorben](https://github.com/jorben))*
- [`3747e32`](https://github.com/TiyAgents/tiycode/commit/3747e32db5a03df669562cccedf2f64a4d2c5d1f) - **settings**: ♻️ rename agent settings labels and profile icon *(commit by [@jorben](https://github.com/jorben))*
- [`6ffd32b`](https://github.com/TiyAgents/tiycode/commit/6ffd32b8f0ee315695e99ee43b3b28d066c7ba3f) - **dashboard-workbench**: ♻️ manage pending thread runs per thread id *(commit by [@jorben](https://github.com/jorben))*
- [`57e2c05`](https://github.com/TiyAgents/tiycode/commit/57e2c052639e3d0e39eb4096f864ca7beb25d267) - 🧹 remove subagent usage update events *(commit by [@jorben](https://github.com/jorben))*
- [`a842e80`](https://github.com/TiyAgents/tiycode/commit/a842e80614b02420271ebc79bc7f7c050f5a37fd) - **settings-center**: ♻️ sort agent profiles by provider and display name *(commit by [@jorben](https://github.com/jorben))*
- [`5343eb6`](https://github.com/TiyAgents/tiycode/commit/5343eb6826d7bff34207b7d7c5298372fe1892d6) - **extensions**: ♻️ simplify skills/extension UI and remove activity & pin support *(commit by [@jorben](https://github.com/jorben))*
- [`4aa6c72`](https://github.com/TiyAgents/tiycode/commit/4aa6c72b7ea4749879b20f073172c7cc0cef6493) - **task-board**: ♻️ improve task board reconciliation and step terminal handling *(commit by [@jorben](https://github.com/jorben))*
- [`a34987a`](https://github.com/TiyAgents/tiycode/commit/a34987a6ae74513e039e7960e8d7f185b2e19def) - **core/prompt**: 📝 refine SKILL.md loading guidance *(commit by [@jorben](https://github.com/jorben))*
- [`04ee378`](https://github.com/TiyAgents/tiycode/commit/04ee3780b4a7fd64c050ec3249fb0dbcd48f060b) - **workbench-shell**: ♻️ simplify runtime effect dependencies *(commit by [@jorben](https://github.com/jorben))*
- [`fc7a02b`](https://github.com/TiyAgents/tiycode/commit/fc7a02b8214432470d03eb43f08524096a31eb8b) - **settings-center**: ♻️ remove sandbox and network access policies *(commit by [@jorben](https://github.com/jorben))*
- [`56c63f7`](https://github.com/TiyAgents/tiycode/commit/56c63f7e4cca7568b61a5c3b2ddfee8b21578bd8) - **code-block**: ♻️ make code highlighting theme-aware *(commit by [@jorben](https://github.com/jorben))*

### :white_check_mark: Tests
- [`dba9049`](https://github.com/TiyAgents/tiycode/commit/dba90494f5cd612c31640a3f7a55707f5d734f74) - **m1**: ✅ add 103 integration tests for Phase 1 verification *(commit by [@jorben](https://github.com/jorben))*
- [`7746b25`](https://github.com/TiyAgents/tiycode/commit/7746b25c8e8b651a2adaedfbb959c3a96fec2ac6) - **index**: ✅ ensure search honors global max results *(commit by [@jorben](https://github.com/jorben))*
- [`04aac8b`](https://github.com/TiyAgents/tiycode/commit/04aac8bc4f7f3a2fbd3561271ae54dafb5e55e55) - ✅ update escalation reason assertion *(commit by [@jorben](https://github.com/jorben))*

### :wrench: Chores
- [`9e55887`](https://github.com/TiyAgents/tiycode/commit/9e5588775c8ad4b930e4360824df788e16b3e707) - **scripts**: 🔧 simplify tauri script names *(commit by [@jorben](https://github.com/jorben))*
- [`7595641`](https://github.com/TiyAgents/tiycode/commit/759564184b7e1875573c3e899b38702afae2d509) - **db**: ✨ add commit message prompt and language to agent_profiles *(commit by [@jorben](https://github.com/jorben))*
- [`e50652a`](https://github.com/TiyAgents/tiycode/commit/e50652a44418fd3b366f2b025c3d43a13c6a7e3e) - 🔧 make commit message language configurable at runtime *(commit by [@jorben](https://github.com/jorben))*
- [`3e7cc23`](https://github.com/TiyAgents/tiycode/commit/3e7cc236eb08df80bb1b9046b2a32c915df6f3f7) - 🔧 update commit prompt formatting and defaults *(commit by [@jorben](https://github.com/jorben))*
- [`23b75f6`](https://github.com/TiyAgents/tiycode/commit/23b75f64c68c0542e2f9e78f54508f21990f2ab2) - **commands**: 🔧 update git commit message prompt formatting *(commit by [@jorben](https://github.com/jorben))*
- [`7eec8e9`](https://github.com/TiyAgents/tiycode/commit/7eec8e9e8b1e897976acd710dc2fb67bcce6ecf9) - **agent-session**: ✍️ update project context snippet formatting *(commit by [@jorben](https://github.com/jorben))*
- [`35a4b9b`](https://github.com/TiyAgents/tiycode/commit/35a4b9b7368f662536a7e2590a3b9657c9e4e723) - **agent_review**: 🧪 delegate post-implementation verification to review helper *(commit by [@jorben](https://github.com/jorben))*
- [`9e6292c`](https://github.com/TiyAgents/tiycode/commit/9e6292cb35893049cb6b3f39ae1b51f77b44b2cc) - 🎨 refine tool-use protocol in runtime orchestration guidelines *(commit by [@jorben](https://github.com/jorben))*
- [`cd629e4`](https://github.com/TiyAgents/tiycode/commit/cd629e4871a868684f47eabdc548ebbe06933270) - **ui**: ♻️ switch app icon to icon.svg *(commit by [@jorben](https://github.com/jorben))*
- [`660c4f2`](https://github.com/TiyAgents/tiycode/commit/660c4f2fccf51f70d703e5343bd904ceec3c64ed) - **icons**: ✨ update Tiy Agent favicon and app icons *(commit by [@jorben](https://github.com/jorben))*
- [`af298d4`](https://github.com/TiyAgents/tiycode/commit/af298d48fa445e8b07c6ffa5b197e5a985615308) - **settings-center**: ✨ update profile picker label to Current Profile *(commit by [@jorben](https://github.com/jorben))*
- [`e946526`](https://github.com/TiyAgents/tiycode/commit/e946526062acfbbea19724372abbb0e03f6a7348) - **icons**: ♻️ update app icon assets and remove legacy SVG *(commit by [@jorben](https://github.com/jorben))*
- [`b084510`](https://github.com/TiyAgents/tiycode/commit/b084510fdff368d37c20c77f453502dd5d86c1ce) - 🧹 remove unused workspace update_name_and_paths function *(commit by [@jorben](https://github.com/jorben))*
- [`8d1c8ba`](https://github.com/TiyAgents/tiycode/commit/8d1c8ba11d1c77d4560fa7d49896e04d4e7238b0) - **icons**: ♻️ update application icon assets *(commit by [@jorben](https://github.com/jorben))*
- [`fbc0d1f`](https://github.com/TiyAgents/tiycode/commit/fbc0d1f9b03114e1cdd1c292dd7b59ad1123e15e) - ✨ rename Tiy Agent to TiyCode *(commit by [@jorben](https://github.com/jorben))*
- [`7ce7fcb`](https://github.com/TiyAgents/tiycode/commit/7ce7fcb2f0cadd8e8c2e9f7f80da4d96085d5ef8) - ♻️ switch app branding to tiycode *(commit by [@jorben](https://github.com/jorben))*
- [`6d88416`](https://github.com/TiyAgents/tiycode/commit/6d88416f0b3dc74d586304aa594447d530123752) - **tauri**: 🔧 bump tiycore and Rust dependencies *(commit by [@jorben](https://github.com/jorben))*
- [`66b5083`](https://github.com/TiyAgents/tiycode/commit/66b508314c02c1fb5b180275cd13a53d2bf3b891) - **deps**: 🔧 bump @tauri-apps/plugin-dialog to 2.7.0 *(commit by [@jorben](https://github.com/jorben))*
- [`ec6682d`](https://github.com/TiyAgents/tiycode/commit/ec6682da54c89548996bfd64c44403587c05509b) - **ci**: ✨ add release, CI, and changelog workflows *(commit by [@jorben](https://github.com/jorben))*
- [`aa80657`](https://github.com/TiyAgents/tiycode/commit/aa8065763429e70e6b0a30fb57521ae267902986) - **workbench-shell**: 🧹 remove login action from top bar *(commit by [@jorben](https://github.com/jorben))*
- [`45b946a`](https://github.com/TiyAgents/tiycode/commit/45b946a747bd2ae771c8bd512dea79aa4c1db8ab) - **docs**: remove extensions-source-removal-design spec *(commit by [@jorben](https://github.com/jorben))*
- [`ca97a42`](https://github.com/TiyAgents/tiycode/commit/ca97a4225070594b5b92d996a6e0aa8320b08753) - **license**: ♻️ update license to Apache 2.0 *(commit by [@jorben](https://github.com/jorben))*
- [`6b00b65`](https://github.com/TiyAgents/tiycode/commit/6b00b65b06b1284c5ce17e3d1ba4c0218b92db8e) - **db**: 🔧 seed latest default settings *(commit by [@jorben](https://github.com/jorben))*

[0.1.1]: https://github.com/TiyAgents/tiycode/compare/0.0.1...0.1.1
[0.1.2]: https://github.com/TiyAgents/tiycode/compare/0.1.1...0.1.2
[0.1.3]: https://github.com/TiyAgents/tiycode/compare/0.1.2...0.1.3
[0.1.4]: https://github.com/TiyAgents/tiycode/compare/0.1.3...0.1.4
[0.1.5]: https://github.com/TiyAgents/tiycode/compare/0.1.4...0.1.5
[0.1.6]: https://github.com/TiyAgents/tiycode/compare/0.1.5...0.1.6
[0.1.8]: https://github.com/TiyAgents/tiycode/compare/0.1.7...0.1.8
[0.1.11]: https://github.com/TiyAgents/tiycode/compare/0.1.10...0.1.11
[0.1.13]: https://github.com/TiyAgents/tiycode/compare/0.1.12...0.1.13
[0.1.14]: https://github.com/TiyAgents/tiycode/compare/0.1.13...0.1.14
[0.1.16]: https://github.com/TiyAgents/tiycode/compare/0.1.15...0.1.16
[0.1.17]: https://github.com/TiyAgents/tiycode/compare/0.1.16...0.1.17
[0.1.19]: https://github.com/TiyAgents/tiycode/compare/0.1.18...0.1.19
[0.2.0]: https://github.com/TiyAgents/tiycode/compare/0.1.19...0.2.0
[0.2.1]: https://github.com/TiyAgents/tiycode/compare/0.2.0...0.2.1
[0.2.2]: https://github.com/TiyAgents/tiycode/compare/0.2.1...0.2.2
[0.2.3]: https://github.com/TiyAgents/tiycode/compare/0.2.2...0.2.3
[0.2.4]: https://github.com/TiyAgents/tiycode/compare/0.2.3...0.2.4
[0.2.5]: https://github.com/TiyAgents/tiycode/compare/0.2.4...0.2.5
[0.2.6]: https://github.com/TiyAgents/tiycode/compare/0.2.5...0.2.6
[0.2.7]: https://github.com/TiyAgents/tiycode/compare/0.2.6...0.2.7
[0.2.8]: https://github.com/TiyAgents/tiycode/compare/0.2.7...0.2.8
[0.2.9]: https://github.com/TiyAgents/tiycode/compare/0.2.8...0.2.9
[0.3.0]: https://github.com/TiyAgents/tiycode/compare/0.2.9...0.3.0
[0.3.1]: https://github.com/TiyAgents/tiycode/compare/0.3.0...0.3.1
[0.3.2]: https://github.com/TiyAgents/tiycode/compare/0.3.1...0.3.2
[0.3.3]: https://github.com/TiyAgents/tiycode/compare/0.3.2...0.3.3
[0.3.4]: https://github.com/TiyAgents/tiycode/compare/0.3.3...0.3.4
[0.3.5]: https://github.com/TiyAgents/tiycode/compare/0.3.4...0.3.5
[0.3.6]: https://github.com/TiyAgents/tiycode/compare/0.3.5...0.3.6
[0.3.7]: https://github.com/TiyAgents/tiycode/compare/0.3.6...0.3.7
[0.3.8]: https://github.com/TiyAgents/tiycode/compare/0.3.7...0.3.8
