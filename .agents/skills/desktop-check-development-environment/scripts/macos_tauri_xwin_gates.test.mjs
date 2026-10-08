import assert from "node:assert/strict";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

/** 本套件执行 macOS 专用的 `macos-tauri-xwin-gates.sh`，只在 POSIX 主机运行。 */
const posixOnly = { skip: process.platform === "win32" };

const SCRIPT = path.join(path.dirname(fileURLToPath(import.meta.url)), "macos-tauri-xwin-gates.sh");

function shellPath(target) { return path.resolve(target); }

function executable(target, content) {
  mkdirSync(path.dirname(target), { recursive: true });
  writeFileSync(target, content);
  chmodSync(target, 0o755);
}

function withTemporaryRoot(callback) {
  const root = mkdtempSync(path.join(tmpdir(), "afh-xwin-gates-"));
  try { callback(root); } finally { rmSync(root, { recursive: true, force: true }); }
}

function addExistingEnvironment(root, {
  includeCrossTools,
  cargoXwinVersion = "0.23.1",
  installedCargoXwinVersion = "0.23.1",
  cargoInstallExit = 0,
}) {
  const probe = path.join(root, "probe");
  const state = path.join(root, "state");
  mkdirSync(state, { recursive: true });
  executable(path.join(probe, "rustup"), `#!/bin/sh
set -eu
if [ "\${1:-}" = "--version" ]; then
  printf '%s\n' 'rustup 1.28.2'
elif [ "\${1:-} \${2:-} \${3:-}" = "target list --installed" ]; then
  if [ -f "${shellPath(path.join(state, "target"))}" ]; then printf '%s\n' 'x86_64-pc-windows-msvc'; fi
elif [ "$1 $2" = "target add" ]; then
  printf '%s\n' "\${RUSTUP_HOME:?}" > "${shellPath(path.join(state, "rustup-home"))}"
  : > "${shellPath(path.join(state, "target"))}"
else exit 2
fi
`);
  executable(path.join(probe, "cargo"), `#!/bin/sh
set -eu
if [ "\${1:-}" = "--version" ]; then
  printf '%s\n' 'cargo 1.98.1'
elif [ "\${1:-} \${2:-} \${3:-} \${4:-} \${5:-}" = "install --locked --version >=0.23.1, <0.24.0 cargo-xwin" ]; then
  [ ${cargoInstallExit} -eq 0 ] || exit ${cargoInstallExit}
  printf '%s\n' "\${CARGO_HOME:?}" > "${shellPath(path.join(state, "cargo-home"))}"
  mkdir -p "$CARGO_HOME/bin"
  printf '#!/bin/sh\nprintf "%%s\\n" "cargo-xwin ${installedCargoXwinVersion}"\n' > "$CARGO_HOME/bin/cargo-xwin"
  chmod +x "$CARGO_HOME/bin/cargo-xwin"
else exit 2
fi
`);
  executable(path.join(probe, "pnpm"), "#!/bin/sh\nprintf '%s\n' '12.4.1'\n");
  if (includeCrossTools) {
    writeFileSync(path.join(state, "target"), "");
    executable(path.join(probe, "llvm-rc"), "#!/bin/sh\nprintf '%s\n' 'llvm-rc test'\n");
    executable(path.join(probe, "lld-link"), "#!/bin/sh\nprintf '%s\n' 'lld-link test'\n");
    executable(path.join(probe, "makensis"), "#!/bin/sh\nprintf '%s\n' 'NSIS test'\n");
    executable(path.join(probe, "cargo-xwin"), `#!/bin/sh\nprintf '%s\n' 'cargo-xwin ${cargoXwinVersion}'\n`);
  }
  return probe;
}

function addFakeFormulaTool(root, formula, tool) {
  executable(path.join(root, "brew", formula, "bin", tool), "#!/bin/sh\nexit 0\n");
}

