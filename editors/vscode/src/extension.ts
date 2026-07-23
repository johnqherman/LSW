import * as vscode from "vscode";
import { execFile } from "child_process";
import * as fs from "fs";
import * as path from "path";

interface IdeEnv {
  target?: string;
  compiler?: string;
  includePaths?: string[];
  defines?: string[];
}

function intelliSenseMode(target?: string): string {
  const t = target ?? "";
  let arch = "x64";
  if (t.startsWith("aarch64") || t.startsWith("arm64")) {
    arch = "arm64";
  } else if (t.startsWith("armv7") || t.startsWith("arm")) {
    arch = "arm";
  } else if (t.startsWith("i686") || t.startsWith("i386")) {
    arch = "x86";
  }
  return `windows-clang-${arch}`;
}

function lswPath(): string {
  return vscode.workspace.getConfiguration("lsw").get<string>("path", "lsw");
}

function shQuote(s: string): string {
  return "'" + s.replace(/'/g, "'\\''") + "'";
}

function runInTerminal(args: string[]): void {
  const terminal = vscode.window.createTerminal("LSW");
  terminal.show();
  terminal.sendText([lswPath(), ...args].map(shQuote).join(" "));
}

function configureIntelliSense(): void {
  const folders = vscode.workspace.workspaceFolders;
  if (!folders || folders.length === 0) {
    vscode.window.showErrorMessage("LSW: open a folder containing lsw.toml first");
    return;
  }
  const root = folders[0].uri.fsPath;
  execFile(lswPath(), ["--format", "json", "ide", "env"], { cwd: root }, (err, stdout) => {
    if (err) {
      vscode.window.showErrorMessage("lsw ide env failed: " + err.message);
      return;
    }
    let env: IdeEnv;
    try {
      env = JSON.parse(stdout) as IdeEnv;
    } catch {
      vscode.window.showErrorMessage("could not parse lsw ide env output");
      return;
    }
    const config = {
      version: 4,
      configurations: [
        {
          name: "LSW",
          compilerPath: env.compiler ?? "",
          includePath: (env.includePaths ?? []).concat(["${workspaceFolder}/**"]),
          defines: env.defines ?? [],
          intelliSenseMode: intelliSenseMode(env.target)
        }
      ]
    };
    const dir = path.join(root, ".vscode");
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, "c_cpp_properties.json"), JSON.stringify(config, null, 2));
    vscode.window.showInformationMessage("LSW: wrote .vscode/c_cpp_properties.json");
  });
}

export function activate(context: vscode.ExtensionContext): void {
  const register = (id: string, handler: (...args: unknown[]) => unknown): void => {
    context.subscriptions.push(vscode.commands.registerCommand(id, handler));
  };
  register("lsw.build", () => runInTerminal(["build"]));
  register("lsw.test", () => runInTerminal(["test"]));
  register("lsw.verify", () => runInTerminal(["verify", "--native-windows"]));
  register("lsw.run", async () => {
    const program = await vscode.window.showInputBox({
      prompt: "Program to run",
      value: "build/app.exe"
    });
    if (program) {
      runInTerminal(["run", program]);
    }
  });
  register("lsw.configureIntelliSense", configureIntelliSense);

  context.subscriptions.push(
    vscode.debug.registerDebugAdapterDescriptorFactory("lsw", {
      createDebugAdapterDescriptor: () => new vscode.DebugAdapterExecutable(lswPath(), ["dap"])
    })
  );
}

export function deactivate(): void {}
