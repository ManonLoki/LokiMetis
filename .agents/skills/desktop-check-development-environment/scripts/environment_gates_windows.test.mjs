import assert from "node:assert/strict";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { pathToFileURL } from "node:url";

import { WINDOWS_SCRIPT } from "./environment_gates_fixture.mjs";

function locatePowerShell() {
  if (process.platform !== "win32") return null;
  for (const name of ["powershell.exe", "pwsh.exe"]) {
    const result = spawnSync("where.exe", [name], { encoding: "utf8" });
    if (result.status === 0) return result.stdout.split(/\r?\n/u).find(Boolean) ?? name;
  }
  return null;
}

const powershell = locatePowerShell();
const windowsRuntime = { skip: process.platform !== "win32" || !powershell };

function command(target, body) {
  mkdirSync(path.dirname(target), { recursive: true });
  writeFileSync(target, `@echo off\r\n${body}\r\n`);
  chmodSync(target, 0o755);
}

function addBaseTools(probe, { git = "2.50.0", rust = "1.98.1", node = "24.21.0", pnpm = null } = {}) {
  command(path.join(probe, "cl.cmd"), "exit /b 0");
  command(path.join(probe, "git.cmd"), `echo git version ${git}`);
  command(path.join(probe, "rustc.cmd"), `if "%1"=="-vV" (echo rustc ${rust} ^(test^) & echo host: x86_64-pc-windows-msvc & echo release: ${rust} & exit /b 0)\r\necho rustc ${rust} (test)`);
  command(path.join(probe, "cargo.cmd"), `echo cargo ${rust} (test)`);
  command(path.join(probe, "rustup.cmd"), "echo rustup 1.28.2 (test)");
  command(path.join(probe, "node.cmd"), `echo v${node}`);
  command(path.join(probe, "npm.cmd"), "echo 11.6.0");
  if (pnpm !== null) command(path.join(probe, "pnpm.cmd"), `echo ${pnpm}`);
}

function runWindowsGate(root, probe, args, { testMode = true, extra = {} } = {}) {
  const home = path.join(root, "home");
  mkdirSync(home, { recursive: true });
  const env = {
    ...process.env,
    AFH_TEST_USER_PROFILE_ROOT: home,
    AFH_TEST_LOCAL_APPDATA_ROOT: path.join(root, "local-app-data"),
    AFH_TEST_ROAMING_APPDATA_ROOT: path.join(root, "roaming-app-data"),
    AFH_PREREQ_PATH: probe,
    AFH_NODE_HOME: path.join(root, "node-home"),
    AFH_PNPM_HOME: path.join(root, "pnpm-home"),
    AFH_SKIP_PERSIST_PATH: "1",
    AFH_TEST_MODE: "1",
    AFH_TEST_MACHINE_PATH: probe,
    CARGO_HOME: path.join(home, "custom-cargo"),
    RUSTUP_HOME: path.join(home, "custom-rustup"),
    AFH_TEST_USER_CARGO_HOME: path.join(home, "custom-cargo"),
    AFH_TEST_USER_RUSTUP_HOME: path.join(home, "custom-rustup"),
    USERPROFILE: home,
    ...extra,
  };
  if (!testMode) delete env.AFH_TEST_MODE;
  return spawnSync(powershell, [
    "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", WINDOWS_SCRIPT, ...args,
  ], {
    cwd: root,
    encoding: "utf8",
    timeout: 30_000,
    env,
  });
}

function withTemporaryRoot(callback) {
  const root = mkdtempSync(path.join(tmpdir(), "afh-env-windows-"));
  try { callback(root); } finally { rmSync(root, { recursive: true, force: true }); }
}

function makeFileSymlink(link, target) {
  mkdirSync(path.dirname(link), { recursive: true });
  return spawnSync("cmd.exe", ["/c", "mklink", link, target], { encoding: "utf8" }).status === 0;
}

function makeJunction(link, target) {
  mkdirSync(path.dirname(link), { recursive: true });
  return spawnSync("cmd.exe", ["/c", "mklink", "/J", link, target], { encoding: "utf8" }).status === 0;
}

