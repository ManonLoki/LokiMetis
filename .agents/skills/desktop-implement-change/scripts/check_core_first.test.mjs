import assert from "node:assert/strict";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";

import { main, validateMetadata } from "./check_core_first.mjs";

const REGISTRY_SOURCE = "registry+https://github.com/rust-lang/crates.io-index";

/** 伪造的 cargo 是 `#!/bin/sh` 脚本，只有 POSIX 主机能直接执行。 */
const posixOnly = { skip: process.platform === "win32" };

function dependency(name, { kind = null, optional = false, target = null, rename = null, dependencyPath = null, source = REGISTRY_SOURCE } = {}) {
  return { name, kind, optional, target, rename, path: dependencyPath, source };
}

function workspaceDependency(name, options = {}) {
  return dependency(name, { ...options, dependencyPath: `/fixture/${name}`, source: null });
}

function packageFixture(name, { packageDependencies = [], library = false } = {}) {
  return {
    id: `path+file:///fixture/${name}#0.1.0`,
    name,
    manifest_path: `/fixture/${name}/Cargo.toml`,
    dependencies: packageDependencies,
    targets: [{ kind: [library ? "lib" : "bin"] }],
  };
}

function metadata(...packages) {
  return { packages, workspace_members: packages.map((item) => String(item.id)) };
}

function captureOutput(callback) {
  let stdout = "";
  let stderr = "";
  const stdoutWrite = process.stdout.write;
  const stderrWrite = process.stderr.write;
  process.stdout.write = (chunk) => { stdout += String(chunk); return true; };
  process.stderr.write = (chunk) => { stderr += String(chunk); return true; };
  try { return { code: callback(), stdout, stderr }; }
  finally { process.stdout.write = stdoutWrite; process.stderr.write = stderrWrite; }
}

function runMainJson(...argumentsList) {
  const result = captureOutput(() => main([...argumentsList, "--json"]));
  return { ...result, payload: JSON.parse(result.stdout) };
}

function withTemporaryRoot(callback) {
  const root = mkdtempSync(path.join(tmpdir(), "afh-core-first-"));
  try { callback(root); } finally { rmSync(root, { recursive: true, force: true }); }
}

test("accepts core, adapters, and extra infrastructure", () => {
  const fixture = metadata(
    packageFixture("sample_core", { packageDependencies: [workspaceDependency("sample_infra")], library: true }),
    packageFixture("sample_infra", { library: true }),
    packageFixture("sample_cli", { packageDependencies: [workspaceDependency("sample_core")] }),
    packageFixture("sample_gui", { packageDependencies: [workspaceDependency("sample_core")] }),
  );
  assert.deepEqual(validateMetadata(fixture), []);
});

test("accepts renamed normal core dependency", () => {
  const fixture = metadata(
    packageFixture("sample_core", { library: true }),
    packageFixture("sample_cli", { packageDependencies: [workspaceDependency("sample_core", { rename: "business" })] }),
  );
  assert.deepEqual(validateMetadata(fixture), []);
});

/** GPUI 和包装 crate 都是适配器依赖，改名或间接经过基础设施也不能进入 core。 */
test("rejects_gpui_framework_dependencies_reachable_from_core", () => {
  for (const framework of ["gpui", "gpui-pre", "gpui-kit", "gpui-component", "gpui-component-assets"]) {
    const fixture = metadata(
      packageFixture("sample_core", { library: true, packageDependencies: [workspaceDependency("sample_infra")] }),
      packageFixture("sample_infra", { library: true, packageDependencies: [dependency(framework, { rename: "native_ui", kind: "dev", target: "cfg(unix)" })] }),
      packageFixture("sample_gui", { packageDependencies: [workspaceDependency("sample_core"), dependency(framework)] }),
    );
    const errors = validateMetadata(fixture);
    assert.ok(errors.some((error) => error.includes(framework)), `${framework}: ${errors.join("; ")}`);
  }
});

