#!/usr/bin/env python3
from __future__ import annotations

import os
import subprocess
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("probe-macos-notarization.sh")


def executable(path: Path, content: str) -> None:
    """创建隔离工具替身，确保测试不读取真实钥匙串或 Xcode。"""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")
    path.chmod(0o755)


def fake_apple_tools(
    root: Path,
    *,
    identities: int = 1,
    include_notarytool: bool = True,
    profile_ready: bool = True,
) -> Path:
    """建立可控的 Developer ID 与 xcrun 探测结果，不包含真实身份或凭据。"""
    probe = root / "probe"
    identity_lines = "\n".join(
        f'  {index + 1}) TESTHASH{index} "Developer ID Application: Test Org {index} (TEAMTEST)"'
        for index in range(identities)
    )
    executable(
        probe / "security",
        f"#!/bin/sh\nprintf '%s\\n' '{identity_lines}'\n",
    )
    notary_status = "exit 0" if include_notarytool else 'case "$2" in notarytool) exit 1 ;; *) exit 0 ;; esac'
    profile_status = "exit 0" if profile_ready else "exit 9"
    executable(
        probe / "xcrun",
        f"""#!/bin/sh
set -eu
if [ "$1" = "--find" ]; then
  {notary_status}
elif [ "$1 $2" = "notarytool history" ]; then
  {profile_status}
else
  exit 2
fi
""",
    )
    return probe


