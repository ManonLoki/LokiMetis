import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  symlinkSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";

import {
  executable,
  fakeExistingTools,
  fakeFrontendTools,
  fakeGitPackageManager,
  fakeNpmInstaller,
  makeNodeDist,
  makeRustDist,
  runGate,
} from "./environment_gates_fixture.mjs";

const posixOnly = { skip: process.platform === "win32" };

function withTemporaryRoot(callback) {
  const root = mkdtempSync(path.join(tmpdir(), "afh-env-security-"));
  try { callback(root); } finally { rmSync(root, { recursive: true, force: true }); }
}

test("user tool directory symlinks fail before installation", posixOnly, () => withTemporaryRoot((root) => {
  const home = path.join(root, "home");
  mkdirSync(home);
  const redirected = path.join(root, "redirected-local");
  mkdirSync(redirected);
  symlinkSync(redirected, path.join(home, ".local"), "dir");
  const probe = path.join(root, "probe");
  fakeExistingTools(probe);
  const result = runGate(root, ["--install-missing", "--interfaces", "GUI"], { probe });
  assert.equal(result.status, 24, result.stderr);
  assert.match(result.stderr, /不能是符号链接/);
  assert.deepEqual(readdirSync(redirected), []);
}));

test("pnpm package path cannot escape through a symlink", posixOnly, () => withTemporaryRoot((root) => {
  const local = path.join(root, "home", ".local");
  mkdirSync(local, { recursive: true });
  const outside = path.join(root, "outside-node-modules");
  mkdirSync(outside);
  symlinkSync(outside, path.join(local, "lib"), "dir");
  const probe = path.join(root, "probe");
  fakeExistingTools(probe);
  fakeFrontendTools(probe, { pnpm: "12.4.0" });
  const result = runGate(root, ["--install-missing", "--interfaces", "GUI"], { probe });
  assert.equal(result.status, 24, result.stderr);
  assert.match(result.stderr, /路径组件不能是符号链接/);
  assert.deepEqual(readdirSync(outside), []);
}));

test("existing pnpm wrappers cannot target outside the user prefix", posixOnly, () => {
  for (const wrapper of ["pnpm", "pnpx"]) withTemporaryRoot((root) => {
    const userBin = path.join(root, "home", ".local", "bin");
    mkdirSync(userBin, { recursive: true });
    const outside = path.join(root, `outside-${wrapper}`);
    writeFileSync(outside, `outside-${wrapper}-must-remain\n`);
    symlinkSync(outside, path.join(userBin, wrapper));
    const probe = path.join(root, "probe");
    fakeExistingTools(probe);
    fakeFrontendTools(probe, { pnpm: "12.4.0" });
    const result = runGate(root, ["--install-missing", "--interfaces", "GUI"], { probe });
    assert.equal(result.status, 24, result.stderr);
    assert.match(result.stderr, /不属于当前受管安装根/);
    assert.equal(readFileSync(outside, "utf8"), `outside-${wrapper}-must-remain\n`);
  });
});

test("a newly installed pnpm wrapper is checked for prefix escape", posixOnly, () => withTemporaryRoot((root) => {
  const probe = path.join(root, "probe");
  fakeExistingTools(probe);
  fakeFrontendTools(probe, { pnpm: "12.4.0" });
  const outside = path.join(root, "outside-pnpm");
  executable(outside, "#!/bin/sh\nprintf '%s\\n' '12.4.1'\n");
  const result = runGate(root, ["--install-missing", "--interfaces", "GUI"], {
    probe,
    extra: { AFH_TEST_PNPM_LINK_TARGET: outside },
  });
  assert.equal(result.status, 24, result.stderr);
  assert.match(result.stderr, /包装器逃逸标准当前用户前缀/);
  assert.equal(existsSync(path.join(root, "home", ".profile")), false);
}));

