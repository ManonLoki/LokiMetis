#!/bin/sh
set -eu

# 探测 macOS Developer ID 签名、公证与 stapling 是否能作为一个完整候选阶段执行。
PROBE_PATH=${AFH_PREREQ_PATH:-${PATH}}
TEST_PLATFORM=${AFH_TEST_PLATFORM:-}

# 只允许隔离测试显式覆盖宿主，避免生产调用把非 macOS 伪装成 Apple 设备。
host_platform() {
    if [ -n "$TEST_PLATFORM" ]; then
        [ "${AFH_ALLOW_TEST_OVERRIDES:-0}" = 1 ] || {
            printf '%s\n' "错误：测试宿主覆盖未获允许" >&2
            exit 2
        }
        printf '%s\n' "$TEST_PLATFORM"
        return
    fi
    uname -s
}

# 仅在受控探测路径解析工具，测试不会读取真实钥匙串或 Xcode。
find_tool() {
    tool_name=$1
    old_ifs=$IFS
    IFS=:
    for tool_dir in $PROBE_PATH; do
        [ -n "$tool_dir" ] || tool_dir=.
        if [ -x "$tool_dir/$tool_name" ] && [ ! -d "$tool_dir/$tool_name" ]; then
            printf '%s\n' "$tool_dir/$tool_name"
            IFS=$old_ifs
            return 0
        fi
    done
    IFS=$old_ifs
    return 1
}

# 输出不含秘密的稳定不可用结论，调用方据此选择 unsigned 或渠道阻断。
unavailable() {
    reason=$1
    printf '%s\n' "gate.macos_notarization.status=unavailable"
    printf 'gate.macos_notarization.reason=%s\n' "$reason"
    exit 3
}

[ "$(host_platform)" = Darwin ] || unavailable "not-macos-apple-device"

security_path=$(find_tool security 2>/dev/null || true)
xcrun_path=$(find_tool xcrun 2>/dev/null || true)
[ -n "$security_path" ] || unavailable "security-tool-missing"
[ -n "$xcrun_path" ] || unavailable "xcrun-missing"

identity_output=$("$security_path" find-identity -v -p codesigning 2>/dev/null || true)
identity_count=$(printf '%s\n' "$identity_output" | awk '/Developer ID Application/ { count += 1 } END { print count + 0 }')
[ "$identity_count" -gt 0 ] || unavailable "developer-id-application-missing"
if [ "$identity_count" -gt 1 ] && [ -z "${APPLE_SIGNING_IDENTITY:-}" ]; then
    unavailable "developer-id-application-ambiguous"
fi
if [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
    printf '%s\n' "$identity_output" | grep -F -- "${APPLE_SIGNING_IDENTITY}" >/dev/null 2>&1 ||
        unavailable "configured-signing-identity-not-found"
fi

"$xcrun_path" --find notarytool >/dev/null 2>&1 || unavailable "notarytool-missing"
"$xcrun_path" --find stapler >/dev/null 2>&1 || unavailable "stapler-missing"

api_count=0
[ -n "${APPLE_API_ISSUER:-}" ] && api_count=$((api_count + 1))
[ -n "${APPLE_API_KEY:-}" ] && api_count=$((api_count + 1))
[ -n "${APPLE_API_KEY_PATH:-}" ] && api_count=$((api_count + 1))
apple_id_count=0
[ -n "${APPLE_ID:-}" ] && apple_id_count=$((apple_id_count + 1))
[ -n "${APPLE_PASSWORD:-}" ] && apple_id_count=$((apple_id_count + 1))
[ -n "${APPLE_TEAM_ID:-}" ] && apple_id_count=$((apple_id_count + 1))
keychain_profile_count=0
[ -n "${APPLE_NOTARYTOOL_PROFILE:-}" ] && keychain_profile_count=1

credential_mode=
if [ "$api_count" -eq 3 ] && [ "$apple_id_count" -eq 0 ] && [ "$keychain_profile_count" -eq 0 ]; then
    [ -f "${APPLE_API_KEY_PATH}" ] && [ -r "${APPLE_API_KEY_PATH}" ] && [ ! -L "${APPLE_API_KEY_PATH}" ] ||
        unavailable "api-private-key-unreadable"
    credential_mode=app-store-connect-api
elif [ "$apple_id_count" -eq 3 ] && [ "$api_count" -eq 0 ] && [ "$keychain_profile_count" -eq 0 ]; then
    credential_mode=apple-id
elif [ "$keychain_profile_count" -eq 1 ] && [ "$api_count" -eq 0 ] && [ "$apple_id_count" -eq 0 ]; then
    "$xcrun_path" notarytool history \
        --keychain-profile "$APPLE_NOTARYTOOL_PROFILE" \
        --output-format json >/dev/null 2>&1 || unavailable "notarytool-keychain-profile-unavailable"
    credential_mode=notarytool-keychain-profile
elif [ "$api_count" -eq 0 ] && [ "$apple_id_count" -eq 0 ] && [ "$keychain_profile_count" -eq 0 ]; then
    unavailable "notarization-credentials-missing"
else
    unavailable "notarization-credentials-incomplete-or-ambiguous"
fi

printf '%s\n' "gate.macos_notarization.status=ready"
printf '%s\n' "gate.macos_notarization.reason=all-required-conditions-present"
printf '%s\n' "gate.macos_notarization.signing_identity=developer-id-application"
printf 'gate.macos_notarization.credentials=%s\n' "$credential_mode"
printf '%s\n' "gate.macos_notarization.notarytool=available"
printf '%s\n' "gate.macos_notarization.stapler=available"