function addFakeBrew(root, probe, { failFormula = "" } = {}) {
  const brewRoot = path.join(root, "brew");
  executable(path.join(probe, "brew"), `#!/bin/sh
set -eu
root='${shellPath(brewRoot)}'
if [ "$1" = "--prefix" ]; then
  [ -d "$root/$2" ] || exit 1
  printf '%s\n' "$root/$2"
elif [ "\${1:-} \${2:-} \${3:-}" = "list --versions --formula" ]; then
  formula=$4
  [ -d "$root/$formula" ] || exit 1
  printf '%s\n' "$formula 1.0.0"
elif [ "$1" = install ]; then
  formula=$2
  [ "$formula" != "${failFormula}" ] || exit 9
  mkdir -p "$root/$formula/bin"
  case "$formula" in
    llvm) printf '#!/bin/sh\nprintf "%%s\\n" "Exactly one input file should be provided." >&2\nexit 1\n' > "$root/$formula/bin/llvm-rc"; chmod +x "$root/$formula/bin/llvm-rc" ;;
    lld) printf '#!/bin/sh\nexit 0\n' > "$root/$formula/bin/lld-link"; chmod +x "$root/$formula/bin/lld-link" ;;
    nsis) printf '#!/bin/sh\nexit 0\n' > "$root/$formula/bin/makensis"; chmod +x "$root/$formula/bin/makensis" ;;
  esac
else exit 2
fi
`);
}

function addPersistedRustHomeLogin(root) {
  const home = path.join(root, "home");
  mkdirSync(home, { recursive: true });
  writeFileSync(path.join(home, ".profile"), `export CARGO_HOME='${shellPath(path.join(home, "custom-cargo"))}'\nexport RUSTUP_HOME='${shellPath(path.join(home, "custom-rustup"))}'\n`);
  const loginShell = path.join(root, "login-shell");
  executable(loginShell, "#!/bin/sh\n[ \"${1:-}\" = -l ] && [ \"${2:-}\" = -c ] || exit 90\n. \"$HOME/.profile\"\nexec /bin/sh -c \"$3\"\n");
  return loginShell;
}

function runGate(root, probe, args = [], { testMode = true, rustHomeMode = "test-overrides", extra = {} } = {}) {
  const env = {
    ...process.env,
    AFH_PREREQ_PATH: shellPath(probe),
    AFH_TEST_PLATFORM: "Darwin",
    AFH_ALLOW_TEST_OVERRIDES: "1",
    AFH_TEST_MODE: "1",
    HOME: shellPath(path.join(root, "home")),
    CARGO_HOME: shellPath(path.join(root, "project", ".cargo")),
    RUSTUP_HOME: shellPath(path.join(root, "project", ".rustup")),
    AFH_MANAGED_CARGO_HOME: shellPath(path.join(root, "cargo")),
    AFH_MANAGED_RUSTUP_HOME: shellPath(path.join(root, "rustup")),
  };
  if (rustHomeMode === "standard") {
    env.CARGO_HOME = shellPath(path.join(root, "home", "custom-cargo"));
    env.RUSTUP_HOME = shellPath(path.join(root, "home", "custom-rustup"));
    delete env.AFH_MANAGED_CARGO_HOME;
    delete env.AFH_MANAGED_RUSTUP_HOME;
  } else if (rustHomeMode === "defaults") {
    for (const name of ["CARGO_HOME", "RUSTUP_HOME", "AFH_MANAGED_CARGO_HOME", "AFH_MANAGED_RUSTUP_HOME"]) delete env[name];
  }
  if (!testMode) delete env.AFH_TEST_MODE;
  Object.assign(env, extra);
  return spawnSync("/bin/sh", [SCRIPT, ...args], { cwd: root, env, encoding: "utf8", timeout: 30_000 });
}

test("test overrides require explicit test mode", posixOnly, () => withTemporaryRoot((root) => {
  const probe = path.join(root, "probe");
  mkdirSync(probe);
  const result = runGate(root, probe, ["--check-only"], { testMode: false });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /仅在 AFH_TEST_MODE=1/);
}));