function makeWindowsNodeMetadata(root, { digest = "1".repeat(64) } = {}) {
  const version = "v24.21.1";
  const archiveName = `node-${version}-win-x64.zip`;
  const release = path.join(root, version);
  mkdirSync(release, { recursive: true });
  writeFileSync(path.join(root, "index.json"), JSON.stringify([
    { version: "v26.8.2", lts: false },
    { version, lts: "Krypton" },
  ]));
  writeFileSync(path.join(release, "SHASUMS256.txt"), `${digest}  ${archiveName}\n`);
  return pathToFileURL(root).href;
}

test("PowerShell gate declares Node as common runtime and pnpm for both GUI frameworks", () => {
  const source = readFileSync(WINDOWS_SCRIPT, "utf8");
  assert.match(source, /\$PnpmRequired = \(\$NormalizedInterfaces -contains "GUI"\)\r?\n/u);
  assert.ok(source.includes('[ValidateSet("tauri", "gpui")]'));
  assert.ok(source.includes('$node = Resolve-GateCommand "node"'));
  assert.ok(source.includes('"gate.node.status=passed"'));
  assert.ok(source.includes('"gate.pnpm.status=$(if ($PnpmRequired) { \'passed\' } else { \'not-required\' })"'));
  assert.equal(source.includes("$FrontendRequired"), false);
});

test("PowerShell persists PATH with a Windows environment-change broadcast and fresh-session probe", () => {
  const source = readFileSync(WINDOWS_SCRIPT, "utf8");
  for (const fragment of [
    "SendMessageTimeout",
    '"Environment"',
    "0x001A",
    "function Test-FreshPowerShellToolDiscovery",
    '[Environment]::GetEnvironmentVariable("Path", "Machine")',
    'foreach (`$pathValue in @(`$machinePath, `$userPath))',
    "-EncodedCommand",
    "-not [IO.Path]::IsPathRooted",
  ]) assert.ok(source.includes(fragment), fragment);
  assert.equal(source.includes("[string[]]$OwnedRoots"), false);
});

test("Windows check-only reports an old Git without writes", windowsRuntime, () => withTemporaryRoot((root) => {
  const probe = path.join(root, "probe");
  addBaseTools(probe, { git: "2.35.8" });
  command(path.join(probe, "winget.cmd"), 'type nul > "%~dp0winget-called"');
  const result = runWindowsGate(root, probe, ["-CheckOnly", "-Interfaces", "CLI"]);
  assert.equal(result.status, 20, result.stderr);
  assert.match(result.stdout, /gate\.git\.status=upgrade-required/);
  assert.match(result.stdout, /gate\.node\.status=passed/);
  assert.match(result.stdout, /gate\.pnpm\.status=not-required/);
}));

test("Windows Node minimum is continuous for every interface", windowsRuntime, () => {
  for (const [node, status, state] of [
    ["24.20.9", 20, "upgrade-required"],
    ["24.21.0", 0, "passed"],
    ["25.9.0", 0, "passed"],
    ["26.7.0", 0, "passed"],
  ]) withTemporaryRoot((root) => {
    const probe = path.join(root, "probe");
    addBaseTools(probe, { node });
    const result = runWindowsGate(root, probe, ["-CheckOnly", "-Interfaces", "CLI"]);
    assert.equal(result.status, status, `${node}: ${result.stderr}`);
    assert.ok(result.stdout.includes(`gate.node.status=${state}`));
  });
});

test("Windows pnpm is required only for GUI", windowsRuntime, () => withTemporaryRoot((root) => {
  const probe = path.join(root, "probe");
  addBaseTools(probe);
  const cli = runWindowsGate(root, probe, ["-CheckOnly", "-Interfaces", "CLI"]);
  assert.equal(cli.status, 0, cli.stderr);
  assert.match(cli.stdout, /gate\.pnpm\.status=not-required/);
  const gui = runWindowsGate(root, probe, ["-CheckOnly", "-Interfaces", "GUI"]);
  assert.equal(gui.status, 20, gui.stderr);
  assert.match(gui.stdout, /gate\.pnpm\.status=missing/);
}));

