#!/bin/sh
set -eu

# 只读挂载最终 DMG，确认 Finder 拖拽安装布局已经真实写入候选字节。
PROBE_PATH=${PATH}
TEST_PLATFORM=${AFH_TEST_PLATFORM:-}

# 只允许测试夹具覆盖宿主，生产调用必须来自真实 macOS。
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

# 只从显式探测路径选择外部工具，使隔离测试不会挂载真实镜像或偷用未声明解析器。
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

# 返回稳定失败原因；调用方不得把布局不完整的 DMG 继续包装为候选。
layout_failure() {
    reason=$1
    printf '%s\n' "gate.macos_dmg_layout.status=failed"
    printf 'gate.macos_dmg_layout.reason=%s\n' "$reason"
    exit 41
}

# 自定义工具路径只属于显式测试宿主；真实 Darwin 必须从调用环境的正常 PATH
# 解析系统 hdiutil，不能被残留的测试探针静默替换。
if [ -n "${AFH_PREREQ_PATH:-}" ]; then
    [ -n "$TEST_PLATFORM" ] && [ "${AFH_ALLOW_TEST_OVERRIDES:-0}" = 1 ] ||
        layout_failure "prerequisite-path-override-not-allowed"
    PROBE_PATH=$AFH_PREREQ_PATH
fi

[ "$#" -eq 2 ] || {
    printf '%s\n' "用法：verify-dmg-layout.sh <final.dmg> <release-notes.json>" >&2
    exit 2
}
[ "$(host_platform)" = Darwin ] || {
    printf '%s\n' "gate.macos_dmg_layout.status=not-applicable"
    printf '%s\n' "gate.macos_dmg_layout.reason=requires-macos-host"
    exit 30
}

dmg_path=$1
release_notes_path=$2
[ -f "$dmg_path" ] && [ ! -L "$dmg_path" ] || layout_failure "dmg-not-regular-file"
[ -f "$release_notes_path" ] && [ ! -L "$release_notes_path" ] ||
    layout_failure "release-notes-source-not-regular-file"
hdiutil_path=$(find_tool hdiutil 2>/dev/null || true)
[ -n "$hdiutil_path" ] || layout_failure "hdiutil-missing"
python3_path=$(find_tool python3 2>/dev/null || true)
[ -n "$python3_path" ] || layout_failure "layout-parser-runtime-missing"
script_dir=$(CDPATH= cd "$(dirname "$0")" && pwd -P)
layout_parser=$script_dir/inspect_dmg_layout.py
[ -f "$layout_parser" ] && [ ! -L "$layout_parser" ] ||
    layout_failure "layout-parser-missing"

inspection_root=$(mktemp -d "${TMPDIR:-/tmp}/afh-dmg-layout.XXXXXX")
mount_path=$inspection_root/volume
mkdir "$mount_path"
device=

# 无论检查在何处失败，都回收本次只读挂载与精确临时目录。
cleanup() {
    if [ -n "${device:-}" ]; then
        "$hdiutil_path" detach "$device" >/dev/null 2>&1 || true
    fi
    rm -rf "$inspection_root"
}
trap cleanup EXIT HUP INT TERM

attach_output=$("$hdiutil_path" attach -readonly -nobrowse -noautoopen -mountpoint "$mount_path" "$dmg_path" 2>/dev/null) ||
    layout_failure "readonly-attach-failed"
device=$(printf '%s\n' "$attach_output" | awk '/^\/dev\// { print $1; exit }')
[ -n "$device" ] || layout_failure "mounted-device-missing"

[ -s "$mount_path/.DS_Store" ] && [ ! -L "$mount_path/.DS_Store" ] ||
    layout_failure "finder-ds-store-missing"
[ -d "$mount_path/.background" ] && [ ! -L "$mount_path/.background" ] ||
    layout_failure "background-directory-invalid"
[ -s "$mount_path/.background/background.png" ] && [ ! -L "$mount_path/.background/background.png" ] ||
    layout_failure "background-image-missing"
[ -L "$mount_path/Applications" ] || layout_failure "applications-link-missing"
[ "$(readlink "$mount_path/Applications")" = /Applications ] ||
    layout_failure "applications-link-target-invalid"

app_count=0
bundled_app_path=
for app_path in "$mount_path"/*.app; do
    [ -e "$app_path" ] || continue
    [ -d "$app_path" ] && [ ! -L "$app_path" ] || layout_failure "app-bundle-not-directory"
    app_count=$((app_count + 1))
    bundled_app_path=$app_path
done
[ "$app_count" -eq 1 ] || layout_failure "app-bundle-count-invalid"
[ -d "$bundled_app_path/Contents" ] && [ ! -L "$bundled_app_path/Contents" ] ||
    layout_failure "app-contents-directory-invalid"
[ -d "$bundled_app_path/Contents/Resources" ] &&
    [ ! -L "$bundled_app_path/Contents/Resources" ] ||
    layout_failure "release-notes-resource-directory-invalid"

# 解析最终卷内 PNG 与 .DS_Store 的活动 B-tree，不能把非空占位文件当作真实 Finder 布局。
app_name=${bundled_app_path##*/}
if layout_output=$("$python3_path" "$layout_parser" \
    --background "$mount_path/.background/background.png" \
    --ds-store "$mount_path/.DS_Store" \
    --app-name "$app_name" \
    --width 660 --height 400 \
    --app-x 180 --app-y 220 \
    --applications-x 480 --applications-y 220); then
    :
else
    layout_reason=${layout_output#reason=}
    case "$layout_reason" in
        background-image-unparseable | background-dimensions-invalid | \
            finder-layout-unparseable | finder-window-size-invalid | \
            finder-background-binding-invalid | \
            app-icon-position-invalid | applications-icon-position-invalid)
            layout_failure "$layout_reason"
            ;;
        *) layout_failure "finder-layout-unparseable" ;;
    esac
fi

[ -f "$bundled_app_path/Contents/Resources/release-notes.json" ] &&
    [ ! -L "$bundled_app_path/Contents/Resources/release-notes.json" ] ||
    layout_failure "release-notes-resource-missing"
cmp -s "$release_notes_path" "$bundled_app_path/Contents/Resources/release-notes.json" ||
    layout_failure "release-notes-resource-mismatch"

"$hdiutil_path" detach "$device" >/dev/null 2>&1 || layout_failure "readonly-detach-failed"
device=
printf '%s\n' "gate.macos_dmg_layout.status=passed"
printf '%s\n' "gate.macos_dmg_layout.reason=final-volume-layout-present"
printf 'gate.macos_dmg_layout.app_count=%s\n' "$app_count"
printf '%s\n' "gate.macos_dmg_layout.background_dimensions=660x400"
printf '%s\n' "gate.macos_dmg_layout.finder_window_size=660x400"
printf '%s\n' "gate.macos_dmg_layout.app_position=180,220"
printf '%s\n' "gate.macos_dmg_layout.applications_position=480,220"
printf '%s\n' "gate.macos_dmg_layout.background_binding=/.background/background.png"
printf '%s\n' "gate.macos_dmg_layout.release_notes=byte-identical"
