import { createHash } from "node:crypto";
import { chmodSync, existsSync, mkdirSync, readFileSync, realpathSync, symlinkSync, writeFileSync } from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { pathToFileURL, fileURLToPath } from "node:url";

export const SCRIPT = path.join(path.dirname(fileURLToPath(import.meta.url)), "development-environment-gates.sh");
export const WINDOWS_SCRIPT = path.join(path.dirname(SCRIPT), "development-environment-gates.ps1");

export function executable(target, content) {
  mkdirSync(path.dirname(target), { recursive: true });
  writeFileSync(target, content);
  chmodSync(target, 0o755);
}

export function fakeNodeTools(binDir, { node = "24.21.0" } = {}) {
  executable(path.join(binDir, "node"), `#!/bin/sh\nprintf '%s\n' 'v${node}'\n`);
  fakeNpmInstaller(binDir);
}

export function fakeExistingTools(binDir, {
  rust = "1.98.1",
  git = "2.39.0",
  includeGit = true,
  includeNode = true,
} = {}) {
  mkdirSync(binDir, { recursive: true });
  executable(path.join(binDir, "rustc"), `#!/bin/sh
if [ "\${1:-}" = -vV ]; then
  printf '%s\n' 'rustc ${rust} (test)' 'host: x86_64-unknown-linux-gnu' 'release: ${rust}'
else
  printf '%s\n' 'rustc ${rust} (test)'
fi
`);
  executable(path.join(binDir, "cargo"), `#!/bin/sh\nprintf '%s\n' 'cargo ${rust} (test)'\n`);
  executable(path.join(binDir, "rustup"), "#!/bin/sh\nprintf '%s\n' 'rustup 1.28.0 (test)'\n");
  if (includeGit) executable(path.join(binDir, "git"), `#!/bin/sh\nprintf '%s\n' 'git version ${git}'\n`);
  if (includeNode) fakeNodeTools(binDir);
}

export function fakeNpmInstaller(binDir, { pnpm = "12.4.1", succeeds = true } = {}) {
  if (!succeeds) {
    executable(path.join(binDir, "npm"), "#!/bin/sh\nif [ \"${1:-}\" = --version ]; then printf '%s\\n' '11.6.0'; exit 0; fi\nexit 9\n");
    return;
  }
  executable(path.join(binDir, "npm"), `#!/bin/sh
set -eu
if [ "\${1:-}" = --version ]; then printf '%s\n' '11.6.0'; exit 0; fi
case "$*" in *"pnpm@>=12.4.1"*) ;; *) exit 8 ;; esac
prefix=
registry=
ignore_scripts=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --prefix) prefix=$2; shift 2 ;;
    --registry) registry=$2; shift 2 ;;
    --ignore-scripts) ignore_scripts=1; shift ;;
    *) shift ;;
  esac
done
[ "$registry" = "\${AFH_PNPM_REGISTRY:-https://registry.npmjs.org/}" ] || exit 7
[ "$ignore_scripts" -eq 1 ] || exit 6
mkdir -p "$prefix/bin" "$prefix/lib/node_modules/pnpm/bin"
printf '#!/bin/sh\nprintf "%%s\\n" "${pnpm}"\n' > "$prefix/lib/node_modules/pnpm/bin/pnpm"
chmod +x "$prefix/lib/node_modules/pnpm/bin/pnpm"
rm -f "$prefix/bin/pnpm"
if [ -n "\${AFH_TEST_PNPM_LINK_TARGET:-}" ]; then
  ln -s "$AFH_TEST_PNPM_LINK_TARGET" "$prefix/bin/pnpm"
else
  ln -s "$prefix/lib/node_modules/pnpm/bin/pnpm" "$prefix/bin/pnpm"
fi
`);
}

export function fakeFrontendTools(binDir, { node = "24.21.0", pnpm = "12.4.1" } = {}) {
  fakeNodeTools(binDir, { node });
  executable(path.join(binDir, "pnpm"), `#!/bin/sh\nprintf '%s\n' '${pnpm}'\n`);
}

