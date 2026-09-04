#!/bin/sh
set -eu

# 为 macOS 上的 Tauri Windows x64 NSIS 交叉候选建立可重复、可复探的工具链门禁。
MODE=install
TARGET=x86_64-pc-windows-msvc
CARGO_XWIN_REQUIREMENT='>=0.23.1, <0.24.0'
PROBE_PATH=${AFH_PREREQ_PATH:-${PATH}}
TEST_PLATFORM=${AFH_TEST_PLATFORM:-}
LLVM_CHANGE=existing
LLD_CHANGE=existing
NSIS_CHANGE=existing
TARGET_CHANGE=existing
XWIN_CHANGE=existing
LLVM_BIN_DIR=
LLD_BIN_DIR=
NSIS_BIN_DIR=
CARGO_BIN_DIR=
CARGO_XWIN_VERSION=Missing

# 展示唯一支持的调用形式，避免把常规 GUI 开发误路由到发布工具安装。
usage() {
    printf '%s\n' "用法：macos-tauri-xwin-gates.sh [--install-missing|--check-only] [--target x86_64-pc-windows-msvc]"
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --install-missing) MODE=install ;;
        --check-only) MODE=check ;;
        --target)
            [ "$#" -ge 2 ] || { usage >&2; exit 2; }
            TARGET=$2
            shift
            ;;
        --help|-h) usage; exit 0 ;;
        *) printf '未知参数：%s\n' "$1" >&2; usage >&2; exit 2 ;;
    esac
    shift
done

[ "$TARGET" = x86_64-pc-windows-msvc ] || {
    printf '不支持的 Tauri xwin 目标：%s\n' "$TARGET" >&2
    exit 2
}

# 只允许测试夹具覆盖宿主，不让生产调用绕过 macOS 专属边界。
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

[ "$(host_platform)" = Darwin ] || {
    printf '%s\n' "gate.tauri_windows_cross.status=not-applicable"
    printf '%s\n' "gate.tauri_windows_cross.reason=requires-macos-host"
    exit 30
}

# 只从明确探测路径选择可执行文件，使隔离测试不碰真实宿主工具链。
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

# 把新安装工具的 bin 目录加入本进程探测路径，同时保留给调用方的稳定输出。
prepend_probe_path() {
    candidate=$1
    [ -d "$candidate" ] || return 0
    case ":$PROBE_PATH:" in
        *":$candidate:"*) ;;
        *) PROBE_PATH=$candidate:$PROBE_PATH ;;
    esac
}

# 使用 Homebrew 事实定位 keg-only LLVM/LLD/NSIS；查询不安装、不更新也不修改环境。
activate_brew_formula() {
    formula=$1
    brew_path=$2
    prefix=$("$brew_path" --prefix "$formula" 2>/dev/null || true)
    [ -n "$prefix" ] || return 1
    [ -d "$prefix/bin" ] || return 1
    prepend_probe_path "$prefix/bin"
    case "$formula" in
        llvm) LLVM_BIN_DIR=$prefix/bin ;;
        lld) LLD_BIN_DIR=$prefix/bin ;;
        nsis) NSIS_BIN_DIR=$prefix/bin ;;
    esac
}

# Homebrew 对未安装 formula 也可能返回预期 prefix；只有实际 bin 目录存在才视为已安装。
brew_formula_installed() {
    formula=$1
    brew_path=$2
    prefix=$("$brew_path" --prefix "$formula" 2>/dev/null || true)
    [ -n "$prefix" ] && [ -d "$prefix/bin" ]
}

# cargo-xwin 是 Cargo 安装的受管工具；既有版本必须落入已验证的兼容范围，不能只检查命令存在。
validate_cargo_xwin() {
    binary=$1
    version_output=$("$binary" --version 2>/dev/null || true)
    set -- $version_output
    [ "${1:-}" = cargo-xwin ] || {
        printf '%s\n' "错误：既有 cargo-xwin 无法报告可解析的稳定版本" >&2
        exit 36
    }
    candidate=${2:-}
    old_ifs=$IFS
    IFS=.
    set -- $candidate
    IFS=$old_ifs
    [ "$#" -eq 3 ] || {
        printf '错误：既有 cargo-xwin 版本 %s 不满足兼容范围 %s\n' "$candidate" "$CARGO_XWIN_REQUIREMENT" >&2
        exit 36
    }
    for part in "$@"; do
        case "$part" in
            ''|*[!0-9]*)
                printf '错误：既有 cargo-xwin 版本 %s 不满足兼容范围 %s\n' "$candidate" "$CARGO_XWIN_REQUIREMENT" >&2
                exit 36
                ;;
        esac
    done
    [ "$1" -eq 0 ] && [ "$2" -eq 23 ] && [ "$3" -ge 1 ] || {
        printf '错误：既有 cargo-xwin 版本 %s 不满足兼容范围 %s\n' "$candidate" "$CARGO_XWIN_REQUIREMENT" >&2
        exit 36
    }
    CARGO_XWIN_VERSION=$candidate
}