test("rejects missing unconditional core dependency", () => {
  const fixture = metadata(
    packageFixture("sample_core", { library: true }),
    packageFixture("sample_cli", { packageDependencies: [
      workspaceDependency("sample_core", { kind: "dev" }),
      workspaceDependency("sample_core", { target: 'cfg(target_os = "windows")' }),
    ] }),
  );
  assert.ok(validateMetadata(fixture).some((error) => error.includes("必须以非可选")));
});

test("rejects core reverse dependency in any scope", () => {
  const fixture = metadata(
    packageFixture("sample_core", { library: true, packageDependencies: [
      workspaceDependency("sample_cli", { kind: "dev", target: "cfg(unix)", rename: "test_cli" }),
    ] }),
    packageFixture("sample_cli", { packageDependencies: [workspaceDependency("sample_core")] }),
  );
  assert.ok(validateMetadata(fixture).some((error) => error.includes("不得依赖 adapter sample_cli")));
});

test("rejects adapter-to-adapter dependency", () => {
  const fixture = metadata(
    packageFixture("sample_core", { library: true }),
    packageFixture("sample_cli", { packageDependencies: [workspaceDependency("sample_core"), workspaceDependency("sample_mcp", { kind: "dev" })] }),
    packageFixture("sample_mcp", { packageDependencies: [workspaceDependency("sample_core")] }),
  );
  assert.ok(validateMetadata(fixture).some((error) => error.includes("不得依赖 adapter sample_mcp")));
});

test("rejects interface framework in core production, build, or dev", () => {
  const fixture = metadata(
    packageFixture("sample_core", { library: true, packageDependencies: [
      dependency("clap"), dependency("tauri-plugin-shell", { kind: "build" }), dependency("tuirealm", { kind: "dev" }),
    ] }),
    packageFixture("sample_cli", { packageDependencies: [workspaceDependency("sample_core")] }),
  );
  const errors = validateMetadata(fixture);
  for (const name of ["clap", "tauri-plugin-shell", "tuirealm"]) {
    assert.ok(errors.some((error) => error.includes(`接口框架 ${name}`)), JSON.stringify(errors));
  }
});

test("rejects interface framework in core dev tests", () => {
  const fixture = metadata(
    packageFixture("sample_core", { library: true, packageDependencies: [dependency("clap", { kind: "dev" })] }),
    packageFixture("sample_cli", { packageDependencies: [workspaceDependency("sample_core")] }),
  );
  assert.ok(validateMetadata(fixture).some((error) => error.includes("接口框架 clap")));
});

test("rejects multiple cores and foreign adapter prefix", () => {
  const multiple = metadata(
    packageFixture("sample_core", { library: true }),
    packageFixture("other_core", { library: true }),
    packageFixture("sample_cli", { packageDependencies: [workspaceDependency("sample_core")] }),
  );
  assert.ok(validateMetadata(multiple).some((error) => error.includes("只能包含一个")));
  const foreign = metadata(
    packageFixture("sample_core", { library: true }),
    packageFixture("other_cli", { packageDependencies: [workspaceDependency("sample_core")] }),
  );
  assert.ok(validateMetadata(foreign).some((error) => error.includes("项目标识 sample 不一致")));
});

test("rejects registry crate with same name as workspace core", () => {
  const fixture = metadata(
    packageFixture("sample_core", { library: true }),
    packageFixture("sample_cli", { packageDependencies: [dependency("sample_core")] }),
  );
  assert.ok(validateMetadata(fixture).some((error) => error.includes("workspace 内的 sample_core")));
});

test("rejects transitive interface framework from core", () => {
  const fixture = metadata(
    packageFixture("sample_core", { library: true, packageDependencies: [workspaceDependency("sample_infra")] }),
    packageFixture("sample_infra", { library: true, packageDependencies: [dependency("tauri")] }),
    packageFixture("sample_cli", { packageDependencies: [workspaceDependency("sample_core")] }),
  );
  const errors = validateMetadata(fixture);
  assert.ok(errors.some((error) => error.includes("sample_core -> sample_infra") && error.includes("接口框架 tauri")));
});