class MacosNotarizationProbeTests(unittest.TestCase):
    """验证签名、公证与 stapling 原子可用性探测不泄露秘密或接受部分状态。"""

    def run_probe(
        self,
        root: Path,
        probe: Path,
        **extra: str,
    ) -> subprocess.CompletedProcess[str]:
        """使用隔离凭据和伪 macOS 宿主运行探测。"""
        env = os.environ.copy()
        env.update(
            {
                "AFH_PREREQ_PATH": str(probe),
                "AFH_TEST_PLATFORM": "Darwin",
                "AFH_ALLOW_TEST_OVERRIDES": "1",
            }
        )
        for key in (
            "APPLE_API_ISSUER",
            "APPLE_API_KEY",
            "APPLE_API_KEY_PATH",
            "APPLE_ID",
            "APPLE_PASSWORD",
            "APPLE_TEAM_ID",
            "APPLE_SIGNING_IDENTITY",
            "APPLE_NOTARYTOOL_PROFILE",
        ):
            env.pop(key, None)
        env.update(extra)
        return subprocess.run(
            ["/bin/sh", str(SCRIPT)],
            text=True,
            capture_output=True,
            env=env,
            timeout=10,
            check=False,
        )

    def test_complete_api_credentials_are_ready_without_secret_output(self) -> None:
        """完整 API 凭据、Developer ID 和 Xcode 工具存在时应通过且不输出值。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = fake_apple_tools(root)
            key = root / "AuthKey_TEST.p8"
            secret = "TOP-SECRET-PRIVATE-KEY"
            key.write_text(secret, encoding="utf-8")
            result = self.run_probe(
                root,
                probe,
                APPLE_API_ISSUER="issuer-secret",
                APPLE_API_KEY="key-secret",
                APPLE_API_KEY_PATH=str(key),
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("gate.macos_notarization.status=ready", result.stdout)
            self.assertIn("credentials=app-store-connect-api", result.stdout)
            combined = result.stdout + result.stderr
            self.assertNotIn(secret, combined)
            self.assertNotIn("issuer-secret", combined)
            self.assertNotIn("key-secret", combined)
            self.assertNotIn(str(key), combined)

    def test_complete_apple_id_credentials_are_ready(self) -> None:
        """完整 Apple ID 三元组应作为另一种互斥凭据模式通过。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = fake_apple_tools(root)
            result = self.run_probe(
                root,
                probe,
                APPLE_ID="test@example.invalid",
                APPLE_PASSWORD="app-password-secret",
                APPLE_TEAM_ID="TEAMTEST",
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("credentials=apple-id", result.stdout)
            self.assertNotIn("app-password-secret", result.stdout + result.stderr)

    def test_authorized_keychain_profile_is_ready_without_profile_output(self) -> None:
        """既有 notarytool Keychain profile 在线可用时应通过且不输出 profile 名。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            profile = "secret-profile-name"
            result = self.run_probe(
                root,
                fake_apple_tools(root),
                APPLE_NOTARYTOOL_PROFILE=profile,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("credentials=notarytool-keychain-profile", result.stdout)
            self.assertNotIn(profile, result.stdout + result.stderr)

    def test_unavailable_keychain_profile_is_rejected(self) -> None:
        """Keychain profile 无法在线使用时必须拒绝，不得留下仅签名候选。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            result = self.run_probe(
                root,
                fake_apple_tools(root, profile_ready=False),
                APPLE_NOTARYTOOL_PROFILE="missing-profile",
            )
            self.assertEqual(result.returncode, 3)
            self.assertIn("keychain-profile-unavailable", result.stdout)

    def test_missing_credentials_is_unavailable_not_partially_ready(self) -> None:
        """只有签名身份而没有公证凭据时必须拒绝只签名中间态。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            result = self.run_probe(root, fake_apple_tools(root))
            self.assertEqual(result.returncode, 3)
            self.assertIn("notarization-credentials-missing", result.stdout)

    def test_partial_or_mixed_credentials_are_rejected(self) -> None:
        """部分凭据或同时混用两套方式必须视为歧义，不得猜测认证路径。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            result = self.run_probe(
                root,
                fake_apple_tools(root),
                APPLE_ID="test@example.invalid",
                APPLE_API_ISSUER="issuer",
            )
            self.assertEqual(result.returncode, 3)
            self.assertIn("incomplete-or-ambiguous", result.stdout)

    def test_keychain_profile_cannot_be_mixed_with_environment_credentials(self) -> None:
        """Keychain profile 与环境变量凭据同时存在时必须拒绝，不能猜测认证路径。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            result = self.run_probe(
                root,
                fake_apple_tools(root),
                APPLE_NOTARYTOOL_PROFILE="profile",
                APPLE_ID="test@example.invalid",
                APPLE_PASSWORD="password",
                APPLE_TEAM_ID="TEAMTEST",
            )
            self.assertEqual(result.returncode, 3)
            self.assertIn("incomplete-or-ambiguous", result.stdout)

    def test_symlinked_api_key_is_rejected(self) -> None:
        """API 私钥路径为符号链接时拒绝，避免探测跟随未审查路径。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = fake_apple_tools(root)
            target = root / "real-key.p8"
            target.write_text("secret", encoding="utf-8")
            link = root / "key-link.p8"
            link.symlink_to(target)
            result = self.run_probe(
                root,
                probe,
                APPLE_API_ISSUER="issuer",
                APPLE_API_KEY="key",
                APPLE_API_KEY_PATH=str(link),
            )
            self.assertEqual(result.returncode, 3)
            self.assertIn("api-private-key-unreadable", result.stdout)

    def test_multiple_identities_require_an_explicit_selection(self) -> None:
        """多个 Developer ID 身份且未显式选择时必须拒绝歧义。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = fake_apple_tools(root, identities=2)
            result = self.run_probe(
                root,
                probe,
                APPLE_ID="test@example.invalid",
                APPLE_PASSWORD="password",
                APPLE_TEAM_ID="TEAMTEST",
            )
            self.assertEqual(result.returncode, 3)
            self.assertIn("developer-id-application-ambiguous", result.stdout)

    def test_missing_notarytool_is_unavailable(self) -> None:
        """旧 Xcode 缺少 notarytool 时必须阻断完整阶段，不能回退 altool。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = fake_apple_tools(root, include_notarytool=False)
            result = self.run_probe(
                root,
                probe,
                APPLE_ID="test@example.invalid",
                APPLE_PASSWORD="password",
                APPLE_TEAM_ID="TEAMTEST",
            )
            self.assertEqual(result.returncode, 3)
            self.assertIn("notarytool-missing", result.stdout)

    def test_non_macos_host_is_unavailable(self) -> None:
        """非 Apple 宿主不能仅凭伪造工具路径宣称可进行 macOS 签名公证。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = fake_apple_tools(root)
            env = os.environ.copy()
            env.update(
                {
                    "AFH_PREREQ_PATH": str(probe),
                    "AFH_TEST_PLATFORM": "Linux",
                    "AFH_ALLOW_TEST_OVERRIDES": "1",
                }
            )
            result = subprocess.run(
                ["/bin/sh", str(SCRIPT)],
                text=True,
                capture_output=True,
                env=env,
                timeout=10,
                check=False,
            )
            self.assertEqual(result.returncode, 3)
            self.assertIn("not-macos-apple-device", result.stdout)


if __name__ == "__main__":
    unittest.main()