export function fakeGitPackageManager(binDir, { git = "2.51.0" } = {}) {
  const gitBody = `#!/bin/sh\nprintf '%s\\n' 'git version ${git}'\n`;
  if (process.platform === "darwin") {
    executable(path.join(binDir, "brew"), `#!/bin/sh
set -eu
if [ "$1" = --prefix ]; then printf '%s\n' "$AFH_PREREQ_PATH"; exit 0; fi
[ "$1" = install ] && [ "$2" = git ]
printf '%b' ${JSON.stringify(gitBody)} > "$AFH_PREREQ_PATH/git"
chmod +x "$AFH_PREREQ_PATH/git"
`);
  } else {
    executable(path.join(binDir, "apt-get"), `#!/bin/sh
set -eu
if [ "$1" = update ]; then exit 0; fi
[ "$1" = install ] && [ "$2" = -y ] && [ "$3" = git ]
printf '%b' ${JSON.stringify(gitBody)} > "$AFH_PREREQ_PATH/git"
chmod +x "$AFH_PREREQ_PATH/git"
`);
    executable(path.join(binDir, "sudo"), "#!/bin/sh\nexec \"$@\"\n");
  }
}

function nodeTuple() {
  const nodePlatform = process.platform === "darwin" ? "darwin" : "linux";
  const nodeArch = process.arch === "x64" ? "x64" : "arm64";
  return [nodePlatform, nodeArch];
}

function createTarGzip(sourceRoot, archive) {
  const result = spawnSync("tar", ["-czf", archive, "-C", path.dirname(sourceRoot), path.basename(sourceRoot)], { encoding: "utf8" });
  if (result.status !== 0) throw new Error(`cannot create Node fixture archive: ${result.stderr}`);
}

export function makeNodeDist(root, {
  validChecksum = true,
  forcedTuple = null,
  version = "v24.21.0",
  reportedVersion = null,
} = {}) {
  const shownVersion = reportedVersion ?? version;
  const [nodePlatform, nodeArch] = forcedTuple ?? nodeTuple();
  const release = path.join(root, version);
  mkdirSync(release, { recursive: true });
  writeFileSync(path.join(root, "index.tab"), `version\tdate\tfiles\tnpm\tv8\tuv\tzlib\topenssl\tmodules\tlts\tsecurity
v25.9.0\t2026-02-01\ttest\t11\t1\t1\t1\t1\t1\t-\t-
v24.21.0\t2026-01-02\ttest\t11\t1\t1\t1\t1\t1\tOlderLTS\t-
${version}\t2026-01-01\ttest\t11\t1\t1\t1\t1\t1\tTestLTS\t-
`);
  const archiveName = `node-${version}-${nodePlatform}-${nodeArch}.tar.gz`;
  const sourceRoot = path.join(root, `node-${version}-${nodePlatform}-${nodeArch}`);
  executable(path.join(sourceRoot, "bin", "node"), `#!/bin/sh\nprintf '%s\n' '${shownVersion}'\n`);
  fakeNpmInstaller(path.join(sourceRoot, "bin"));
  const archive = path.join(release, archiveName);
  createTarGzip(sourceRoot, archive);
  let digest = createHash("sha256").update(readFileSync(archive)).digest("hex");
  if (!validChecksum) digest = "0".repeat(64);
  writeFileSync(path.join(release, "SHASUMS256.txt"), `${digest}  ${archiveName}\n`);
  return pathToFileURL(root).href;
}

function rustupTarget() {
  const architecture = process.arch === "x64" ? "x86_64" : "aarch64";
  if (process.platform === "darwin") return `${architecture}-apple-darwin`;
  const ldd = spawnSync("ldd", ["--version"], { encoding: "utf8" });
  const libc = `${ldd.stdout}${ldd.stderr}`.toLowerCase().includes("musl") ? "musl" : "gnu";
  return `${architecture}-unknown-linux-${libc}`;
}