test("Windows GPUI requires pnpm without probing Tauri dependencies", windowsRuntime, () => {
  for (const [pnpm, status, state] of [
    [null, 20, "missing"],
    ["12.4.0", 20, "upgrade-required"],
    ["12.4.1", 0, "passed"],
    ["13.0.0", 0, "passed"],
  ]) withTemporaryRoot((root) => {
    const probe = path.join(root, "probe");
    addBaseTools(probe, { pnpm });
    for (const tool of ["pkg-config", "WebView2Loader", "cargo-xwin", "llvm-rc", "lld-link", "makensis"]) {
      command(path.join(probe, `${tool}.cmd`), 'type nul > "%~dp0tauri-probed"\r\nexit /b 99');
    }
    const args = ["-Interfaces", "GUI", "-GuiFramework", "gpui"];
    if (status !== 0) args.unshift("-CheckOnly");
    const result = runWindowsGate(root, probe, args);
    assert.equal(result.status, status, `${pnpm}: ${result.stderr}`);
    assert.ok(result.stdout.includes(`gate.pnpm.status=${state}`), result.stdout);
    assert.match(result.stdout, /gate\.pnpm\.requirement=>=12\.4\.1/u);
    assert.match(result.stdout, /gate\.node\.status=passed/u);
    assert.match(result.stdout, /gate\.node\.requirement=>=24\.21\.0/u);
    if (status === 0) {
      assert.match(result.stdout, /gate\.pnpm\.change=existing/u);
      assert.match(result.stdout, /gate\.changed=false/u);
    }
    assert.equal(existsSync(path.join(probe, "tauri-probed")), false);
  });
});

test("Windows GPUI enforces the common Node minimum", windowsRuntime, () => withTemporaryRoot((root) => {
  const probe = path.join(root, "probe");
  addBaseTools(probe, { node: "24.20.9", pnpm: "12.4.1" });
  const result = runWindowsGate(root, probe, ["-CheckOnly", "-Interfaces", "GUI", "-GuiFramework", "gpui"]);
  assert.equal(result.status, 20, result.stderr);
  assert.match(result.stdout, /gate\.node\.status=upgrade-required/u);
  assert.match(result.stdout, /gate\.pnpm\.status=passed/u);
}));

test("Windows test overrides require explicit test mode", windowsRuntime, () => withTemporaryRoot((root) => {
  const probe = path.join(root, "probe");
  addBaseTools(probe);
  const result = runWindowsGate(root, probe, ["-CheckOnly", "-Interfaces", "CLI"], { testMode: false });
  assert.equal(result.status, 2, result.stderr);
  assert.match(result.stderr, /仅在 AFH_TEST_MODE=1/);
}));

test("a Git-only Windows change requires exact fresh PowerShell discovery", windowsRuntime, () => {
  for (const globallyVisible of [true, false]) withTemporaryRoot((root) => {
    const probe = path.join(root, "probe");
    addBaseTools(probe, { git: "2.35.8" });
    command(path.join(probe, "git.cmd"), 'if exist "%~dp0git-upgraded" (echo git version 2.51.0) else (echo git version 2.35.8)');
    command(path.join(probe, "winget.cmd"), 'type nul > "%~dp0git-upgraded"');
    const userPathFile = path.join(root, "user-path.txt");
    writeFileSync(userPathFile, "");
    const result = runWindowsGate(root, probe, ["-Interfaces", "CLI"], {
      extra: {
        AFH_SKIP_PERSIST_PATH: "0",
        AFH_TEST_USER_PATH_FILE: userPathFile,
        AFH_TEST_MACHINE_PATH: globallyVisible ? probe : "relative\\probe",
      },
    });
    if (globallyVisible) {
      assert.equal(result.status, 0, result.stderr);
      assert.match(result.stdout, /gate\.git\.change=upgraded/);
      assert.match(result.stdout, /gate\.fresh_shell\.status=passed/);
    } else {
      assert.equal(result.status, 24, result.stderr);
      assert.match(result.stderr, /持久 User\/Machine PATH 无法解析当前已通过的 rustup/);
      assert.equal(existsSync(path.join(probe, "git-upgraded")), false);
    }
  });
});

