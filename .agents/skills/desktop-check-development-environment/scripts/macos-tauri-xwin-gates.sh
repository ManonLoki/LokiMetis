#!/bin/sh
set -eu

# 为 macOS 上的 Tauri Windows x64 NSIS 交叉候选建立可重复、可复探的工具链门禁。
MODE=install
TARGET=x86_64-pc-windows-msvc
CARGO_XWIN_REQUIREMENT='>=0.23.1, <0.24.0'
PROBE_PATH=${AFH_PREREQ_PATH:-${PATH}}
TEST_PLATFORM=${AFH_TEST_PLATFORM:-}
TEST_MODE=${AFH_TEST_MODE:-0}
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
xwin_upgrade_required=0

case "$TEST_MODE" in
    0|1) ;;
    *) printf '%s\n' "错误：AFH_TEST_MODE 只接受显式值 1" >&2; exit 2 ;;
esac
for override_name in AFH_PREREQ_PATH AFH_TEST_PLATFORM AFH_ALLOW_TEST_OVERRIDES \
    AFH_MANAGED_CARGO_HOME AFH_MANAGED_RUSTUP_HOME; do
    eval "override_value=\${$override_name-}"
    if [ -n "$override_value" ] && [ "$TEST_MODE" != 1 ]; then
        printf '错误：测试覆盖 %s 仅在 AFH_TEST_MODE=1 时允许\n' "$override_name" >&2
        exit 2
    fi
done

USER_HOME=${HOME:?必须设置 HOME}
MANAGED_CARGO_HOME=$USER_HOME/.cargo
MANAGED_RUSTUP_HOME=$USER_HOME/.rustup
if [ "$TEST_MODE" = 1 ]; then
    MANAGED_CARGO_HOME=${AFH_MANAGED_CARGO_HOME:-$MANAGED_CARGO_HOME}
    MANAGED_RUSTUP_HOME=${AFH_MANAGED_RUSTUP_HOME:-$MANAGED_RUSTUP_HOME}