test("an unmanaged node symlink is never replaced", posixOnly, () => withTemporaryRoot((root) => {
  const probe = path.join(root, "probe");
  fakeExistingTools(probe, { includeNode: false });
  fakeNpmInstaller(probe);
  const nodeDist = makeNodeDist(path.join(root, "node-dist"));
  const userBin = path.join(root, "home", ".local", "bin");
  mkdirSync(userBin, { recursive: true });
  const outside = path.join(root, "outside-node");
  executable(outside, "#!/bin/sh\nprintf '%s\\n' 'v99.0.0'\n");
  symlinkSync(outside, path.join(userBin, "node"));
  const result = runGate(root, ["--install-missing"], {
    probe,
    extra: { AFH_NODE_DIST_BASE: nodeDist, AFH_ALLOW_FILE_URLS: "1" },
  });
  assert.equal(result.status, 24, result.stderr);
  assert.match(result.stderr, /不属于当前受管安装根/);
  assert.equal(existsSync(path.join(root, "home", ".local", "lib", "nodejs")), false);
}));

test("managed Rust root symlinks fail before download", posixOnly, () => withTemporaryRoot((root) => {
  mkdirSync(path.join(root, "home"));
  const outside = path.join(root, "outside-cargo");
  mkdirSync(outside);
  symlinkSync(outside, path.join(root, "home", ".cargo"), "dir");
  const probe = path.join(root, "probe");
  fakeExistingTools(probe);
  for (const tool of ["rustup", "rustc", "cargo"]) unlinkSync(path.join(probe, tool));
  const result = runGate(root, ["--install-missing"], {
    probe,
    extra: {
      AFH_RUSTUP_DIST_BASE: new URL(`file://${path.join(root, "unreachable-rust-dist")}`).href,
      AFH_ALLOW_FILE_URLS: "1",
    },
  });
  assert.equal(result.status, 24, result.stderr);
  assert.match(result.stderr, /路径组件不能是符号链接/);
  assert.deepEqual(readdirSync(outside), []);
}));

test("unmanaged Node version roots are rejected before network access", posixOnly, () => withTemporaryRoot((root) => {
  const probe = path.join(root, "probe");
  fakeExistingTools(probe);
  fakeFrontendTools(probe, { node: "24.20.9" });
  const existingBin = path.join(root, "home", ".local", "lib", "nodejs", "v23.0.0", "bin");
  mkdirSync(existingBin, { recursive: true });
  executable(path.join(existingBin, "node"), "#!/bin/sh\nprintf '%s\\n' 'v23.0.0'\n");
  fakeNpmInstaller(existingBin);
  const result = runGate(root, ["--install-missing"], {
    probe,
    extra: {
      AFH_NODE_DIST_BASE: new URL(`file://${path.join(root, "unreachable-node-dist")}`).href,
      AFH_ALLOW_FILE_URLS: "1",
    },
  });
  assert.equal(result.status, 24, result.stderr);
  assert.match(result.stderr, /缺少受管所有权标记/);
  assert.doesNotMatch(result.stderr, /发布版本索引下载失败/);
}));

