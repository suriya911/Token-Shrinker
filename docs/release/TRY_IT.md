# Try Token-Shrinker before publication

These steps exercise the same surfaces users will receive without depending on a public registry.

## 1. Build and verify the release binary

From the repository root:

```powershell
pnpm install --frozen-lockfile
cargo build --release --bin token-shrinker
pnpm --filter @token-shrinker/cli test
node scripts/verify-native-package.mjs --profile release
```

The last command creates temporary platform and umbrella npm tarballs, installs them offline in a clean directory, selects the native executable, checks versions, installs and validates a Codex adapter, removes it, uninstalls npm packages, and deletes the temporary directory.

## 2. Try the native CLI

```powershell
.\target\release\token-shrinker.exe version --json
.\target\release\token-shrinker.exe doctor --json
.\target\release\token-shrinker.exe benchmark demo --json
```

The demo must retain the required answer and citations while meeting the checked-in reduction threshold.

## 3. Try an agent integration safely

Preview first. Use an absolute binary path:

```powershell
node packages/cli/dist/index.js add codex --root . --binary E:\Token-Shrinker\target\release\token-shrinker.exe --dry-run --json
node packages/cli/dist/index.js add codex --root . --binary E:\Token-Shrinker\target\release\token-shrinker.exe --json
```

Restart Codex, open this trusted project, and inspect its MCP servers. The project files are `.codex/config.toml` and `.agents/skills/token-shrinker/SKILL.md`. Then ask the agent to call `token_shrinker_capabilities` and `token_shrinker_build_context` for a repository question.

Claude Code uses the same flow with `claude-code`; generated files are `.mcp.json` and `.claude/skills/token-shrinker/SKILL.md`. Supported names are `codex`, `claude-code`, `gemini`, `opencode`, and `aider`.

Remove only Token-Shrinker's owned fragments:

```powershell
node packages/cli/dist/index.js remove codex --root . --binary E:\Token-Shrinker\target\release\token-shrinker.exe --json
```

The adapter never changes `OPENAI_BASE_URL`, `ANTHROPIC_BASE_URL`, credentials, or native model transport.

## 4. Try the VS Code extension

```powershell
pnpm --filter ./packages/vscode test
pnpm --filter ./packages/vscode package
code --install-extension packages/vscode/dist/token-shrinker.vsix --force
```

In VS Code settings, set `Token Shrinker: Binary Path` to the absolute release executable. Reload the window, trust the test workspace, and run these commands from the Command Palette:

- `Token-Shrinker: Show Status`
- `Token-Shrinker: Build Context`
- `Token-Shrinker: Show Statistics`

Repeat in an untrusted temporary workspace: status may work, while repository-reading commands must remain blocked. Remove the local build with:

```powershell
code --uninstall-extension token-shrinker.token-shrinker
```

## 5. Verify public artifacts after publication

Use a clean machine or VM, not the repository checkout:

```powershell
npm install --global @token-shrinker/cli@next
token-shrinker version --json
token-shrinker doctor --json
token-shrinker add codex --root C:\path\to\test-project --dry-run --json
```

Confirm npm provenance, GitHub checksums/attestations, Marketplace publisher identity, clean uninstall, and that no compiler or postinstall download was required.