fi
for managed_path in "$MANAGED_CARGO_HOME" "$MANAGED_RUSTUP_HOME"; do
    case "$managed_path" in
        /*) ;;
        *) printf '错误：受管 Rust 根必须是绝对路径：%s\n' "$managed_path" >&2; exit 2 ;;
    esac
done

# rustup/cargo 的写入根固定到当前用户；既有符号链接或非目录目标失败关闭。
validate_managed_rust_root() {
    managed_path=$1
    managed_label=$2
    case "$managed_path" in
        "$USER_HOME"/*) ;;
        *)
            test_root=${USER_HOME%/*}
            [ "$TEST_MODE" = 1 ] || { printf '错误：%s 必须位于当前用户目录内\n' "$managed_label" >&2; exit 36; }
            case "$managed_path" in
                "$test_root"/*) ;;
                *) printf '错误：测试 %s 必须位于隔离 HOME 的同级测试根内\n' "$managed_label" >&2; exit 36 ;;
            esac
            ;;
    esac
    [ ! -L "$managed_path" ] || { printf '错误：%s 不能是符号链接：%s\n' "$managed_label" "$managed_path" >&2; exit 36; }
    if [ -e "$managed_path" ] && [ ! -d "$managed_path" ]; then
        printf '错误：%s 不是普通目录：%s\n' "$managed_label" "$managed_path" >&2
        exit 36
    fi
}

# 删除 PATH 空段和重复项，避免当前工作目录中的 shim 被当作宿主工具。
sanitize_path() {
    input_path=$1
    result_path=
    old_ifs=$IFS
    glob_was_enabled=0
    case $- in
        *f*) ;;
        *) set -f; glob_was_enabled=1 ;;
    esac
    IFS=:
    for path_entry in $input_path; do
        [ -n "$path_entry" ] || continue
        case "$path_entry" in /*) ;; *) continue ;; esac
        case ":$result_path:" in
            *":$path_entry:"*) ;;
            *) [ -n "$result_path" ] && result_path=$result_path:$path_entry || result_path=$path_entry ;;
        esac
    done
    IFS=$old_ifs
    [ "$glob_was_enabled" -eq 0 ] || set +f
    printf '%s\n' "$result_path"
}

PATH=$(sanitize_path "${PATH}")
export PATH
PROBE_PATH=$(sanitize_path "$PROBE_PATH")

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
        [ "$TEST_MODE" = 1 ] || {
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
    glob_was_enabled=0
    case $- in
        *f*) ;;
        *) set -f; glob_was_enabled=1 ;;
    esac
    IFS=:
    for tool_dir in $PROBE_PATH; do
        [ -n "$tool_dir" ] || continue
        if [ -x "$tool_dir/$tool_name" ] && [ ! -d "$tool_dir/$tool_name" ]; then
            printf '%s\n' "$tool_dir/$tool_name"
            IFS=$old_ifs
            [ "$glob_was_enabled" -eq 0 ] || set +f
            return 0
        fi
    done
    IFS=$old_ifs
    [ "$glob_was_enabled" -eq 0 ] || set +f
    return 1
}

# 把新安装工具的 bin 目录加入本进程探测路径，同时保留给调用方的稳定输出。
prepend_probe_path() {
    candidate=$1
    [ -d "$candidate" ] || return 0
    case ":$PROBE_PATH:" in
        *":$candidate:"*) ;;
        *) PROBE_PATH=$candidate${PROBE_PATH:+:$PROBE_PATH} ;;
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

# cargo-xwin 是 Cargo 安装的受管工具；明确低于下界时允许升级，其他范围外版本继续失败关闭。
validate_cargo_xwin() {
    binary=$1
    version_output=$(CARGO_HOME=$MANAGED_CARGO_HOME RUSTUP_HOME=$MANAGED_RUSTUP_HOME "$binary" --version 2>/dev/null || true)
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
    [ "$1" -eq 0 ] || {
        printf '错误：既有 cargo-xwin 版本 %s 不满足兼容范围 %s\n' "$candidate" "$CARGO_XWIN_REQUIREMENT" >&2
        exit 36
    }
    if [ "$2" -lt 23 ] || { [ "$2" -eq 23 ] && [ "$3" -lt 1 ]; }; then
        CARGO_XWIN_VERSION=$candidate
        xwin_upgrade_required=1
        return
    fi
    [ "$2" -eq 23 ] || {
        printf '错误：既有 cargo-xwin 版本 %s 不满足兼容范围 %s\n' "$candidate" "$CARGO_XWIN_REQUIREMENT" >&2
        exit 36
    }
    CARGO_XWIN_VERSION=$candidate
}

# 重新探测全部必需能力，安装后必须再次通过本函数才能宣称成功。
probe_all() {
    xwin_upgrade_required=0
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
    if [ -n "$rustup_path" ] && CARGO_HOME=$MANAGED_CARGO_HOME RUSTUP_HOME=$MANAGED_RUSTUP_HOME "$rustup_path" target list --installed 2>/dev/null | grep -Fx -- "$TARGET" >/dev/null 2>&1; then
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
    if [ "$overall" = passed ] && [ "$xwin_upgrade_required" -eq 1 ]; then
        overall=upgrade-required
    fi
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
    if [ "$xwin_missing" -eq 1 ]; then
        cargo_xwin_status=missing
    elif [ "$xwin_upgrade_required" -eq 1 ]; then
        cargo_xwin_status=upgrade-required
    else
        cargo_xwin_status=passed
    fi
    printf 'gate.cargo_xwin.status=%s\n' "$cargo_xwin_status"
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
        [ "$xwin_missing" -eq 0 ] &&
        [ "$xwin_upgrade_required" -eq 0 ] || exit 20
    exit 0
fi

[ "$base_missing" -eq 0 ] || {
    emit_status
    printf '%s\n' "错误：请先运行常规 GUI 开发环境门禁以提供 rustup、cargo 和 pnpm" >&2
    exit 31
}

if [ "$target_missing" -eq 1 ]; then
    validate_managed_rust_root "$MANAGED_RUSTUP_HOME" "Rust rustup 受管安装根"
fi
if [ "$xwin_missing" -eq 1 ] || [ "$xwin_upgrade_required" -eq 1 ]; then
    validate_managed_rust_root "$MANAGED_CARGO_HOME" "Rust Cargo 受管安装根"
fi

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
    CARGO_HOME=$MANAGED_CARGO_HOME RUSTUP_HOME=$MANAGED_RUSTUP_HOME "$rustup_path" target add "$TARGET" || {
        printf '%s\n' "错误：Windows Rust target 安装失败" >&2
        exit 35
    }
    TARGET_CHANGE=installed
fi

if [ "$xwin_missing" -eq 1 ] || [ "$xwin_upgrade_required" -eq 1 ]; then
    if [ "$xwin_upgrade_required" -eq 1 ]; then
        requested_xwin_change=upgraded
    else
        requested_xwin_change=installed
    fi
    CARGO_HOME=$MANAGED_CARGO_HOME RUSTUP_HOME=$MANAGED_RUSTUP_HOME "$cargo_path" install --locked --version "$CARGO_XWIN_REQUIREMENT" cargo-xwin || {
        printf '%s\n' "错误：cargo-xwin 安装失败" >&2
        exit 36
    }
    CARGO_BIN_DIR=$MANAGED_CARGO_HOME/bin
    prepend_probe_path "$CARGO_BIN_DIR"
    XWIN_CHANGE=$requested_xwin_change
fi

probe_all
[ "$base_missing" -eq 0 ] &&
    [ "$llvm_missing" -eq 0 ] &&
    [ "$lld_missing" -eq 0 ] &&
    [ "$nsis_missing" -eq 0 ] &&
    [ "$target_missing" -eq 0 ] &&
    [ "$xwin_missing" -eq 0 ] &&
    [ "$xwin_upgrade_required" -eq 0 ] || {
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
[ "$XWIN_CHANGE" = upgraded ] && changed=true
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