test("Windows rejects Machine PATH shadows before user-level upgrades", windowsRuntime, () => {
  for (const scenario of ["rust", "node", "existing-node"]) withTemporaryRoot((root) => {
    const probe = path.join(root, "probe");
    const userPathFile = path.join(root, "user-path.txt");
    writeFileSync(userPathFile, scenario === "existing-node" ? probe : "");
    let machinePath = probe;
    if (scenario === "rust") {
      addBaseTools(probe, { rust: "1.98.0" });
      command(path.join(probe, "rustup.cmd"), 'if "%1"=="toolchain" type nul > "%~dp0rust-install-called"\r\necho rustup 1.28.2 (test)');
    } else if (scenario === "node") {
      addBaseTools(probe, { node: "22.16.0", pnpm: "12.4.1" });
    } else {
      addBaseTools(probe, { node: "26.0.0" });
      machinePath = path.join(root, "machine");
      command(path.join(machinePath, "node.cmd"), "echo v22.16.0");
      command(path.join(machinePath, "npm.cmd"), "echo 10.0.0");
    }
    const result = runWindowsGate(root, probe, ["-Interfaces", scenario === "rust" ? "CLI" : "GUI"], {
      extra: {
        AFH_SKIP_PERSIST_PATH: "0",
        AFH_TEST_USER_PATH_FILE: userPathFile,
        AFH_TEST_MACHINE_PATH: machinePath,
      },
    });
    assert.equal(result.status, 24, `${scenario}: ${result.stderr}`);
    if (scenario === "rust") {
      assert.match(result.stderr, /Machine PATH 中的 rustup 会遮蔽/);
      assert.equal(existsSync(path.join(probe, "rust-install-called")), false);
    } else if (scenario === "node") {
      assert.match(result.stderr, /Machine PATH 中的 node 会遮蔽/);
      assert.equal(existsSync(path.join(root, "node-home")), false);
    } else {
      assert.match(result.stderr, /持久 User\/Machine PATH 解析的 node/);
      assert.equal(existsSync(path.join(root, "pnpm-home")), false);
    }
  });
});

test("every passed Windows tool must keep the same persisted identity", windowsRuntime, () => {
  for (const transientName of ["git", "rustup", "rustc", "cargo", "node", "npm", "pnpm"]) withTemporaryRoot((root) => {
    const persisted = path.join(root, "persisted");
    const transient = path.join(root, "transient");
    mkdirSync(transient, { recursive: true });
    addBaseTools(persisted, { node: "26.0.0", pnpm: "12.4.1" });
    const unexpectedInstall = path.join(root, "unexpected-install");
    command(path.join(persisted, "npm.cmd"), `if "%~1"=="--version" (echo 11.6.0 & exit /b 0)\r\ntype nul > "${unexpectedInstall}"`);
    if (transientName === "pnpm") {
      command(path.join(persisted, "git.cmd"), "echo git version 2.35.8");
      command(path.join(persisted, "winget.cmd"), `type nul > "${unexpectedInstall}"`);
    } else {
      unlinkSync(path.join(persisted, "pnpm.cmd"));
    }
    renameSync(path.join(persisted, `${transientName}.cmd`), path.join(transient, `${transientName}.cmd`));
    const userPathFile = path.join(root, "user-path.txt");
    writeFileSync(userPathFile, persisted);
    const result = runWindowsGate(root, persisted, ["-Interfaces", "GUI"], {
      extra: {
        AFH_PREREQ_PATH: `${persisted};${transient}`,
        AFH_SKIP_PERSIST_PATH: "0",
        AFH_TEST_USER_PATH_FILE: userPathFile,
        AFH_TEST_MACHINE_PATH: "",
      },
    });
    assert.equal(result.status, 24, `${transientName}: ${result.stderr}`);
    assert.ok(result.stderr.includes(`当前已通过的 ${transientName}`), result.stderr);
    assert.equal(existsSync(unexpectedInstall), false);
    assert.equal(existsSync(path.join(root, "pnpm-home")), false);
    assert.equal(readFileSync(userPathFile, "utf8"), persisted);
  });
});