test("existing environment passes without installing", posixOnly, () => withTemporaryRoot((root) => {
  const probe = addExistingEnvironment(root, { includeCrossTools: true });
  const result = runGate(root, probe, ["--install-missing"]);
  assert.equal(result.status, 0, result.stderr);
  for (const fragment of [
    "gate.tauri_windows_cross.status=passed",
    "gate.cargo_xwin.requirement=>=0.23.1, <0.24.0",
    "gate.cargo_xwin.version=0.23.1",
    "gate.changed=false",
  ]) assert.ok(result.stdout.includes(fragment), fragment);
  assert.equal(existsSync(path.join(root, "cargo")), false);
}));

test("missing environment is installed and reprobed", posixOnly, () => withTemporaryRoot((root) => {
  const probe = addExistingEnvironment(root, { includeCrossTools: false });
  addFakeBrew(root, probe);
  const result = runGate(root, probe, ["--install-missing"]);
  assert.equal(result.status, 0, result.stderr);
  for (const tool of ["llvm", "lld", "nsis", "rust_target", "cargo_xwin"]) assert.match(result.stdout, new RegExp(`gate\\.${tool}\\.change=installed`));
  assert.match(result.stdout, /gate\.changed=true/);
  assert.equal(existsSync(path.join(root, "cargo", "bin", "cargo-xwin")), true);
  assert.equal(existsSync(path.join(root, "project")), false);
}));

test("standard Cargo and rustup homes are respected", posixOnly, () => withTemporaryRoot((root) => {
  const probe = addExistingEnvironment(root, { includeCrossTools: false });
  addFakeBrew(root, probe);
  const loginShell = addPersistedRustHomeLogin(root);
  const result = runGate(root, probe, ["--install-missing"], { rustHomeMode: "standard", extra: { SHELL: loginShell } });
  assert.equal(result.status, 0, result.stderr);
  const cargoHome = path.join(root, "home", "custom-cargo");
  const rustupHome = path.join(root, "home", "custom-rustup");
  assert.equal(readFileSync(path.join(root, "state", "cargo-home"), "utf8").trim(), cargoHome);
  assert.equal(readFileSync(path.join(root, "state", "rustup-home"), "utf8").trim(), rustupHome);
  assert.equal(existsSync(path.join(cargoHome, "bin", "cargo-xwin")), true);
  assert.equal(existsSync(path.join(root, "cargo")), false);
}));

test("process-only custom Rust homes fail before install", posixOnly, () => withTemporaryRoot((root) => {
  mkdirSync(path.join(root, "home"));
  const probe = addExistingEnvironment(root, { includeCrossTools: false });
  addFakeBrew(root, probe);
  const loginShell = path.join(root, "login-shell");
  executable(loginShell, "#!/bin/sh\n[ \"${1:-}\" = -l ] && [ \"${2:-}\" = -c ] || exit 90\nexec /bin/sh -c \"$3\"\n");
  const result = runGate(root, probe, ["--install-missing"], { rustHomeMode: "standard", extra: { SHELL: loginShell } });
  assert.equal(result.status, 36, result.stderr);
  assert.match(result.stderr, /必须由新 login shell 持久恢复/);
  assert.equal(existsSync(path.join(root, "state", "target")), false);
}));

test("standard Rust home rejects symlinked intermediate component", posixOnly, () => withTemporaryRoot((root) => {
  const probe = addExistingEnvironment(root, { includeCrossTools: false });
  addFakeBrew(root, probe);
  const home = path.join(root, "home");
  const outside = path.join(root, "outside-rust-home");
  mkdirSync(home); mkdirSync(outside);
  symlinkSync(outside, path.join(home, "tools"), "dir");
  const result = runGate(root, probe, ["--install-missing"], {
    rustHomeMode: "standard", extra: { CARGO_HOME: `${home}/tools/cargo` },
  });
  assert.equal(result.status, 36, result.stderr);
  assert.match(result.stderr, /路径组件不能是符号链接/);
  assert.deepEqual(readdirSync(outside), []);
}));