test("rejects transitive adapter-to-adapter dependency", () => {
  const fixture = metadata(
    packageFixture("sample_core", { library: true }),
    packageFixture("sample_bridge", { library: true, packageDependencies: [workspaceDependency("sample_mcp")] }),
    packageFixture("sample_cli", { packageDependencies: [workspaceDependency("sample_core"), workspaceDependency("sample_bridge")] }),
    packageFixture("sample_mcp", { packageDependencies: [workspaceDependency("sample_core")] }),
  );
  const errors = validateMetadata(fixture);
  assert.ok(errors.some((error) => error.includes("sample_cli -> sample_bridge -> sample_mcp") && error.includes("不得依赖 adapter sample_mcp")));
});

test("CLI returns zero for isolated Cargo metadata", posixOnly, () => withTemporaryRoot((root) => {
  writeFileSync(path.join(root, "Cargo.toml"), "[workspace]\n");
  const fixture = metadata(
    packageFixture("sample_core", { library: true }),
    packageFixture("sample_cli", { packageDependencies: [workspaceDependency("sample_core")] }),
  );
  const cargo = path.join(root, "fake-cargo");
  const argumentsFile = path.join(root, "args.txt");
  writeFileSync(cargo, "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$AFH_ARGS_FILE\"\nprintf '%s' \"$AFH_FAKE_METADATA\"\n");
  chmodSync(cargo, 0o755);
  const previousMetadata = process.env.AFH_FAKE_METADATA;
  const previousArgs = process.env.AFH_ARGS_FILE;
  process.env.AFH_FAKE_METADATA = JSON.stringify(fixture);
  process.env.AFH_ARGS_FILE = argumentsFile;
  try {
    const result = runMainJson("--workspace-root", root, "--cargo", cargo, "--timeout-seconds", "7");
    assert.equal(result.code, 0, result.stderr);
    assert.deepEqual(result.payload, { errors: [], ok: true, toolError: null });
    assert.equal(result.stdout, `${JSON.stringify({ errors: [], ok: true, toolError: null })}\n`);
    const cargoArgs = readFileSync(argumentsFile, "utf8");
    assert.doesNotMatch(cargoArgs, /--locked/);
    assert.match(cargoArgs, /metadata/);
  } finally {
    if (previousMetadata === undefined) delete process.env.AFH_FAKE_METADATA; else process.env.AFH_FAKE_METADATA = previousMetadata;
    if (previousArgs === undefined) delete process.env.AFH_ARGS_FILE; else process.env.AFH_ARGS_FILE = previousArgs;
  }
}));

test("CLI passes --locked to Cargo only when the workspace opts in", posixOnly, () => withTemporaryRoot((root) => {
  writeFileSync(path.join(root, "Cargo.toml"), "[workspace]\n[workspace.metadata.agent-first-harness]\ndependency-lock-policy = \"tracked\"\n");
  const fixture = metadata(
    packageFixture("sample_core", { library: true }),
    packageFixture("sample_cli", { packageDependencies: [workspaceDependency("sample_core")] }),
  );
  const cargo = path.join(root, "fake-cargo");
  const argumentsFile = path.join(root, "args.txt");
  writeFileSync(cargo, "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$AFH_ARGS_FILE\"\nprintf '%s' \"$AFH_FAKE_METADATA\"\n");
  chmodSync(cargo, 0o755);
  const prior = { metadata: process.env.AFH_FAKE_METADATA, args: process.env.AFH_ARGS_FILE };
  process.env.AFH_FAKE_METADATA = JSON.stringify(fixture);
  process.env.AFH_ARGS_FILE = argumentsFile;
  try {
    const result = runMainJson("--workspace-root", root, "--cargo", cargo);
    assert.equal(result.code, 0, result.stderr);
    assert.match(readFileSync(argumentsFile, "utf8"), /^--locked$/mu);
  } finally {
    if (prior.metadata === undefined) delete process.env.AFH_FAKE_METADATA; else process.env.AFH_FAKE_METADATA = prior.metadata;
    if (prior.args === undefined) delete process.env.AFH_ARGS_FILE; else process.env.AFH_ARGS_FILE = prior.args;
  }
}));