test("Windows rejects missing or pending tools with a different persisted identity", windowsRuntime, () => {
  for (const pending of [false, true]) withTemporaryRoot((root) => {
    const probe = path.join(root, "probe");
    const persisted = path.join(root, "persisted");
    addBaseTools(probe, { node: "26.0.0", pnpm: pending ? "11.23.9" : null });
    command(path.join(persisted, "pnpm.cmd"), "echo 13.0.0");
    const unexpectedInstall = path.join(root, "unexpected-install");
    command(path.join(probe, "npm.cmd"), `if "%~1"=="--version" (echo 11.6.0 & exit /b 0)\r\ntype nul > "${unexpectedInstall}"\r\nexit /b 98`);
    const userPathFile = path.join(root, "user-path.txt");
    const originalPath = pending ? `${persisted};${probe}` : `${probe};${persisted}`;
    writeFileSync(userPathFile, originalPath);
    const result = runWindowsGate(root, probe, ["-Interfaces", "GUI"], {
      extra: {
        AFH_SKIP_PERSIST_PATH: "0",
        AFH_TEST_USER_PATH_FILE: userPathFile,
        AFH_TEST_MACHINE_PATH: "",
      },
    });
    assert.equal(result.status, 24, result.stderr);
    assert.match(result.stderr, pending
      ? /待恢复 pnpm 与当前探测工具路径不一致/
      : /当前探测未找到 pnpm，但持久 User\/Machine PATH 已解析到现有工具/);
    assert.equal(existsSync(unexpectedInstall), false);
    assert.equal(existsSync(path.join(root, "pnpm-home")), false);
    assert.equal(readFileSync(userPathFile, "utf8"), originalPath);
  });
});

test("projected Windows pnpm and Rust paths cannot shadow passed tools", windowsRuntime, () => {
  for (const scenario of ["pnpm-node", "pending-node", "rust-node"]) withTemporaryRoot((root) => {
    const probe = path.join(root, "probe");
    const unexpectedInstall = path.join(root, "unexpected-install");
    const userPathFile = path.join(root, "user-path.txt");
    writeFileSync(userPathFile, probe);
    let prereqPath = probe;
    if (scenario === "rust-node") {
      addBaseTools(probe, { rust: "1.97.0", node: "26.0.0", pnpm: "12.4.1" });
      const rustupBin = path.join(root, "rustup-bin");
      const rustcBin = path.join(root, "rustc-bin");
      const cargoBin = path.join(root, "cargo-bin");
      for (const [tool, destination] of [["rustup", rustupBin], ["rustc", rustcBin], ["cargo", cargoBin]]) {
        mkdirSync(destination);
        renameSync(path.join(probe, `${tool}.cmd`), path.join(destination, `${tool}.cmd`));
      }
      command(path.join(rustupBin, "rustup.cmd"), `if "%~1"=="--version" (echo rustup 1.28.2 ^(test^) & exit /b 0)\r\ntype nul > "${unexpectedInstall}"\r\nexit /b 98`);
      command(path.join(rustcBin, "node.cmd"), "echo v24.21.0");
      prereqPath = [probe, rustupBin, rustcBin, cargoBin].join(";");
    } else {
      addBaseTools(probe, { node: scenario === "pending-node" ? "22.16.0" : "26.0.0" });
      command(path.join(root, "pnpm-home", "node.cmd"), "echo v24.21.0");
      command(path.join(probe, "npm.cmd"), `if "%~1"=="--version" (echo 11.6.0 & exit /b 0)\r\ntype nul > "${unexpectedInstall}"\r\nexit /b 98`);
    }
    const result = runWindowsGate(root, probe, ["-Interfaces", "GUI"], {
      extra: {
        AFH_PREREQ_PATH: prereqPath,
        AFH_ALLOW_FILE_URLS: "1",
        AFH_NODE_DIST_BASE: pathToFileURL(path.join(root, "unreachable-node-dist")).href,
        AFH_SKIP_PERSIST_PATH: "0",
        AFH_TEST_USER_PATH_FILE: userPathFile,
        AFH_TEST_MACHINE_PATH: "",
      },
    });
    assert.equal(result.status, 24, `${scenario}: ${result.stderr}`);
    assert.match(result.stderr, scenario === "rust-node"
      ? /Rust 持久 PATH 目录包含意外的 node/
      : /pnpm 当前用户全局前缀包含意外的 node/);
    assert.equal(existsSync(unexpectedInstall), false);
    assert.equal(readFileSync(userPathFile, "utf8"), probe);
  });
});