test("path separator in home or Rust root fails before probe", posixOnly, () => {
  for (const name of ["HOME", "AFH_MANAGED_CARGO_HOME", "AFH_MANAGED_RUSTUP_HOME"]) withTemporaryRoot((root) => {
    mkdirSync(path.join(root, "home"));
    const probe = addExistingEnvironment(root, { includeCrossTools: false });
    const base = name === "HOME" ? path.join(root, "home") : name.includes("CARGO") ? path.join(root, "cargo") : path.join(root, "rustup");
    const result = runGate(root, probe, [], { extra: { [name]: `${base}:${path.join(root, "outside")}` } });
    assert.equal(result.status, 2, result.stderr);
    assert.match(result.stderr, /PATH 分隔符冒号/);
  });
});

test("unset standard Rust homes use HOME defaults", posixOnly, () => withTemporaryRoot((root) => {
  const probe = addExistingEnvironment(root, { includeCrossTools: false });
  addFakeBrew(root, probe);
  const result = runGate(root, probe, ["--install-missing"], { rustHomeMode: "defaults" });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(readFileSync(path.join(root, "state", "cargo-home"), "utf8").trim(), path.join(root, "home", ".cargo"));
  assert.equal(readFileSync(path.join(root, "state", "rustup-home"), "utf8").trim(), path.join(root, "home", ".rustup"));
}));

test("install PATH drops empty and duplicate probe segments", posixOnly, () => withTemporaryRoot((root) => {
  const probe = addExistingEnvironment(root, { includeCrossTools: false });
  addFakeBrew(root, probe);
  const result = runGate(root, probe, ["--install-missing"], { extra: { AFH_PREREQ_PATH: `:${probe}::${probe}:` } });
  assert.equal(result.status, 0, result.stderr);
  const pathLine = result.stdout.split("\n").find((line) => line.startsWith("gate.path.prepend="));
  const entries = pathLine.slice("gate.path.prepend=".length).split(":");
  assert.equal(entries.includes(""), false);
  assert.equal(entries.length, new Set(entries).size);
}));

test("literal glob probe entry is never expanded", posixOnly, () => withTemporaryRoot((root) => {
  const probe = addExistingEnvironment(root, { includeCrossTools: true });
  unlinkSync(path.join(probe, "cargo-xwin"));
  const globMatch = path.join(root, "glob-match");
  executable(path.join(globMatch, "cargo-xwin"), "#!/bin/sh\nprintf '%s\n' 'cargo-xwin 0.23.1'\n");
  const result = runGate(root, probe, ["--check-only"], { extra: { AFH_PREREQ_PATH: `${root}/*:${probe}` } });
  assert.equal(result.status, 20, result.stderr);
  assert.match(result.stdout, /gate\.cargo_xwin\.status=missing/);
}));

test("higher compatible cargo-xwin is preserved", posixOnly, () => withTemporaryRoot((root) => {
  const probe = addExistingEnvironment(root, { includeCrossTools: true, cargoXwinVersion: "0.23.9" });
  const result = runGate(root, probe, ["--install-missing"]);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /gate\.cargo_xwin\.version=0\.23\.9/);
  assert.match(result.stdout, /gate\.cargo_xwin\.change=existing/);
}));