# 重新探测全部必需能力，安装后必须再次通过本函数才能宣称成功。
probe_all() {
    brew_path=$(find_tool brew 2>/dev/null || true)
    [ -n "$brew_path" ] && activate_brew_formula llvm "$brew_path" || true
    [ -n "$brew_path" ] && activate_brew_formula lld "$brew_path" || true
    [ -n "$brew_path" ] && activate_brew_formula nsis "$brew_path" || true

    rustup_path=$(find_tool rustup 2>/dev/null || true)
    cargo_path=$(find_tool cargo 2>/dev/null || true)
    pnpm_path=$(find_tool pnpm 2>/dev/null || true)
    llvm_rc_path=$(find_tool llvm-rc 2>/dev/null || true)
    lld_link_path=$(find_tool lld-link 2>/dev/null || true)
    makensis_path=$(find_tool makensis 2>/dev/null || true)
    cargo_xwin_path=$(find_tool cargo-xwin 2>/dev/null || true)

    base_missing=0
    [ -n "$rustup_path" ] || base_missing=1
    [ -n "$cargo_path" ] || base_missing=1
    [ -n "$pnpm_path" ] || base_missing=1
    llvm_missing=0
    llvm_rc_status=0
    if [ -n "$llvm_rc_path" ]; then
        "$llvm_rc_path" >/dev/null 2>&1 || llvm_rc_status=$?
    fi
    if [ -z "$llvm_rc_path" ] || [ "$llvm_rc_status" -gt 1 ]; then
        llvm_missing=1
    fi
    lld_missing=0
    if [ -z "$lld_link_path" ] || ! "$lld_link_path" --version >/dev/null 2>&1; then
        lld_missing=1
    fi
    nsis_missing=0
    if [ -z "$makensis_path" ] || ! "$makensis_path" -VERSION >/dev/null 2>&1; then
        nsis_missing=1
    fi
    xwin_missing=0
    CARGO_XWIN_VERSION=Missing
    if [ -z "$cargo_xwin_path" ]; then
        xwin_missing=1
    else
        validate_cargo_xwin "$cargo_xwin_path"
    fi
    target_missing=1
    if [ -n "$rustup_path" ] && "$rustup_path" target list --installed 2>/dev/null | grep -Fx -- "$TARGET" >/dev/null 2>&1; then
        target_missing=0
    fi
}

# 输出每一项状态；不打印安装路径之外的宿主细节，也不读取任何签名秘密。
emit_status() {
    overall=passed
    [ "$base_missing" -eq 0 ] || overall=missing
    [ "$llvm_missing" -eq 0 ] || overall=missing
    [ "$lld_missing" -eq 0 ] || overall=missing
    [ "$nsis_missing" -eq 0 ] || overall=missing
    [ "$target_missing" -eq 0 ] || overall=missing
    [ "$xwin_missing" -eq 0 ] || overall=missing
    printf 'gate.tauri_windows_cross.status=%s\n' "$overall"
    printf 'gate.tauri_windows_cross.target=%s\n' "$TARGET"
    printf 'gate.tauri_windows_cross.base=%s\n' "$([ "$base_missing" -eq 0 ] && printf passed || printf missing)"
    printf 'gate.llvm.status=%s\n' "$([ "$llvm_missing" -eq 0 ] && printf passed || printf missing)"
    printf 'gate.llvm.change=%s\n' "$LLVM_CHANGE"
    printf 'gate.lld.status=%s\n' "$([ "$lld_missing" -eq 0 ] && printf passed || printf missing)"
    printf 'gate.lld.change=%s\n' "$LLD_CHANGE"
    printf 'gate.nsis.status=%s\n' "$([ "$nsis_missing" -eq 0 ] && printf passed || printf missing)"
    printf 'gate.nsis.change=%s\n' "$NSIS_CHANGE"
    printf 'gate.rust_target.status=%s\n' "$([ "$target_missing" -eq 0 ] && printf passed || printf missing)"
    printf 'gate.rust_target.change=%s\n' "$TARGET_CHANGE"
    printf 'gate.cargo_xwin.status=%s\n' "$([ "$xwin_missing" -eq 0 ] && printf passed || printf missing)"
    printf 'gate.cargo_xwin.requirement=%s\n' "$CARGO_XWIN_REQUIREMENT"
    printf 'gate.cargo_xwin.version=%s\n' "$CARGO_XWIN_VERSION"
    printf 'gate.cargo_xwin.change=%s\n' "$XWIN_CHANGE"
}