test("Windows validates managed Node markers before archive download", windowsRuntime, () => {
  for (const scenario of ["missing-marker", "mismatched-marker"]) withTemporaryRoot((root) => {
    const probe = path.join(root, "probe");
    addBaseTools(probe, { node: "22.16.0", pnpm: "12.4.1" });
    const install = path.join(root, "node-home", "v24.21.1");
    command(path.join(install, "node.cmd"), "echo v24.21.1");
    command(path.join(install, "npm.cmd"), "echo 11.6.0");
    let base = pathToFileURL(path.join(root, "unreachable-node-dist")).href;
    if (scenario === "mismatched-marker") {
      writeFileSync(path.join(install, ".agent-first-harness-managed"),
        `managed by agent-first-harness development environment gate\nnode.version=v24.21.1\nnode.archive.sha256=${"0".repeat(64)}\n`);
      base = makeWindowsNodeMetadata(path.join(root, "node-dist"));
    }
    const result = runWindowsGate(root, probe, ["-Interfaces", "GUI"], {
      extra: { AFH_ALLOW_FILE_URLS: "1", AFH_NODE_DIST_BASE: base },
    });
    assert.equal(result.status, 26, `${scenario}: ${result.stderr}`);
    assert.match(result.stderr, /缺少有效受管所有权标记/);
  });
});

test("Windows rejects reparse roots and wrappers before installers write", windowsRuntime, (context) => {
  for (const scenario of ["node-root", "managed-npm", "pnpm-wrapper"]) {
    let unavailable = false;
    withTemporaryRoot((root) => {
      const probe = path.join(root, "probe");
      const userPathFile = path.join(root, "user-path.txt");
      if (scenario === "node-root") {
        addBaseTools(probe, { node: "22.0.0", pnpm: "12.4.1" });
        const outside = path.join(root, "outside");
        mkdirSync(outside);
        if (!makeJunction(path.join(root, "node-home"), outside)) { unavailable = true; return; }
        const result = runWindowsGate(root, probe, ["-Interfaces", "GUI"], {
          extra: {
            AFH_NODE_DIST_BASE: pathToFileURL(path.join(root, "unreachable-node-dist")).href,
            AFH_ALLOW_FILE_URLS: "1",
          },
        });
        assert.equal(result.status, 24, result.stderr);
        assert.match(result.stderr, /reparse point/);
        assert.deepEqual(readdirSync(outside), []);
        return;
      }
      addBaseTools(probe, { node: scenario === "managed-npm" ? undefined : "26.0.0" });
      let link;
      let external;
      if (scenario === "managed-npm") {
        const nodeVersion = path.join(root, "node-home", "v26.0.0");
        command(path.join(nodeVersion, "node.cmd"), "echo v26.0.0");
        command(path.join(nodeVersion, "npm.cmd"), "echo 11.6.0");
        writeFileSync(path.join(nodeVersion, ".agent-first-harness-managed"),
          `managed by agent-first-harness development environment gate\nnode.version=v26.0.0\nnode.archive.sha256=${"0".repeat(64)}\n`);
        external = path.join(root, "outside", "npm.ps1");
        mkdirSync(path.dirname(external), { recursive: true });
        writeFileSync(external, "Write-Output '11.6.0'\nexit 98\n");
        link = path.join(nodeVersion, "npm.ps1");
        writeFileSync(userPathFile, `${nodeVersion};${probe}`);
      } else {
        external = path.join(root, "outside", "pnpm.ps1");
        mkdirSync(path.dirname(external), { recursive: true });
        writeFileSync(external, "protected external wrapper\r\n");
        link = path.join(root, "pnpm-home", "pnpm.ps1");
        writeFileSync(userPathFile, probe);
      }
      const original = readFileSync(external);
      if (!makeFileSymlink(link, external)) { unavailable = true; return; }
      const result = runWindowsGate(root, probe, ["-Interfaces", "GUI"], {
        extra: {
          AFH_PREREQ_PATH: scenario === "managed-npm" ? `${path.dirname(link)};${probe}` : probe,
          AFH_ALLOW_FILE_URLS: "1",
          AFH_SKIP_PERSIST_PATH: "0",
          AFH_TEST_USER_PATH_FILE: userPathFile,
          AFH_TEST_MACHINE_PATH: "",
        },
      });
      assert.ok([26, 28].includes(result.status), result.stderr);
      assert.match(result.stderr, /reparse point|不在受管目录内/);
      assert.deepEqual(readFileSync(external), original);
      assert.equal(existsSync(path.join(root, "pnpm-home", "pnpm.cmd")), false);
    });
    if (unavailable) {
      context.skip("current Windows host cannot create the required reparse fixture");
      return;
    }
  }
});