test("failing version and target probes are rejected", posixOnly, () => {
  withTemporaryRoot((root) => {
    const probe = addExistingEnvironment(root, { includeCrossTools: true });
    executable(path.join(probe, "cargo-xwin"), "#!/bin/sh\nprintf '%s\n' 'cargo-xwin 0.23.1'\nexit 9\n");
    const result = runGate(root, probe, ["--check-only"]);
    assert.equal(result.status, 36);
    assert.match(result.stderr, /版本探测返回失败/);
  });
  withTemporaryRoot((root) => {
    const probe = addExistingEnvironment(root, { includeCrossTools: true });
    executable(path.join(probe, "rustup"), "#!/bin/sh\nprintf '%s\n' 'x86_64-pc-windows-msvc'\nexit 9\n");
    const result = runGate(root, probe, ["--check-only"]);
    assert.equal(result.status, 36);
    assert.match(result.stderr, /无法列出已安装 target/);
  });
});

test("unrecognized llvm-rc and failing base tools are not accepted", posixOnly, () => {
  withTemporaryRoot((root) => {
    const probe = addExistingEnvironment(root, { includeCrossTools: true });
    executable(path.join(probe, "llvm-rc"), "#!/bin/sh\nprintf '%s\n' broken >&2\nexit 1\n");
    const result = runGate(root, probe, ["--check-only"]);
    assert.equal(result.status, 20);
    assert.match(result.stdout, /gate\.llvm\.status=missing/);
  });
  for (const tool of ["cargo", "pnpm"]) withTemporaryRoot((root) => {
    const probe = addExistingEnvironment(root, { includeCrossTools: true });
    executable(path.join(probe, tool), "#!/bin/sh\nexit 9\n");
    const result = runGate(root, probe, ["--check-only"]);
    assert.equal(result.status, 20);
    assert.match(result.stdout, /gate\.tauri_windows_cross\.base=missing/);
  });
});

test("outdated cargo-xwin is upgraded and reprobed", posixOnly, () => {
  for (const version of ["0.22.9", "0.23.0"]) withTemporaryRoot((root) => {
    const probe = addExistingEnvironment(root, { includeCrossTools: true, cargoXwinVersion: version });
    const result = runGate(root, probe, ["--install-missing"]);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /gate\.cargo_xwin\.version=0\.23\.1/);
    assert.match(result.stdout, /gate\.cargo_xwin\.change=upgraded/);
    assert.equal(existsSync(path.join(root, "cargo", "bin", "cargo-xwin")), true);
  });
});

test("check-only reports outdated cargo-xwin without writes", posixOnly, () => withTemporaryRoot((root) => {
  const probe = addExistingEnvironment(root, { includeCrossTools: true, cargoXwinVersion: "0.23.0" });
  const result = runGate(root, probe, ["--check-only"]);
  assert.equal(result.status, 20, result.stderr);
  assert.match(result.stdout, /gate\.tauri_windows_cross\.status=upgrade-required/);
  assert.equal(existsSync(path.join(root, "cargo")), false);
}));

test("upgrade failures and invalid versions fail closed", posixOnly, () => {
  withTemporaryRoot((root) => {
    const probe = addExistingEnvironment(root, { includeCrossTools: true, cargoXwinVersion: "0.23.0", cargoInstallExit: 9 });
    const result = runGate(root, probe, ["--install-missing"]);
    assert.equal(result.status, 36);
    assert.match(result.stderr, /cargo-xwin 安装失败/);
  });
  withTemporaryRoot((root) => {
    const probe = addExistingEnvironment(root, { includeCrossTools: true, cargoXwinVersion: "0.22.9", installedCargoXwinVersion: "0.23.0" });
    const result = runGate(root, probe, ["--install-missing"]);
    assert.equal(result.status, 37);
    assert.match(result.stderr, /安装后复探仍失败/);
  });
  for (const version of ["0.24.0", "0.23.1-beta.1", "invalid"]) withTemporaryRoot((root) => {
    const probe = addExistingEnvironment(root, { includeCrossTools: true, cargoXwinVersion: version });
    const result = runGate(root, probe, ["--install-missing"]);
    assert.equal(result.status, 36);
    assert.match(result.stderr, /不满足兼容范围/);
    assert.equal(existsSync(path.join(root, "cargo")), false);
  });
});