probe_all
if [ "$MODE" = check ]; then
    emit_status
    [ "$base_missing" -eq 0 ] &&
        [ "$llvm_missing" -eq 0 ] &&
        [ "$lld_missing" -eq 0 ] &&
        [ "$nsis_missing" -eq 0 ] &&
        [ "$target_missing" -eq 0 ] &&
        [ "$xwin_missing" -eq 0 ] || exit 20
    exit 0
fi

[ "$base_missing" -eq 0 ] || {
    emit_status
    printf '%s\n' "错误：请先运行常规 GUI 开发环境门禁以提供 rustup、cargo 和 pnpm" >&2
    exit 31
}

brew_path=$(find_tool brew 2>/dev/null || true)
if [ "$llvm_missing" -eq 1 ] || [ "$lld_missing" -eq 1 ] || [ "$nsis_missing" -eq 1 ]; then
    [ -n "$brew_path" ] || {
        emit_status
        printf '%s\n' "错误：自动安装 LLVM/LLD/NSIS 需要既有 Homebrew；本门禁不自动安装 Homebrew" >&2
        exit 32
    }
fi

if [ "$lld_missing" -eq 1 ]; then
    if brew_formula_installed lld "$brew_path"; then
        printf '%s\n' "错误：既有 LLD 缺少 lld-link，拒绝静默升级/重装" >&2
        exit 33
    fi
    HOMEBREW_NO_AUTO_UPDATE=1 "$brew_path" install lld || {
        printf '%s\n' "错误：LLD 安装失败" >&2
        exit 33
    }
    LLD_CHANGE=installed
fi

if [ "$llvm_missing" -eq 1 ]; then
    if brew_formula_installed llvm "$brew_path"; then
        printf '%s\n' "错误：既有 LLVM 缺少 llvm-rc，拒绝静默升级/重装" >&2
        exit 33
    fi
    HOMEBREW_NO_AUTO_UPDATE=1 "$brew_path" install llvm || {
        printf '%s\n' "错误：LLVM 安装失败" >&2
        exit 33
    }
    LLVM_CHANGE=installed
fi

if [ "$nsis_missing" -eq 1 ]; then
    if brew_formula_installed nsis "$brew_path"; then
        printf '%s\n' "错误：既有 NSIS 缺少 makensis，拒绝静默升级/重装" >&2
        exit 34
    fi
    HOMEBREW_NO_AUTO_UPDATE=1 "$brew_path" install nsis || {
        printf '%s\n' "错误：NSIS 安装失败" >&2
        exit 34
    }
    NSIS_CHANGE=installed
fi

if [ "$target_missing" -eq 1 ]; then
    "$rustup_path" target add "$TARGET" || {
        printf '%s\n' "错误：Windows Rust target 安装失败" >&2
        exit 35
    }
    TARGET_CHANGE=installed
fi

if [ "$xwin_missing" -eq 1 ]; then
    cargo_home=${CARGO_HOME:-${HOME:?必须设置 HOME}/.cargo}
    "$cargo_path" install --locked --version "$CARGO_XWIN_REQUIREMENT" cargo-xwin || {
        printf '%s\n' "错误：cargo-xwin 安装失败" >&2
        exit 36
    }
    CARGO_BIN_DIR=$cargo_home/bin
    prepend_probe_path "$CARGO_BIN_DIR"
    XWIN_CHANGE=installed
fi

probe_all
[ "$base_missing" -eq 0 ] &&
    [ "$llvm_missing" -eq 0 ] &&
    [ "$lld_missing" -eq 0 ] &&
    [ "$nsis_missing" -eq 0 ] &&
    [ "$target_missing" -eq 0 ] &&
    [ "$xwin_missing" -eq 0 ] || {
    emit_status
    printf '%s\n' "错误：Tauri xwin 环境安装后复探仍失败" >&2
    exit 37
}

emit_status
changed=false
[ "$LLVM_CHANGE" = installed ] && changed=true
[ "$LLD_CHANGE" = installed ] && changed=true
[ "$NSIS_CHANGE" = installed ] && changed=true
[ "$TARGET_CHANGE" = installed ] && changed=true
[ "$XWIN_CHANGE" = installed ] && changed=true
printf 'gate.changed=%s\n' "$changed"

path_prepend=
for candidate in "$LLVM_BIN_DIR" "$LLD_BIN_DIR" "$NSIS_BIN_DIR" "$CARGO_BIN_DIR"; do
    [ -n "$candidate" ] || continue
    case ":$path_prepend:" in
        *":$candidate:"*) ;;
        *) [ -n "$path_prepend" ] && path_prepend=$path_prepend:$candidate || path_prepend=$candidate ;;
    esac
done
if [ -n "$path_prepend" ]; then
    printf 'gate.path.prepend=%s\n' "$path_prepend"
fi
