/** Thin VS Code integration backed by the Token-Shrinker SDK. */
import * as vscode from "vscode";
import { TokenShrinkerClient } from "@token-shrinker/sdk";
import {
  authorizeWorkspaceOperation,
  renderStructuredResult,
  statusLabel,
  type WorkspaceOperation,
} from "./core.js";

let client: TokenShrinkerClient | undefined;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 10);
  status.command = "tokenShrinker.showStatus";
  status.text = "Token-Shrinker: starting";
  status.show();
  context.subscriptions.push(status);

  context.subscriptions.push(
    vscode.commands.registerCommand("tokenShrinker.showStatus", async () => {
      await guarded("status", async (activeClient) => {
        const capabilities = await activeClient.capabilities({ timeoutMs: 5_000 });
        status.text = statusLabel(capabilities);
        await showJson("Token-Shrinker Status", capabilities);
      });
    }),
    vscode.commands.registerCommand("tokenShrinker.buildContext", async () => {
      await guarded("build-context", async (activeClient) => {
        const folder = vscode.workspace.workspaceFolders?.[0];
        if (!folder) {
          await vscode.window.showWarningMessage("Open a workspace before building context.");
          return;
        }
        const goal = await vscode.window.showInputBox({
          title: "Token-Shrinker goal",
          prompt: "What should the agent investigate or build?",
          ignoreFocusOut: true,
        });
        if (!goal?.trim()) return;
        const budget = vscode.workspace
          .getConfiguration("tokenShrinker", folder.uri)
          .get<number>("contextBudget", 16_000);
        const result = await activeClient.buildContext({
          root: folder.uri.fsPath,
          goal: goal.trim(),
          budget,
        });
        await showJson("Token-Shrinker Context", result);
      });
    }),
    vscode.commands.registerCommand("tokenShrinker.showStats", async () => {
      await guarded("stats", async (activeClient) => {
        const result = await activeClient.transport.call<Record<string, unknown>>(
          "token_shrinker_stats",
          {},
          { timeoutMs: 5_000 },
        );
        await showJson("Token-Shrinker Statistics", result);
      });
    }),
  );

  try {
    const capabilities = await getClient().then((value) => value.capabilities({ timeoutMs: 2_000 }));
    status.text = statusLabel(capabilities);
  } catch {
    status.text = "Token-Shrinker: unavailable";
  }
}

export async function deactivate(): Promise<void> {
  const active = client;
  client = undefined;
  await active?.close();
}

async function guarded(
  operation: WorkspaceOperation,
  action: (activeClient: TokenShrinkerClient) => Promise<void>,
): Promise<void> {
  const decision = authorizeWorkspaceOperation(operation, vscode.workspace.isTrusted);
  if (!decision.allowed) {
    await vscode.window.showWarningMessage(
      "Token-Shrinker will not read or execute workspace content until this workspace is trusted.",
    );
    return;
  }
  try {
    await action(await getClient());
  } catch (error) {
    const message = error instanceof Error ? error.message : "Unknown Token-Shrinker error";
    await vscode.window.showErrorMessage(message);
  }
}

async function getClient(): Promise<TokenShrinkerClient> {
  if (client) return client;
  const binaryPath = vscode.workspace
    .getConfiguration("tokenShrinker")
    .get<string>("binaryPath", "token-shrinker");
  client = await TokenShrinkerClient.connect({ transport: "stdio", binaryPath });
  return client;
}

async function showJson(title: string, value: unknown): Promise<void> {
  const document = await vscode.workspace.openTextDocument({
    language: "json",
    content: renderStructuredResult(value),
  });
  await vscode.window.showTextDocument(document, { preview: true });
  await vscode.window.showInformationMessage(`${title} ready`);
}