test("check-only reports missing without writes", posixOnly, () => withTemporaryRoot((root) => {
  const probe = addExistingEnvironment(root, { includeCrossTools: false });
  addFakeBrew(root, probe);
  const result = runGate(root, probe, ["--check-only"]);
  assert.equal(result.status, 20);
  assert.match(result.stdout, /gate\.tauri_windows_cross\.status=missing/);
  assert.equal(existsSync(path.join(root, "brew", "llvm")), false);
  assert.equal(existsSync(path.join(root, "state", "target")), false);
}));

test("missing Homebrew and formula install failures block", posixOnly, () => {
  withTemporaryRoot((root) => {
    const probe = addExistingEnvironment(root, { includeCrossTools: false });
    const result = runGate(root, probe, ["--install-missing"]);
    assert.equal(result.status, 32);
    assert.match(result.stderr, /不自动安装 Homebrew/);
  });
  withTemporaryRoot((root) => {
    const probe = addExistingEnvironment(root, { includeCrossTools: false });
    addFakeBrew(root, probe, { failFormula: "llvm" });
    const result = runGate(root, probe, ["--install-missing"]);
    assert.equal(result.status, 33);
    assert.match(result.stderr, /LLVM 安装失败/);
  });
});

test("split LLVM installs lld and damaged formulas fail before writes", posixOnly, () => {
  withTemporaryRoot((root) => {
    const probe = addExistingEnvironment(root, { includeCrossTools: false });
    addFakeBrew(root, probe);
    addFakeFormulaTool(root, "llvm", "llvm-rc");
    addFakeFormulaTool(root, "nsis", "makensis");
    writeFileSync(path.join(root, "state", "target"), "");
    executable(path.join(probe, "cargo-xwin"), "#!/bin/sh\nprintf '%s\n' 'cargo-xwin 0.23.1'\n");
    const result = runGate(root, probe, ["--install-missing"]);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /gate\.lld\.change=installed/);
  });
  for (const missingBin of [false, true]) withTemporaryRoot((root) => {
    const probe = addExistingEnvironment(root, { includeCrossTools: false });
    addFakeBrew(root, probe);
    addFakeFormulaTool(root, "llvm", "llvm-rc");
    addFakeFormulaTool(root, "nsis", "makensis");
    mkdirSync(path.join(root, "brew", "lld", ...(missingBin ? [] : ["bin"])), { recursive: true });
    const result = runGate(root, probe, ["--install-missing"]);
    assert.equal(result.status, 33);
    assert.match(result.stderr, /既有 LLD 缺少 lld-link/);
  });
});

test("all formula conflicts are preflighted before install", posixOnly, () => withTemporaryRoot((root) => {
  const probe = addExistingEnvironment(root, { includeCrossTools: false });
  addFakeBrew(root, probe);
  mkdirSync(path.join(root, "brew", "llvm", "bin"), { recursive: true });
  addFakeFormulaTool(root, "nsis", "makensis");
  const result = runGate(root, probe, ["--install-missing"]);
  assert.equal(result.status, 33);
  assert.match(result.stderr, /既有 LLVM 缺少 llvm-rc/);
  assert.equal(existsSync(path.join(root, "brew", "lld")), false);
}));

test("non-macOS host and unsupported target are rejected", posixOnly, () => {
  withTemporaryRoot((root) => {
    const probe = addExistingEnvironment(root, { includeCrossTools: true });
    const result = runGate(root, probe, ["--check-only"], { extra: { AFH_TEST_PLATFORM: "Linux" } });
    assert.equal(result.status, 30);
    assert.match(result.stdout, /requires-macos-host/);
  });
  withTemporaryRoot((root) => {
    const probe = addExistingEnvironment(root, { includeCrossTools: true });
    const result = runGate(root, probe, ["--check-only", "--target", "aarch64-pc-windows-msvc"]);
    assert.equal(result.status, 2);
    assert.match(result.stderr, /不支持的 Tauri xwin 目标/);
  });
});