export function makeRustDist(root, {
  installedVersion = "1.98.1",
  succeeds = true,
  validChecksum = true,
  forcedTarget = null,
} = {}) {
  const release = path.join(root, forcedTarget ?? rustupTarget());
  mkdirSync(release, { recursive: true });
  const installer = path.join(release, "rustup-init");
  const body = succeeds ? `#!/bin/sh
set -eu
case " $* " in *" --no-modify-path "*) ;; *) exit 97 ;; esac
mkdir -p "$CARGO_HOME/bin" "$RUSTUP_HOME"
for tool in rustc cargo rustup; do
  case "$tool" in
    rustc)
      printf '#!/bin/sh\nif [ "\${1:-}" = -vV ]; then\n  printf "%%s\\n" "rustc ${installedVersion} (test)" "host: x86_64-unknown-linux-gnu" "release: ${installedVersion}"\nelse\n  printf "%%s\\n" "rustc ${installedVersion} (test)"\nfi\n' > "$CARGO_HOME/bin/$tool"
      chmod +x "$CARGO_HOME/bin/$tool"
      continue ;;
    cargo) version='cargo ${installedVersion} (test)' ;;
    rustup) version='rustup 1.28.0 (test)' ;;
  esac
  printf '#!/bin/sh\nprintf "%%s\\n" "%s"\n' "$version" > "$CARGO_HOME/bin/$tool"
  chmod +x "$CARGO_HOME/bin/$tool"
done
` : "#!/bin/sh\nexit 9\n";
  executable(installer, body);
  const digest = validChecksum ? createHash("sha256").update(readFileSync(installer)).digest("hex") : "0".repeat(64);
  writeFileSync(path.join(release, "rustup-init.sha256"), `${digest}  rustup-init\n`);
  return pathToFileURL(root).href;
}

let resolvedHostAwk = null;

/** 每个测试进程只解析一次宿主 awk 真实路径。 */
function hostAwk() {
  if (resolvedHostAwk === null) {
    const awk = spawnSync("sh", ["-c", "command -v awk"], { encoding: "utf8" }).stdout.trim();
    if (!awk) throw new Error("POSIX gate tests require awk");
    resolvedHostAwk = realpathSync(awk);
  }
  return resolvedHostAwk;
}

export function runGate(root, args, { probe = null, testMode = true, extra = {} } = {}) {
  const probePath = probe ?? path.join(root, "probe");
  mkdirSync(probePath, { recursive: true });
  mkdirSync(path.join(root, "home"), { recursive: true });
  const loginShell = path.join(root, "test-login-shell", "sh");
  executable(loginShell, "#!/bin/sh\n[ \"${1:-}\" = -l ] && [ \"${2:-}\" = -c ] || exit 90\n[ ! -f \"$HOME/.profile\" ] || . \"$HOME/.profile\"\neval \"$3\"\n");
  const freshSystem = path.join(root, "fresh-system");
  mkdirSync(freshSystem, { recursive: true });
  const freshAwk = path.join(freshSystem, "awk");
  if (!existsSync(freshAwk)) symlinkSync(hostAwk(), freshAwk);
  const env = {
    ...process.env,
    AFH_PREREQ_PATH: probePath,
    AFH_TEST_SYSTEM_PATH: `${probePath}:${freshSystem}`,
    HOME: path.join(root, "home"),
    CARGO_HOME: path.join(root, "home", ".cargo"),
    RUSTUP_HOME: path.join(root, "home", ".rustup"),
    SHELL: loginShell,
    PATH: `${probePath}:${process.env.PATH ?? ""}`,
    AFH_TEST_MODE: "1",
  };
  if (!testMode) delete env.AFH_TEST_MODE;
  Object.assign(env, extra);
  return spawnSync("sh", [SCRIPT, ...args], { cwd: root, env, encoding: "utf8", timeout: 30_000 });
}