test("CLI rejects an invalid lock policy before Cargo runs", posixOnly, () => withTemporaryRoot((root) => {
  writeFileSync(path.join(root, "Cargo.toml"), "[workspace.metadata.agent-first-harness]\ndependency-lock-policy = true\n");
  const marker = path.join(root, "cargo-ran");
  const cargo = path.join(root, "fake-cargo");
  writeFileSync(cargo, `#!/bin/sh\ntouch "${marker}"\n`);
  chmodSync(cargo, 0o755);
  const result = runMainJson("--workspace-root", root, "--cargo", cargo);
  assert.equal(result.code, 2);
  assert.match(result.payload.toolError, /dependency-lock-policy/u);
  assert.equal(existsSync(marker), false);
}));

test("CLI returns one for architecture violation", () => withTemporaryRoot((root) => {
  const fixture = metadata(
    packageFixture("sample_core", { library: true }),
    packageFixture("sample_cli", { packageDependencies: [dependency("sample_core")] }),
  );
  const metadataPath = path.join(root, "metadata.json");
  writeFileSync(metadataPath, JSON.stringify(fixture));
  const result = runMainJson("--metadata-file", metadataPath);
  assert.equal(result.code, 1);
  assert.equal(result.payload.ok, false);
  assert.equal(result.payload.toolError, null);
  assert.ok(result.payload.errors.some((item) => item.includes("workspace 内的 sample_core")));
  assert.equal(result.stderr, "");
}));

test("CLI returns two for JSON and Cargo tool failures", () => withTemporaryRoot((root) => {
  const malformed = path.join(root, "metadata.json");
  writeFileSync(malformed, "{");
  let result = runMainJson("--metadata-file", malformed);
  assert.equal(result.code, 2);
  assert.equal(result.payload.ok, false);
  assert.deepEqual(result.payload.errors, []);
  assert.ok(result.payload.toolError);

  result = runMainJson("--timeout-seconds", "0");
  assert.equal(result.code, 2);
  assert.deepEqual(result.payload, { errors: [], ok: false, toolError: "--timeout-seconds 必须为正整数" });

  for (const timeout of ["0x10", "1e2"]) {
    result = runMainJson("--timeout-seconds", timeout);
    assert.equal(result.code, 2);
    assert.deepEqual(result.payload, { errors: [], ok: false, toolError: "--timeout-seconds 必须为正整数" });
  }
}));

test("CLI reports a Cargo timeout as a tool failure", posixOnly, () => withTemporaryRoot((root) => {
  writeFileSync(path.join(root, "Cargo.toml"), "[workspace]\n");
  const cargo = path.join(root, "slow-cargo");
  writeFileSync(cargo, "#!/bin/sh\nsleep 2\n");
  chmodSync(cargo, 0o755);
  const result = runMainJson("--workspace-root", root, "--cargo", cargo, "--timeout-seconds", "1");
  assert.equal(result.code, 2);
  assert.deepEqual(result.payload, { errors: [], ok: false, toolError: "cargo metadata 在 1 秒后超时" });
  assert.equal(result.stderr, "");
}));

test("CLI rejects metadata files that are not strict UTF-8", () => withTemporaryRoot((root) => {
  const metadataPath = path.join(root, "metadata.json");
  const prefix = Buffer.from('{"note":"');
  const suffix = Buffer.from('","packages":[],"workspace_members":[]}');
  writeFileSync(metadataPath, Buffer.concat([prefix, Buffer.from([0xff]), suffix]));
  const result = runMainJson("--metadata-file", metadataPath);
  assert.equal(result.code, 2);
  assert.equal(result.payload.ok, false);
  assert.deepEqual(result.payload.errors, []);
  assert.match(result.payload.toolError, /UTF-8|utf-8/i);
  assert.equal(result.stderr, "");
}));