test("custom standard Rust homes are honored", posixOnly, () => withTemporaryRoot((root) => {
  const probe = path.join(root, "probe");
  fakeExistingTools(probe);
  for (const tool of ["rustup", "rustc", "cargo"]) unlinkSync(path.join(probe, tool));
  const rustDist = makeRustDist(path.join(root, "rust-dist"));
  const cargoHome = path.join(root, "home", "toolchains", "cargo");
  const rustupHome = path.join(root, "home", "toolchains", "rustup");
  const result = runGate(root, ["--install-missing"], {
    probe,
    extra: {
      AFH_RUSTUP_DIST_BASE: rustDist,
      AFH_ALLOW_FILE_URLS: "1",
      AFH_SKIP_PERSIST_PATH: "1",
      CARGO_HOME: cargoHome,
      RUSTUP_HOME: rustupHome,
    },
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(existsSync(path.join(cargoHome, "bin", "cargo")), true);
  assert.equal(existsSync(rustupHome), true);
}));

test("fresh shell rechecks unchanged Node and pnpm after a Rust upgrade", posixOnly, () => withTemporaryRoot((root) => {
  const probe = path.join(root, "probe");
  fakeExistingTools(probe, { rust: "1.98.0" });
  fakeFrontendTools(probe);
  const rustDist = makeRustDist(path.join(root, "rust-dist"));
  const result = runGate(root, ["--install-missing", "--interfaces", "GUI"], {
    probe,
    extra: {
      AFH_RUSTUP_DIST_BASE: rustDist,
      AFH_ALLOW_FILE_URLS: "1",
      AFH_TEST_SYSTEM_PATH: "/usr/bin:/bin:/usr/sbin:/sbin",
    },
  });
  assert.equal(result.status, 24, result.stderr);
  assert.match(result.stderr, /新 shell 无法从持久 PATH/);
}));

test("every unchanged tool must be identical in the persistent shell before installation", posixOnly, () => {
  const cases = {
    git: ["git"],
    rust: ["rustup", "rustc", "cargo"],
    node: ["node"],
    npm: ["npm"],
    pnpm: ["pnpm"],
  };
  for (const [target, transientTools] of Object.entries(cases)) withTemporaryRoot((root) => {
    const persistent = path.join(root, "persistent");
    const transient = path.join(root, "transient");
    fakeExistingTools(persistent, { rust: target === "pnpm" ? "1.98.0" : "1.98.1" });
    fakeFrontendTools(persistent, { pnpm: target === "pnpm" ? "12.4.1" : "12.4.0" });
    mkdirSync(transient);
    for (const tool of transientTools) {
      copyFileSync(path.join(persistent, tool), path.join(transient, tool));
      unlinkSync(path.join(persistent, tool));
    }
    const result = runGate(root, ["--install-missing", "--interfaces", "GUI"], {
      probe: persistent,
      extra: {
        AFH_PREREQ_PATH: `${transient}:${persistent}`,
        AFH_TEST_SYSTEM_PATH: `${persistent}:/usr/bin:/bin:/usr/sbin:/sbin`,
        AFH_RUSTUP_DIST_BASE: new URL(`file://${path.join(root, "unreachable-rust-dist")}`).href,
        AFH_ALLOW_FILE_URLS: "1",
      },
    });
    assert.equal(result.status, 24, `${target}: ${result.stderr}`);
    assert.match(result.stderr, /写入前新 shell/);
    assert.equal(existsSync(path.join(root, "home", ".profile")), false);
    assert.equal(existsSync(path.join(root, "home", ".cargo")), false);
    assert.equal(existsSync(path.join(root, "home", ".local")), false);
  });
});

test("profile shadows are rejected before package installation", posixOnly, () => {
  for (const pendingGit of [false, true]) withTemporaryRoot((root) => {
    const home = path.join(root, "home");
    mkdirSync(home);
    const probe = path.join(root, "probe");
    fakeExistingTools(probe, { git: pendingGit ? "2.35.8" : "2.39.0" });
    if (pendingGit) fakeGitPackageManager(probe);
    else fakeFrontendTools(probe, { pnpm: "12.4.0" });
    const shadow = path.join(home, "shadow");
    mkdirSync(shadow);
    executable(path.join(shadow, "git"), "#!/bin/sh\nprintf '%s\\n' 'git version 2.34.1'\n");
    const profile = path.join(home, ".profile");
    const profileText = `PATH="${shadow}:$PATH"\nexport PATH\n`;
    writeFileSync(profile, profileText);
    const args = pendingGit ? ["--install-missing"] : ["--install-missing", "--interfaces", "GUI"];
    const result = runGate(root, args, { probe });
    assert.equal(result.status, 24, result.stderr);
    assert.match(result.stderr, /既有工具 git/);
    assert.equal(readFileSync(profile, "utf8"), profileText);
    assert.equal(existsSync(path.join(home, ".local")), false);
    if (pendingGit) {
      const version = spawnSync(path.join(probe, "git"), ["--version"], { encoding: "utf8" });
      assert.equal(version.stdout.trim(), "git version 2.35.8");
    }
  });
});

test("a missing current Node cannot ignore a different persisted version", posixOnly, () => withTemporaryRoot((root) => {
  const home = path.join(root, "home");
  mkdirSync(home);
  const probe = path.join(root, "probe");
  fakeExistingTools(probe);
  unlinkSync(path.join(probe, "node"));
  unlinkSync(path.join(probe, "npm"));
  const persistent = path.join(home, "persistent-node");
  mkdirSync(persistent);
  executable(path.join(persistent, "node"), "#!/bin/sh\nprintf '%s\\n' 'v26.8.2'\n");
  fakeNpmInstaller(persistent);
  const profile = path.join(home, ".profile");
  const profileText = `PATH="${persistent}:$PATH"\nexport PATH\n`;
  writeFileSync(profile, profileText);
  const result = runGate(root, ["--install-missing", "--interfaces", "GUI"], { probe });
  assert.equal(result.status, 24, result.stderr);
  assert.match(result.stderr, /既有工具 node/);
  assert.equal(readFileSync(profile, "utf8"), profileText);
  assert.equal(existsSync(path.join(home, ".local")), false);
}));

test("projected user PATH entries cannot shadow an already-passed Node", posixOnly, () => withTemporaryRoot((root) => {
  const userBin = path.join(root, "home", ".local", "bin");
  mkdirSync(userBin, { recursive: true });
  executable(path.join(userBin, "node"), "#!/bin/sh\nprintf '%s\\n' 'v22.0.0'\n");
  fakeNpmInstaller(userBin);
  const probe = path.join(root, "probe");
  fakeExistingTools(probe);
  fakeFrontendTools(probe, { node: "26.8.2", pnpm: "12.4.0" });
  const result = runGate(root, ["--install-missing", "--interfaces", "GUI"], { probe });
  assert.equal(result.status, 24, result.stderr);
  assert.match(result.stderr, /既有工具 node/);
  assert.equal(existsSync(path.join(root, "home", ".profile")), false);
  assert.equal(existsSync(path.join(userBin, "pnpm")), false);
}));

test("existing Node version roots require an exact marker and contained executables", posixOnly, () => {
  for (const scenario of ["missing-marker", "escaping-node", "wrong-version"]) withTemporaryRoot((root) => {
    const probe = path.join(root, "probe");
    fakeExistingTools(probe);
    unlinkSync(path.join(probe, "node"));
    unlinkSync(path.join(probe, "npm"));
    const selectedVersion = scenario === "wrong-version" ? "v26.7.0" : "v24.21.0";
    makeNodeDist(path.join(root, "node-dist"), { version: selectedVersion });
    const release = path.join(root, "node-dist", selectedVersion);
    const archiveName = readdirSync(release).find((name) => name.endsWith(".tar.gz"));
    assert.ok(archiveName);
    const archive = path.join(release, archiveName);
    const digest = createHash("sha256").update(readFileSync(archive)).digest("hex");
    rmSync(archive);
    const versionRoot = path.join(root, "home", ".local", "lib", "nodejs", selectedVersion);
    const installBin = path.join(versionRoot, "bin");
    mkdirSync(installBin, { recursive: true });
    const reportedVersion = scenario === "wrong-version" ? "v24.21.0" : selectedVersion;
    executable(path.join(installBin, "node"), `#!/bin/sh\nprintf '%s\\n' '${reportedVersion}'\n`);
    fakeNpmInstaller(installBin);
    if (scenario !== "missing-marker") {
      writeFileSync(path.join(versionRoot, ".agent-first-harness-managed"),
        `# managed by agent-first-harness development environment gate\nnode.version=${selectedVersion}\nnode.archive.sha256=${digest}\n`);
    }
    if (scenario === "escaping-node") {
      const outside = path.join(root, "outside-node");
      executable(outside, `#!/bin/sh\nprintf '%s\\n' '${selectedVersion}'\n`);
      unlinkSync(path.join(installBin, "node"));
      symlinkSync(outside, path.join(installBin, "node"));
    }
    const result = runGate(root, ["--install-missing", "--interfaces", "GUI"], {
      probe,
      extra: {
        AFH_NODE_DIST_BASE: new URL(`file://${path.join(root, "node-dist")}`).href,
        AFH_ALLOW_FILE_URLS: "1",
      },
    });
    assert.ok([24, 26].includes(result.status), `${scenario}: ${result.stderr}`);
    if (scenario === "wrong-version") assert.match(result.stderr, /node 版本与目录名称不一致/);
    assert.equal(existsSync(path.join(root, "home", ".local", "bin", "node")), false);
  });
});

test("profile updates preserve concurrent changes and fail closed", posixOnly, (context) => {
  const located = spawnSync("sh", ["-c", "command -v cmp"], { encoding: "utf8" }).stdout.trim();
  if (!located) {
    context.skip("requires cmp");
    return;
  }
  withTemporaryRoot((root) => {
    const home = path.join(root, "home");
    mkdirSync(home);
    const profile = path.join(home, ".profile");
    writeFileSync(profile, "owner\n");
    const probe = path.join(root, "probe");
    fakeExistingTools(probe);
    for (const tool of ["rustup", "rustc", "cargo"]) unlinkSync(path.join(probe, tool));
    const rustDist = makeRustDist(path.join(root, "rust-dist"));
    const shims = path.join(root, "path-shims");
    mkdirSync(shims);
    executable(path.join(shims, "cmp"), `#!/bin/sh\nprintf '%s\\n' concurrent >> "$AFH_CONCURRENT_PROFILE"\nexec "${located}" "$@"\n`);
    const result = runGate(root, ["--install-missing"], {
      probe,
      extra: {
        AFH_RUSTUP_DIST_BASE: rustDist,
        AFH_ALLOW_FILE_URLS: "1",
        AFH_CONCURRENT_PROFILE: profile,
        PATH: `${shims}:${process.env.PATH ?? ""}`,
      },
    });
    assert.equal(result.status, 24, result.stderr);
    assert.match(result.stderr, /并发修改/);
    assert.equal(readFileSync(profile, "utf8"), "owner\nconcurrent\n");
    assert.doesNotMatch(readFileSync(profile, "utf8"), /agent-first-harness\/env\.sh/);
  });
});

test("a Git-only change still requires exact fresh login-shell discovery", posixOnly, () => {
  for (const probeIsPersistent of [true, false]) withTemporaryRoot((root) => {
    const probe = path.join(root, "probe");
    fakeExistingTools(probe, { git: "2.35.8" });
    fakeGitPackageManager(probe);
    const loginShell = path.join(root, "login", "sh");
    executable(loginShell, "#!/bin/sh\n[ \"${1:-}\" = -l ] && [ \"${2:-}\" = -c ] || exit 90\neval \"$3\"\n");
    const basePath = probeIsPersistent ? `${probe}:${process.env.PATH ?? ""}` : (process.env.PATH ?? "");
    const result = runGate(root, ["--install-missing"], {
      probe,
      extra: {
        SHELL: loginShell,
        PATH: basePath,
        AFH_TEST_SYSTEM_PATH: probeIsPersistent ? `${probe}:/usr/bin:/bin:/usr/sbin:/sbin` : "/usr/bin:/bin:/usr/sbin:/sbin",
      },
    });
    if (probeIsPersistent) {
      assert.equal(result.status, 0, result.stderr);
      assert.match(result.stdout, /gate\.git\.change=upgraded/);
      assert.match(result.stdout, /gate\.fresh_shell\.status=passed/);
    } else {
      assert.equal(result.status, 24, result.stderr);
      assert.match(result.stderr, /新 shell 无法从持久 PATH/);
    }
  });
});
