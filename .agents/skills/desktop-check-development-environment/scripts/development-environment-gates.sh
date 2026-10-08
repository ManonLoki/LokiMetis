#!/bin/sh
set -eu

# MSRV 的唯一事实来源见 docs/RUST_CLI_TEMPLATE.md；修改此值时必须同步更新 development-environment-gates.ps1。
MIN_RUST_MAJOR=1
MIN_RUST_MINOR=98
MIN_RUST_PATCH=1
NODE_REQUIREMENT='>=24.21.0'
PNPM_REQUIREMENT='>=12.4.1'
PNPM_INSTALL_REQUIREMENT='pnpm@>=12.4.1'
GIT_REQUIREMENT='>=2.36.0'
PNPM_REGISTRY='https://registry.npmjs.org/'
TEST_MODE=${AFH_TEST_MODE:-0}
MODE=install
INTERFACES=
GUI_FRAMEWORK=tauri
PNPM_REQUIRED=0

# 输出稳定的命令入口说明，避免调用方误把只读模式当成首次开发安装模式。
usage() {
    printf '%s\n' "用法：development-environment-gates.sh [--install-missing|--check-only] --interfaces CLI,TUI,MCP,GUI [--gui-framework tauri|gpui]"
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --install-missing) MODE=install ;;
        --check-only) MODE=check ;;
        --interfaces)
            [ "$#" -ge 2 ] || { usage >&2; exit 2; }
            INTERFACES=$2
            shift
            ;;
        --gui-framework)
            [ "$#" -ge 2 ] || { usage >&2; exit 2; }
            GUI_FRAMEWORK=$2
            case "$GUI_FRAMEWORK" in
                tauri|gpui) ;;
                *) printf '不支持的 GUI 框架：%s\n' "$GUI_FRAMEWORK" >&2; exit 2 ;;
            esac
            shift
            ;;
        --help|-h) usage; exit 0 ;;
        *) printf '未知参数：%s\n' "$1" >&2; usage >&2; exit 2 ;;
    esac
    shift
done

PROBE_PATH=${AFH_PREREQ_PATH:-${PATH}}
RUST_CHANGED=existing
GIT_CHANGED=existing
NODE_CHANGED=existing
PNPM_CHANGED=existing
NODE_BIN_DIR=
PNPM_BIN_DIR=
CARGO_BIN_DIR=
GIT_BIN_DIR=
TEMP_DIR=
FISH_TEMP_FILE=
FRESH_VERIFY_FILE=
FRESH_VERIFY_DIR=
RUST_HOME_VERIFY_FILE=
RUST_HOME_VERIFY_DIR=
LINK_TEMP_DIR=
PROFILE_TEMP_FILE=
PROFILE_SNAPSHOT_FILE=
SELECTED_NODE_VERSION=
FRESH_SHELL_STATUS=not-required

# 只清理本进程通过 mktemp 创建的下载目录，不触碰安装目标或用户已有文件。
cleanup() {
    for gate_temp_file in "$FISH_TEMP_FILE" "$FRESH_VERIFY_FILE" "$RUST_HOME_VERIFY_FILE" "$PROFILE_TEMP_FILE" "$PROFILE_SNAPSHOT_FILE"; do
        [ -n "$gate_temp_file" ] || continue
        if [ -f "$gate_temp_file" ] || [ -L "$gate_temp_file" ]; then rm -f "$gate_temp_file" 2>/dev/null || true; fi
    done
    if [ -n "$LINK_TEMP_DIR" ] && [ -d "$LINK_TEMP_DIR" ] && [ ! -L "$LINK_TEMP_DIR" ]; then
        find "$LINK_TEMP_DIR" -depth -delete 2>/dev/null || true
    fi
    if [ -n "$FRESH_VERIFY_DIR" ] && [ -d "$FRESH_VERIFY_DIR" ] && [ ! -L "$FRESH_VERIFY_DIR" ]; then
        find "$FRESH_VERIFY_DIR" -depth -delete 2>/dev/null || true
    fi
    if [ -n "$RUST_HOME_VERIFY_DIR" ] && [ -d "$RUST_HOME_VERIFY_DIR" ] && [ ! -L "$RUST_HOME_VERIFY_DIR" ]; then
        find "$RUST_HOME_VERIFY_DIR" -depth -delete 2>/dev/null || true
    fi
    if [ -n "$TEMP_DIR" ] && [ -d "$TEMP_DIR" ]; then
        find "$TEMP_DIR" -depth -delete 2>/dev/null || true
    fi
}
trap cleanup EXIT HUP INT TERM

# 以稳定退出码终止门禁，让 Agent 能区分缺失、版本、下载和校验失败。
fail() {
    code=$1
    shift
    printf '错误：%s\n' "$*" >&2
    exit "$code"
}

# 探测路径、下载镜像、安装目录和 registry 覆盖都只允许显式隔离测试使用。
case "$TEST_MODE" in
    0|1) ;;
    *) fail 2 "AFH_TEST_MODE 只接受显式值 1" ;;
esac
for override_name in \
    AFH_PREREQ_PATH AFH_ALLOW_FILE_URLS AFH_RUSTUP_DIST_BASE AFH_NODE_DIST_BASE \
    AFH_MANAGED_CARGO_HOME AFH_MANAGED_RUSTUP_HOME AFH_NODE_HOME AFH_PNPM_HOME \
    AFH_PNPM_REGISTRY AFH_SKIP_PERSIST_PATH AFH_TEST_HOST_OS AFH_TEST_HOST_ARCH \
    AFH_TEST_LINUX_LIBC AFH_TEST_SYSTEM_PATH; do
    eval "override_value=\${$override_name-}"
    if [ -n "$override_value" ] && [ "$TEST_MODE" != 1 ]; then
        fail 2 "测试覆盖 $override_name 仅在 AFH_TEST_MODE=1 时允许"
    fi
done
if [ "$TEST_MODE" = 1 ] && [ -n "${AFH_PNPM_REGISTRY:-}" ]; then
    PNPM_REGISTRY=$AFH_PNPM_REGISTRY
fi
case "$PNPM_REGISTRY" in
    https://*) ;;
    *) fail 2 "pnpm registry 必须使用 HTTPS" ;;
esac

# 生产调用使用各工具的标准当前用户安装根；用户已设置的标准 CARGO_HOME/RUSTUP_HOME 会被原样尊重。
USER_HOME=${HOME:?必须设置 HOME}
case "$USER_HOME" in
    /*) ;;
    *) fail 24 "HOME 必须是绝对路径" ;;
esac
case "$USER_HOME" in
    *:*) fail 24 "HOME 不能包含 PATH 分隔符冒号" ;;
esac
MANAGED_CARGO_HOME=${CARGO_HOME:-$USER_HOME/.cargo}
MANAGED_RUSTUP_HOME=${RUSTUP_HOME:-$USER_HOME/.rustup}
NODE_HOME=$USER_HOME/.local/lib/nodejs
PNPM_HOME=$USER_HOME/.local
if [ "$TEST_MODE" = 1 ]; then
    MANAGED_CARGO_HOME=${AFH_MANAGED_CARGO_HOME:-$MANAGED_CARGO_HOME}
    MANAGED_RUSTUP_HOME=${AFH_MANAGED_RUSTUP_HOME:-$MANAGED_RUSTUP_HOME}
    NODE_HOME=${AFH_NODE_HOME:-$NODE_HOME}
    PNPM_HOME=${AFH_PNPM_HOME:-$PNPM_HOME}
fi
for managed_path in "$MANAGED_CARGO_HOME" "$MANAGED_RUSTUP_HOME" "$NODE_HOME" "$PNPM_HOME"; do
    case "$managed_path" in
        /*) ;;
        *) fail 24 "受管安装根必须是绝对路径：$managed_path" ;;
    esac
    case "$managed_path" in
        *:*) fail 24 "受管安装根不能包含 PATH 分隔符冒号：$managed_path" ;;
    esac
done

# 删除 PATH 空段和重复项，永远不把空段解释为当前工作目录。
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
FRESH_BASE_PATH=/usr/bin:/bin:/usr/sbin:/sbin
if [ "$TEST_MODE" = 1 ] && [ -n "${AFH_TEST_SYSTEM_PATH:-}" ]; then
    FRESH_BASE_PATH=$AFH_TEST_SYSTEM_PATH
fi
FRESH_BASE_PATH=$(sanitize_path "$FRESH_BASE_PATH")
[ -n "$FRESH_BASE_PATH" ] || fail 24 "新登录会话的系统 PATH 基线为空"

normalized_interfaces=$(printf '%s' "$INTERFACES" | tr '[:lower:]' '[:upper:]' | tr -d ' ')
for interface in $(printf '%s' "$normalized_interfaces" | tr ',' ' '); do
    case "$interface" in
        CLI|TUI|MCP|GUI) ;;
        *) printf '不支持的接口：%s\n' "$interface" >&2; usage >&2; exit 2 ;;
    esac
done
case ",$normalized_interfaces," in
    *,GUI,*) PNPM_REQUIRED=1 ;;
esac

# 仅在门禁探测路径中解析工具，隔离测试可因此隐藏机器已有环境。
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

# 下载官方 HTTPS 制品；file URL 只在显式测试开关下用于隔离测试夹具。
download() {
    source_url=$1
    destination=$2
    case "$source_url" in
        https://*) curl --proto '=https' --tlsv1.2 -fsSL "$source_url" -o "$destination" ;;
        file://*)
            [ "$TEST_MODE" = 1 ] && [ "${AFH_ALLOW_FILE_URLS:-0}" = 1 ] || fail 24 "file URL 已禁用"
            curl -fsSL "$source_url" -o "$destination"
            ;;
        *) fail 24 "不支持的下载 URL 协议：$source_url" ;;
    esac
}

# 验证 rustup/rustc/cargo 为稳定工具链，并用 rustc -vV 复核 release 与 host。
validate_rust() {
    rustup_path=$1
    rustc_path=$2
    cargo_path=$3
    rustup_text=$("$rustup_path" --version 2>/dev/null) || fail 21 "rustup 探测失败"
    rust_text=$("$rustc_path" --version 2>/dev/null) || fail 21 "rustc 探测失败"
    cargo_text=$("$cargo_path" --version 2>/dev/null) || fail 21 "cargo 探测失败"
    rust_verbose=$("$rustc_path" -vV 2>/dev/null) || fail 21 "rustc -vV 探测失败"
    [ "$(printf '%s\n' "$rustup_text" | awk 'END { print NR }')" -eq 1 ] || fail 21 "rustup --version 必须返回唯一一行"
    [ "$(printf '%s\n' "$rust_text" | awk 'END { print NR }')" -eq 1 ] || fail 21 "rustc --version 必须返回唯一一行"
    [ "$(printf '%s\n' "$cargo_text" | awk 'END { print NR }')" -eq 1 ] || fail 21 "cargo --version 必须返回唯一一行"
    set -- $rustup_text
    [ "${1:-}" = rustup ] && [ -n "${2:-}" ] || fail 21 "现有 rustup 不是可识别的稳定发布版：$rustup_text"
    rustup_release=$2
    set -- $rust_text
    [ "${1:-}" = rustc ] && [ -n "${2:-}" ] || fail 21 "现有 rustc 不是可识别的稳定发布版：$rust_text"
    rust_release=$2
    set -- $cargo_text
    [ "${1:-}" = cargo ] && [ -n "${2:-}" ] || fail 21 "现有 cargo 不是可识别的稳定发布版：$cargo_text"
    cargo_release=$2
    for tool_release in "$rustup_release" "$rust_release" "$cargo_release"; do
        case "$tool_release" in
            *-*|*+*) fail 21 "现有 Rust 工具链不是可识别的稳定发布版：$tool_release" ;;
            *.*.*) ;;
            *) fail 21 "无法识别 Rust 工具链版本：$tool_release" ;;
        esac
        release_parts=$(printf '%s\n' "$tool_release" | awk -F. '{print NF ":" $1 ":" $2 ":" $3}')
        case "$release_parts" in
            3:[0-9]*:[0-9]*:[0-9]*) ;;
            *) fail 21 "无法识别 Rust 工具链版本：$tool_release" ;;
        esac
        case "$tool_release" in
            *[!0-9.]*) fail 21 "无法识别 Rust 工具链版本：$tool_release" ;;
        esac
    done
    rust_major=$(printf '%s\n' "$rust_release" | awk -F. '{print $1}')
    rust_minor=$(printf '%s\n' "$rust_release" | awk -F. '{print $2}')
    rust_patch=$(printf '%s\n' "$rust_release" | awk -F. '{print $3}')
    cargo_major=$(printf '%s\n' "$cargo_release" | awk -F. '{print $1}')
    cargo_minor=$(printf '%s\n' "$cargo_release" | awk -F. '{print $2}')
    cargo_patch=$(printf '%s\n' "$cargo_release" | awk -F. '{print $3}')
    case "$rust_major:$rust_minor:$rust_patch" in
        *[!0-9:]*|::*|*::|*::*:*) fail 21 "无法识别 Rust 发布版本：$rust_release" ;;
    esac
    case "$cargo_major:$cargo_minor:$cargo_patch" in
        *[!0-9:]*|::*|*::|*::*:*) fail 21 "无法识别 Cargo 发布版本：$cargo_release" ;;
    esac
    [ "$cargo_major" -eq "$rust_major" ] && [ "$cargo_minor" -eq "$rust_minor" ] || fail 21 "rustc 与 cargo 不属于同一 stable 工具链"
    verbose_release_count=$(printf '%s\n' "$rust_verbose" | awk -F': ' '$1 == "release" { count++ } END { print count + 0 }')
    verbose_host_count=$(printf '%s\n' "$rust_verbose" | awk -F': ' '$1 == "host" { count++ } END { print count + 0 }')
    [ "$verbose_release_count" -eq 1 ] || fail 21 "rustc -vV 必须返回唯一 release"
    [ "$verbose_host_count" -eq 1 ] || fail 21 "rustc -vV 必须返回唯一 host"
    verbose_release=$(printf '%s\n' "$rust_verbose" | awk -F': ' '$1 == "release" { print $2 }')
    verbose_host=$(printf '%s\n' "$rust_verbose" | awk -F': ' '$1 == "host" { print $2 }')
    [ "$verbose_release" = "$rust_release" ] || fail 21 "rustc -vV 的 release 与 rustc --version 不一致"
    case "$verbose_host" in
        ''|*[!A-Za-z0-9_.-]*) fail 21 "rustc -vV 未返回可识别 host" ;;
    esac
    RUSTUP_VERSION=$rustup_text
    RUST_VERSION=$rust_text
    CARGO_VERSION=$cargo_text
    RUST_HOST=$verbose_host
    if [ "$rust_major" -lt "$MIN_RUST_MAJOR" ] || {
        [ "$rust_major" -eq "$MIN_RUST_MAJOR" ] && {
            [ "$rust_minor" -lt "$MIN_RUST_MINOR" ] || {
                [ "$rust_minor" -eq "$MIN_RUST_MINOR" ] && [ "$rust_patch" -lt "$MIN_RUST_PATCH" ];
            };
        };
    } || [ "$cargo_major" -lt "$MIN_RUST_MAJOR" ] || {
        [ "$cargo_major" -eq "$MIN_RUST_MAJOR" ] && {
            [ "$cargo_minor" -lt "$MIN_RUST_MINOR" ] || {
                [ "$cargo_minor" -eq "$MIN_RUST_MINOR" ] && [ "$cargo_patch" -lt "$MIN_RUST_PATCH" ];
            };
        };
    }; then
        RUST_STATUS=upgrade-required
        return
    fi
    RUST_STATUS=passed
}

# 验证 Node.js 达到连续最低下界；任何更高正式版本都直接通过。
validate_node() {
    node_path=$1
    node_text=$("$node_path" --version 2>/dev/null) || fail 23 "Node.js 探测失败"
    node_release=$(printf '%s\n' "$node_text" | sed 's/^v//')
    case "$node_release" in
        *-*|*+*) fail 23 "现有 Node.js 不是可识别的稳定发布版：$node_text" ;;
        *.*.*) ;;
        *) fail 23 "无法识别 Node.js 发布版本：$node_text" ;;
    esac
    node_parts=$(printf '%s\n' "$node_release" | awk -F. '{print NF ":" $1 ":" $2 ":" $3}')
    case "$node_parts" in
        3:[0-9]*:[0-9]*:[0-9]*) ;;
        *) fail 23 "无法识别 Node.js 发布版本：$node_text" ;;
    esac
    case "$node_release" in
        *[!0-9.]*) fail 23 "无法识别 Node.js 发布版本：$node_text" ;;
    esac
    node_major=$(printf '%s\n' "$node_release" | awk -F. '{print $1}')
    node_minor=$(printf '%s\n' "$node_release" | awk -F. '{print $2}')
    node_patch=$(printf '%s\n' "$node_release" | awk -F. '{print $3}')
    case "$node_major:$node_minor:$node_patch" in
        *[!0-9:]*|::*|*::|*::*:*) fail 23 "无法识别 Node.js 发布版本：$node_text" ;;
    esac
    node_compatible=0
    if [ "$node_major" -gt 24 ] || {
        [ "$node_major" -eq 24 ] && {
            [ "$node_minor" -gt 21 ] || { [ "$node_minor" -eq 21 ] && [ "$node_patch" -ge 0 ]; };
        };
    }; then
        node_compatible=1
    fi
    NODE_VERSION=$node_text
    if [ "$node_compatible" -eq 1 ]; then
        NODE_STATUS=passed
    else
        NODE_STATUS=upgrade-required
    fi
}

# 验证 Node 路线自带的 npm 是可执行、可解析的稳定发布版。
validate_npm() {
    npm_path=$1
    npm_text=$(PATH=$PROBE_PATH${PATH:+:$PATH} "$npm_path" --version 2>/dev/null) || fail 23 "npm 探测失败"
    case "$npm_text" in
        *-*|*+*|*[!0-9.]*) fail 23 "现有 npm 不是可识别的稳定发布版：$npm_text" ;;
        *.*.*) ;;
        *) fail 23 "无法识别 npm 发布版本：$npm_text" ;;
    esac
    npm_parts=$(printf '%s\n' "$npm_text" | awk -F. '{print NF ":" $1 ":" $2 ":" $3}')
    case "$npm_parts" in
        3:[0-9]*:[0-9]*:[0-9]*) ;;
        *) fail 23 "无法识别 npm 发布版本：$npm_text" ;;
    esac
    NPM_VERSION=$npm_text
}

# 验证 pnpm 满足声明下界；可证明低于下界时返回升级需求。
validate_pnpm() {
    pnpm_path=$1
    pnpm_text=$(PATH=$PROBE_PATH${PATH:+:$PATH} "$pnpm_path" --version 2>/dev/null) || fail 28 "pnpm 探测失败"
    case "$pnpm_text" in
        *-*|*+*) fail 28 "现有 pnpm 不是可识别的稳定发布版：$pnpm_text" ;;
        *.*.*) ;;
        *) fail 28 "无法识别 pnpm 发布版本：$pnpm_text" ;;
    esac
    pnpm_parts=$(printf '%s\n' "$pnpm_text" | awk -F. '{print NF ":" $1 ":" $2 ":" $3}')
    case "$pnpm_parts" in
        3:[0-9]*:[0-9]*:[0-9]*) ;;
        *) fail 28 "无法识别 pnpm 发布版本：$pnpm_text" ;;
    esac
    case "$pnpm_text" in
        *[!0-9.]*) fail 28 "无法识别 pnpm 发布版本：$pnpm_text" ;;
    esac
    pnpm_major=$(printf '%s\n' "$pnpm_text" | awk -F. '{print $1}')
    pnpm_minor=$(printf '%s\n' "$pnpm_text" | awk -F. '{print $2}')
    pnpm_patch=$(printf '%s\n' "$pnpm_text" | awk -F. '{print $3}')
    case "$pnpm_major:$pnpm_minor:$pnpm_patch" in
        *[!0-9:]*|::*|*::|*::*:*) fail 28 "无法识别 pnpm 发布版本：$pnpm_text" ;;
    esac
    PNPM_VERSION=$pnpm_text
    if [ "$pnpm_major" -lt 12 ] || {
        [ "$pnpm_major" -eq 12 ] && {
            [ "$pnpm_minor" -lt 4 ] || { [ "$pnpm_minor" -eq 4 ] && [ "$pnpm_patch" -lt 1 ]; };
        };
    }; then
        PNPM_STATUS=upgrade-required
        return
    fi
    PNPM_STATUS=passed
}

# Git 是所有初始化路径的基础工具；低于 2.36.0 时返回升级需求。
validate_git() {
    git_path=$1
    git_text=$("$git_path" --version 2>/dev/null) || fail 29 "Git 探测失败"
    git_release=$(printf '%s\n' "$git_text" | awk '{print $3}')
    git_major=$(printf '%s\n' "$git_release" | awk -F. '{print $1}')
    git_minor=$(printf '%s\n' "$git_release" | awk -F. '{print $2}')
    git_patch=$(printf '%s\n' "$git_release" | awk -F. '{print $3}')
    case "$git_major:$git_minor:$git_patch" in
        *[!0-9:]*|::*|*::|*::*:*) fail 29 "现有 Git 不是可识别的稳定发布版：$git_text" ;;
    esac
    git_base="$git_major.$git_minor.$git_patch"
    if [ "$git_release" != "$git_base" ]; then
        git_windows_prefix="${git_base}.windows."
        case "$git_release" in
            "$git_windows_prefix"*)
                git_windows_serial=${git_release#"$git_windows_prefix"}
                case "$git_windows_serial" in
                    ''|*[!0-9]*) fail 29 "现有 Git 不是可识别的稳定发布版：$git_text" ;;
                esac
                ;;
            *) fail 29 "现有 Git 不是可识别的稳定发布版：$git_text" ;;
        esac
    fi
    GIT_VERSION=$git_text
    if [ "$git_major" -gt 2 ] || { [ "$git_major" -eq 2 ] && [ "$git_minor" -ge 36 ]; }; then
        GIT_STATUS=passed
    else
        GIT_STATUS=upgrade-required
    fi
}

# Linux 包管理器需要提权时只使用既有 sudo；不下载或安装新的包管理器。
run_git_package_manager() {
    if [ "$(id -u)" -eq 0 ]; then
        "$@"
        return
    fi
    sudo_path=$(find_tool sudo 2>/dev/null || true)
    [ -n "$sudo_path" ] || fail 29 "安装 Git 需要既有 sudo 或管理员权限"
    "$sudo_path" "$@"
}

# 只通过宿主已有的受管包管理器安装或升级 Git，完成后由主流程重新探测。
install_git() {
    requested_change=$1
    git_host_os=$(uname -s 2>/dev/null) || fail 29 "无法为 Git 安装探测操作系统"
    case "$git_host_os" in
        Darwin)
            brew_path=$(find_tool brew 2>/dev/null || true)
            if [ -z "$brew_path" ]; then
                for brew_candidate in /opt/homebrew/bin/brew /usr/local/bin/brew; do
                    if [ -x "$brew_candidate" ] && [ ! -d "$brew_candidate" ]; then
                        brew_path=$brew_candidate
                        break
                    fi
                done
            fi
            [ -n "$brew_path" ] || fail 29 "macOS 安装 Git 需要既有 Homebrew；门禁不会自动安装 Homebrew"
            printf '正在通过既有 Homebrew 安装或升级 Git。\n' >&2
            "$brew_path" install git || fail 29 "Homebrew 安装 Git 失败"
            brew_prefix=$($brew_path --prefix 2>/dev/null || true)
            if [ -n "$brew_prefix" ] && [ -d "$brew_prefix/bin" ]; then
                GIT_BIN_DIR=$brew_prefix/bin
                prepend_probe_path "$brew_prefix/bin"
            fi
            ;;
        Linux)
            git_manager=
            for manager_name in apt-get dnf yum zypper apk pacman; do
                manager_path=$(find_tool "$manager_name" 2>/dev/null || true)
                if [ -n "$manager_path" ]; then
                    git_manager=$manager_name
                    break
                fi
            done
            [ -n "$git_manager" ] || fail 29 "Linux 安装 Git 需要受支持的既有系统包管理器（apt-get/dnf/yum/zypper/apk/pacman）"
            printf '正在通过既有 %s 安装或升级 Git。\n' "$git_manager" >&2
            case "$git_manager" in
                apt-get)
                    run_git_package_manager "$manager_path" update || fail 29 "apt-get 更新软件包索引失败"
                    run_git_package_manager "$manager_path" install -y git || fail 29 "apt-get 安装 Git 失败"
                    ;;
                dnf|yum) run_git_package_manager "$manager_path" install -y git || fail 29 "$git_manager 安装 Git 失败" ;;
                zypper) run_git_package_manager "$manager_path" --non-interactive install git || fail 29 "zypper 安装 Git 失败" ;;
                apk) run_git_package_manager "$manager_path" add --no-cache git || fail 29 "apk 安装 Git 失败" ;;
                pacman) run_git_package_manager "$manager_path" --sync --needed --noconfirm git || fail 29 "pacman 安装 Git 失败" ;;
            esac
            ;;
        *) fail 29 "不支持在此 Unix 操作系统自动安装 Git：$git_host_os" ;;
    esac
    GIT_CHANGED=$requested_change
}

# 识别 Linux libc；测试与 Rust/Node 安装共用同一结论，未知值停止而不猜测。
detect_linux_libc() {
    libc_failure_code=$1
    libc_consumer=$2
    detected_linux_libc=${AFH_TEST_LINUX_LIBC:-}
    if [ -z "$detected_linux_libc" ]; then
        if command -v getconf >/dev/null 2>&1 && getconf GNU_LIBC_VERSION >/dev/null 2>&1; then
            detected_linux_libc=gnu
        else
            ldd_text=$(ldd --version 2>&1 || true)
            case "$ldd_text" in
                *musl*|*MUSL*) detected_linux_libc=musl ;;
                *glibc*|*GLIBC*|*GNU*) detected_linux_libc=gnu ;;
            esac
        fi
    fi
    if [ -z "$detected_linux_libc" ]; then
        for musl_loader in /lib/ld-musl-*.so.1 /usr/lib/ld-musl-*.so.1; do
            if [ -e "$musl_loader" ]; then detected_linux_libc=musl; break; fi
        done
    fi
    case "$detected_linux_libc" in
        gnu|musl) ;;
        *) fail "$libc_failure_code" "无法确定 $libc_consumer 所需的 Linux libc" ;;
    esac
}

# 将 Unix 宿主映射到 Rust 官方 rustup-init target；Linux libc 无法确定时停止而不猜测。
host_rustup_target() {
    if [ -n "${AFH_TEST_HOST_OS:-}" ]; then
        rust_host_os=$AFH_TEST_HOST_OS
    else
        rust_host_os=$(uname -s 2>/dev/null) || fail 22 "无法为 rustup 探测操作系统"
    fi
    if [ -n "${AFH_TEST_HOST_ARCH:-}" ]; then
        rust_host_arch=$AFH_TEST_HOST_ARCH
    else
        rust_host_arch=$(uname -m 2>/dev/null) || fail 22 "无法为 rustup 探测 CPU 架构"
    fi
    case "$rust_host_os" in
        Darwin)
            case "$rust_host_arch" in
                x86_64|amd64) rustup_target=x86_64-apple-darwin ;;
                arm64|aarch64) rustup_target=aarch64-apple-darwin ;;
                *) fail 22 "rustup 不支持此 macOS 架构：$rust_host_arch" ;;
            esac
            ;;
        Linux)
            detect_linux_libc 22 rustup
            rust_libc=$detected_linux_libc
            case "$rust_host_arch" in
                x86_64|amd64) rustup_target=x86_64-unknown-linux-$rust_libc ;;
                arm64|aarch64) rustup_target=aarch64-unknown-linux-$rust_libc ;;
                *) fail 22 "rustup 不支持此 Linux 架构：$rust_host_arch" ;;
            esac
            ;;
        *) fail 22 "rustup 不支持此 Unix 操作系统：$rust_host_os" ;;
    esac
}

# 下载宿主匹配的官方 rustup-init，核对发布摘要后安装到标准当前用户根；PATH 由本门禁在完整预检后统一持久化。
install_rust() {
    requested_change=$1
    command -v curl >/dev/null 2>&1 || fail 22 "安装 Rust 需要 curl"
    host_rustup_target
    TEMP_DIR=$(mktemp -d) || fail 22 "无法创建临时目录"
    rustup_dist_base=${AFH_RUSTUP_DIST_BASE:-https://static.rust-lang.org/rustup/dist}
    release_base=$rustup_dist_base/$rustup_target
    installer_url=$release_base/rustup-init
    checksum_url=$release_base/rustup-init.sha256
    installer_path=$TEMP_DIR/rustup-init
    checksum_path=$TEMP_DIR/rustup-init.sha256
    printf '正在从 %s 安装或升级 Rust stable 到标准当前用户位置；PATH 将由门禁统一持久化。\n' "$release_base" >&2
    download "$installer_url" "$installer_path" || fail 22 "Rust 安装器下载失败"
    download "$checksum_url" "$checksum_path" || fail 22 "Rust 安装器校验和下载失败"
    expected_sum=$(awk '{ print $1; exit }' "$checksum_path")
    [ -n "$expected_sum" ] || fail 22 "Rust 安装器校验和为空"
    actual_sum=$(sha256_file "$installer_path")
    [ "$actual_sum" = "$expected_sum" ] || fail 22 "rustup-init SHA-256 校验失败"
    chmod +x "$installer_path" || fail 22 "无法把 rustup-init 设为可执行文件"
    prepare_managed_directory_path "$MANAGED_CARGO_HOME" "Rust Cargo 当前用户安装根"
    prepare_managed_directory_path "$MANAGED_RUSTUP_HOME" "Rust rustup 当前用户安装根"
    "$installer_path" -y --profile minimal --default-toolchain stable --no-modify-path || fail 22 "Rust 安装失败"
    CARGO_BIN_DIR=$MANAGED_CARGO_HOME/bin
    prepend_probe_path "$CARGO_BIN_DIR"
    RUST_CHANGED=$requested_change
    cleanup
    TEMP_DIR=
}

# 将宿主系统与 CPU 映射到 Node 官方制品命名，未知组合必须停止而非猜测。
host_node_tuple() {
    if [ -n "${AFH_TEST_HOST_OS:-}" ]; then
        host_os=$AFH_TEST_HOST_OS
    else
        host_os=$(uname -s 2>/dev/null) || fail 23 "无法探测操作系统"
    fi
    if [ -n "${AFH_TEST_HOST_ARCH:-}" ]; then
        host_arch=$AFH_TEST_HOST_ARCH
    else
        host_arch=$(uname -m 2>/dev/null) || fail 23 "无法探测 CPU 架构"
    fi
    case "$host_os" in
        Darwin) node_platform=darwin ;;
        Linux)
            node_platform=linux
            detect_linux_libc 26 Node.js
            ;;
        *) fail 23 "Node.js 不支持此 Unix 操作系统：$host_os" ;;
    esac
    case "$host_arch" in
        x86_64|amd64)
            node_arch=x64
            [ "$host_os" != Linux ] || [ "$detected_linux_libc" != musl ] || node_arch=x64-musl
            ;;
        arm64|aarch64)
            if [ "$host_os" = Linux ] && [ "$detected_linux_libc" = musl ]; then
                fail 26 "Node.js $NODE_REQUIREMENT 没有官方 Linux arm64 musl 制品"
            fi
            node_arch=arm64
            ;;
        *) fail 23 "Node.js 不支持此 CPU 架构：$host_arch" ;;
    esac
}

# 使用宿主已有 SHA-256 工具计算制品摘要，缺少校验能力时拒绝任何安装。
sha256_file() {
    checksum_path=$1
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$checksum_path" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$checksum_path" | awk '{print $1}'
    else
        fail 25 "校验下载制品需要 sha256sum 或 shasum"
    fi
}

# 从官方倒序索引选择当前最高 LTS 的最新补丁，用于安装或升级。
install_node() {
    requested_change=$1
    command -v curl >/dev/null 2>&1 || fail 26 "安装 Node.js 需要 curl"
    command -v tar >/dev/null 2>&1 || fail 26 "安装 Node.js 需要 tar"
    host_node_tuple
    node_dist_base=${AFH_NODE_DIST_BASE:-https://nodejs.org/dist}
    TEMP_DIR=$(mktemp -d) || fail 26 "无法创建临时目录"
    index_path=$TEMP_DIR/index.tab
    download "$node_dist_base/index.tab" "$index_path" || fail 26 "Node.js 发布版本索引下载失败"
    node_version=$(awk -F '\t' '
        NR > 1 && $1 ~ /^v[0-9]+\.[0-9]+\.[0-9]+$/ && $10 != "" && $10 != "-" {
            raw = substr($1, 2)
            split(raw, parts, ".")
            if (parts[1] > 24 ||
                (parts[1] == 24 && (parts[2] > 21 || (parts[2] == 21 && parts[3] >= 0)))) {
                if (!found || parts[1] > best_major ||
                    (parts[1] == best_major && (parts[2] > best_minor ||
                    (parts[2] == best_minor && parts[3] > best_patch)))) {
                    found = 1
                    best_major = parts[1]
                    best_minor = parts[2]
                    best_patch = parts[3]
                    best_version = $1
                }
            }
        }
        END { if (found) print best_version }
    ' "$index_path")
    [ -n "$node_version" ] || fail 26 "Node.js 发布版本索引中没有满足 $NODE_REQUIREMENT 的 LTS 稳定版"
    archive_name=node-$node_version-$node_platform-$node_arch.tar.gz
    archive_path=$TEMP_DIR/$archive_name
    sums_path=$TEMP_DIR/SHASUMS256.txt
    release_base=$node_dist_base/$node_version
    install_dir=$NODE_HOME/$node_version
    if [ -e "$install_dir" ] || [ -L "$install_dir" ]; then
        validate_existing_node_version_root "$install_dir"
    fi
    download "$release_base/SHASUMS256.txt" "$sums_path" || fail 26 "Node.js 校验和下载失败"
    expected_sum=$(awk -v name="$archive_name" '$2 == name { print $1; exit }' "$sums_path")
    [ -n "$expected_sum" ] || fail 26 "Node.js 校验和列表不包含 $archive_name"
    case "$expected_sum" in *[!0-9A-Fa-f]*) fail 26 "Node.js 校验和格式无效：$archive_name" ;; esac
    [ "${#expected_sum}" -eq 64 ] || fail 26 "Node.js 校验和格式无效：$archive_name"
    expected_sum=$(printf '%s' "$expected_sum" | tr 'A-F' 'a-f')
    node_marker=$(printf '%s\n%s\n%s' \
        '# managed by agent-first-harness development environment gate' \
        "node.version=$node_version" \
        "node.archive.sha256=$expected_sum")
    extracted_dir=$TEMP_DIR/node-$node_version-$node_platform-$node_arch
    if [ -e "$install_dir" ] || [ -L "$install_dir" ]; then
        validate_existing_node_version_root "$install_dir"
        [ "$(cat "$install_dir/.agent-first-harness-managed")" = "$node_marker" ] || fail 26 "Node.js 目标受管标记与已校验官方归档不一致：$install_dir"
    else
        printf '正在安装或升级到当前最高 LTS 的最新 Node.js %s，来源为 %s，目标为标准用户级目录。\n' "$node_version" "$release_base" >&2
        download "$release_base/$archive_name" "$archive_path" || fail 26 "Node.js 归档下载失败"
        actual_sum=$(sha256_file "$archive_path")
        [ "$actual_sum" = "$expected_sum" ] || fail 26 "Node.js SHA-256 校验失败"
        prepare_managed_directory_path "$NODE_HOME" "Node.js 受管安装根"
        tar -xzf "$archive_path" -C "$TEMP_DIR" || fail 26 "Node.js 归档解压失败"
        [ -x "$extracted_dir/bin/node" ] || fail 26 "Node.js 归档不包含预期可执行文件"
        [ -x "$extracted_dir/bin/npm" ] || fail 26 "Node.js 归档不包含预期 npm 可执行文件"
        extracted_node_version=$("$extracted_dir/bin/node" --version 2>/dev/null) || fail 26 "Node.js 归档版本探测失败"
        [ "$extracted_node_version" = "$node_version" ] || fail 26 "Node.js 归档版本与已选择稳定版不一致：期望 ${node_version}，实际 $extracted_node_version"
        mv "$extracted_dir" "$install_dir" || fail 26 "无法完成 Node.js 安装"
        printf '%s\n' "$node_marker" > "$install_dir/.agent-first-harness-managed" || fail 26 "无法写入 Node.js 版本所有权标记"
    fi
    SELECTED_NODE_VERSION=$node_version
    NODE_BIN_DIR=$install_dir/bin
    prepend_probe_path "$NODE_BIN_DIR"
    link_user_tool "$NODE_BIN_DIR/node" node "$NODE_HOME"
    link_user_tool "$NODE_BIN_DIR/npm" npm "$NODE_HOME"
    for optional_node_tool in npx corepack; do
        if [ -x "$NODE_BIN_DIR/$optional_node_tool" ] && [ ! -d "$NODE_BIN_DIR/$optional_node_tool" ]; then
            link_user_tool "$NODE_BIN_DIR/$optional_node_tool" "$optional_node_tool" "$NODE_HOME"
        fi
    done
    NODE_CHANGED=$requested_change
    cleanup
    TEMP_DIR=
}

# 仅在 GUI 项目中通过 Node 自带 npm 安装缺失 pnpm，或升级低于门禁的 pnpm。
install_pnpm() {
    requested_change=$1
    npm_path=$(find_tool npm 2>/dev/null || true)
    [ -n "$npm_path" ] || fail 28 "为 GUI 开发安装 pnpm 需要 npm"
    prepare_managed_directory_path "$PNPM_HOME" "npm 当前用户全局前缀"
    prepare_managed_directory_path "$PNPM_HOME/lib/node_modules" "npm 当前用户全局包目录"
    validate_managed_directory_path "$PNPM_HOME/lib/node_modules/pnpm" "pnpm 当前用户全局包目标"
    printf '正在从官方 npm 软件包仓库全局安装或升级 pnpm %s 到标准当前用户前缀。\n' "$PNPM_INSTALL_REQUIREMENT" >&2
    PATH=$PROBE_PATH${PATH:+:$PATH} "$npm_path" install --global --prefix "$PNPM_HOME" "$PNPM_INSTALL_REQUIREMENT" --registry "$PNPM_REGISTRY" --ignore-scripts || fail 28 "pnpm 安装失败"
    PNPM_BIN_DIR=$PNPM_HOME/bin
    [ -x "$PNPM_BIN_DIR/pnpm" ] && [ ! -d "$PNPM_BIN_DIR/pnpm" ] || fail 28 "npm 未在标准当前用户前缀生成 pnpm 可执行文件"
    validate_installed_user_tool_range "$PNPM_BIN_DIR/pnpm" pnpm "$PNPM_HOME"
    if [ -e "$PNPM_BIN_DIR/pnpx" ] || [ -L "$PNPM_BIN_DIR/pnpx" ]; then
        validate_installed_user_tool_range "$PNPM_BIN_DIR/pnpx" pnpx "$PNPM_HOME"
    fi
    prepend_probe_path "$PNPM_BIN_DIR"
    USER_BIN_DIR=$PNPM_BIN_DIR
    PNPM_CHANGED=$requested_change
}

# 把目录加入当前门禁探测路径并去重；空目录永远不会退化为当前工作目录。
prepend_probe_path() {
    candidate=$1
    [ -n "$candidate" ] && [ -d "$candidate" ] || return 0
    case ":$PROBE_PATH:" in
        *":$candidate:"*) ;;
        *) PROBE_PATH=$candidate${PROBE_PATH:+:$PROBE_PATH} ;;
    esac
}

# 只创建一个已经验证父目录的普通目录；既有符号链接或非目录对象一律拒绝。
ensure_plain_directory() {
    afh_directory_path=$1
    afh_directory_label=$2
    [ ! -L "$afh_directory_path" ] || fail 24 "$afh_directory_label 不能是符号链接：$afh_directory_path"
    if [ -e "$afh_directory_path" ]; then
        [ -d "$afh_directory_path" ] || fail 24 "$afh_directory_label 不是普通目录：$afh_directory_path"
    else
        mkdir "$afh_directory_path" || fail 24 "无法创建${afh_directory_label}：$afh_directory_path"
    fi
    [ -d "$afh_directory_path" ] && [ ! -L "$afh_directory_path" ] || fail 24 "$afh_directory_label 在创建时发生变化：$afh_directory_path"
}

# 逐级验证当前用户受管安装根；测试重定向也只能位于隔离 HOME 的同级测试根内。
validate_managed_directory_path() {
    afh_candidate=$1
    afh_directory_label=$2
    case "$afh_candidate" in
        "$USER_HOME"/*)
            afh_trusted_root=$USER_HOME
            afh_relative=${afh_candidate#"$USER_HOME"/}
            ;;
        *)
            [ "$TEST_MODE" = 1 ] || fail 24 "$afh_directory_label 必须位于当前用户目录内：$afh_candidate"
            afh_test_root=${USER_HOME%/*}
            case "$afh_candidate" in
                "$afh_test_root"/*)
                    afh_trusted_root=$afh_test_root
                    afh_relative=${afh_candidate#"$afh_test_root"/}
                    ;;
                *) fail 24 "测试受管安装根必须位于隔离 HOME 的同级测试根内：$afh_candidate" ;;
            esac
            ;;
    esac
    afh_directory_cursor=$afh_trusted_root
    afh_old_ifs=$IFS
    afh_glob_was_enabled=0
    case $- in
        *f*) ;;
        *) set -f; afh_glob_was_enabled=1 ;;
    esac
    IFS=/
    for afh_component in $afh_relative; do
        case "$afh_component" in
            ''|.|..) fail 24 "$afh_directory_label 包含不安全路径组件：$afh_candidate" ;;
        esac
        afh_directory_cursor=$afh_directory_cursor/$afh_component
        [ ! -L "$afh_directory_cursor" ] || fail 24 "$afh_directory_label 的路径组件不能是符号链接：$afh_directory_cursor"
        if [ -e "$afh_directory_cursor" ] && [ ! -d "$afh_directory_cursor" ]; then
            fail 24 "$afh_directory_label 的路径组件不是普通目录：$afh_directory_cursor"
        fi
    done
    IFS=$afh_old_ifs
    [ "$afh_glob_was_enabled" -eq 0 ] || set +f
}

# 预检通过后逐级创建同一路径，每一步都复核没有被符号链接替换。
prepare_managed_directory_path() {
    afh_candidate=$1
    afh_directory_label=$2
    validate_managed_directory_path "$afh_candidate" "$afh_directory_label"
    case "$afh_candidate" in
        "$USER_HOME"/*)
            afh_trusted_root=$USER_HOME
            afh_relative=${afh_candidate#"$USER_HOME"/}
            ;;
        *)
            afh_trusted_root=${USER_HOME%/*}
            afh_relative=${afh_candidate#"$afh_trusted_root"/}
            ;;
    esac
    afh_directory_cursor=$afh_trusted_root
    afh_old_ifs=$IFS
    afh_glob_was_enabled=0
    case $- in
        *f*) ;;
        *) set -f; afh_glob_was_enabled=1 ;;
    esac
    IFS=/
    for afh_component in $afh_relative; do
        afh_directory_cursor=$afh_directory_cursor/$afh_component
        if [ ! -e "$afh_directory_cursor" ]; then
            mkdir "$afh_directory_cursor" || fail 24 "无法创建${afh_directory_label}：$afh_directory_cursor"
        fi
        [ -d "$afh_directory_cursor" ] && [ ! -L "$afh_directory_cursor" ] || fail 24 "$afh_directory_label 在创建时发生变化：$afh_directory_cursor"
    done
    IFS=$afh_old_ifs
    [ "$afh_glob_was_enabled" -eq 0 ] || set +f
}

# 标准当前用户 PATH 块只有这一份规范字节；预检与写入共用，避免 marker 与正文分离。
profile_managed_block() {
    printf '%s\n' \
        '# agent-first-harness: standard current-user tool PATH' \
        'user_path_result=' \
        'user_cargo_home=${CARGO_HOME:-$HOME/.cargo}' \
        'case "$user_cargo_home" in "$HOME"/*) case "$user_cargo_home" in *:*|*/../*|*/..|*/./*|*/.) user_cargo_home= ;; esac ;; *) user_cargo_home= ;; esac' \
        'for user_path_entry in "${user_cargo_home:+$user_cargo_home/bin}" "$HOME/.local/bin"; do' \
        '    [ -n "$user_path_entry" ] || continue' \
        '    case "$user_path_entry" in /*) ;; *) continue ;; esac' \
        '    case ":$user_path_result:" in *":$user_path_entry:"*) ;; *) [ -n "$user_path_result" ] && user_path_result=$user_path_result:$user_path_entry || user_path_result=$user_path_entry ;; esac' \
        'done' \
        'user_path_old_ifs=$IFS' \
        'user_path_glob_was_enabled=0' \
        'case $- in *f*) ;; *) set -f; user_path_glob_was_enabled=1 ;; esac' \
        'IFS=:' \
        'for user_path_entry in ${PATH-}; do' \
        '    [ -n "$user_path_entry" ] || continue' \
        '    case "$user_path_entry" in /*) ;; *) continue ;; esac' \
        '    case ":$user_path_result:" in *":$user_path_entry:"*) ;; *) [ -n "$user_path_result" ] && user_path_result=$user_path_result:$user_path_entry || user_path_result=$user_path_entry ;; esac' \
        'done' \
        'IFS=$user_path_old_ifs' \
        '[ "$user_path_glob_was_enabled" -eq 0 ] || set +f' \
        'PATH=$user_path_result' \
        'export PATH' \
        'unset user_path_result user_path_entry user_cargo_home user_path_old_ifs user_path_glob_was_enabled' \
        '# agent-first-harness: end standard current-user tool PATH'
}

# 配置文件写入前必须已经是普通文件；受管块必须唯一、正序且正文逐字匹配。
validate_profile_file_shape() {
    afh_profile_path=$1
    [ ! -L "$afh_profile_path" ] || fail 24 "shell profile 不能是符号链接：$afh_profile_path"
    if [ -e "$afh_profile_path" ] && [ ! -f "$afh_profile_path" ]; then
        fail 24 "shell profile 不是普通文件：$afh_profile_path"
    fi
    [ -f "$afh_profile_path" ] || return 0
    afh_path_marker='# agent-first-harness: standard current-user tool PATH'
    afh_path_end_marker='# agent-first-harness: end standard current-user tool PATH'
    afh_path_marker_count=$(grep -Fxc "$afh_path_marker" "$afh_profile_path" || true)
    afh_path_end_marker_count=$(grep -Fxc "$afh_path_end_marker" "$afh_profile_path" || true)
    [ "$afh_path_marker_count" -eq "$afh_path_end_marker_count" ] && [ "$afh_path_marker_count" -le 1 ] || \
        fail 24 "shell profile 中的标准用户 PATH 管理块不完整或重复：$afh_profile_path"
    if [ "$afh_path_marker_count" -eq 1 ]; then
        afh_actual_block=$(awk -v start="$afh_path_marker" -v finish="$afh_path_end_marker" \
            '$0 == start { capture = 1 } capture { print } capture && $0 == finish { exit }' "$afh_profile_path")
        afh_expected_block=$(profile_managed_block)
        [ "$afh_actual_block" = "$afh_expected_block" ] || \
            fail 24 "shell profile 中的标准用户 PATH 管理块顺序或正文已损坏：$afh_profile_path"
    fi
}

validate_managed_file_shape() {
    afh_managed_file=$1
    afh_managed_label=$2
    afh_expected_marker='# managed by agent-first-harness development environment gate'
    [ ! -L "$afh_managed_file" ] || fail 24 "$afh_managed_label 不能是符号链接：$afh_managed_file"
    if [ -e "$afh_managed_file" ]; then
        [ -f "$afh_managed_file" ] || fail 24 "$afh_managed_label 不是普通文件：$afh_managed_file"
        [ "$(sed -n '1p' "$afh_managed_file")" = "$afh_expected_marker" ] || fail 24 "$afh_managed_label 已存在且不受门禁管理：$afh_managed_file"
    fi
}

# 验证标准 ~/.local/bin 的每个固定路径组件都是普通目录，禁止沿预置符号链接写出预期范围。
validate_user_tool_directory_path() {
    afh_local_dir=${HOME:?必须设置 HOME}/.local
    afh_user_bin=$afh_local_dir/bin
    for afh_directory in "$afh_local_dir" "$afh_user_bin"; do
        [ ! -L "$afh_directory" ] || fail 24 "用户级工具目录组件不能是符号链接：$afh_directory"
        if [ -e "$afh_directory" ] && [ ! -d "$afh_directory" ]; then
            fail 24 "用户级工具目录组件不是普通目录：$afh_directory"
        fi
    done
}

# 逐级创建并复核标准用户 bin；不用 mkdir -p 跨越未经验证的中间组件。
prepare_user_tool_directory() {
    validate_user_tool_directory_path
    for afh_directory in "$afh_local_dir" "$afh_user_bin"; do
        if [ ! -e "$afh_directory" ]; then
            mkdir "$afh_directory" || fail 24 "无法创建用户级工具目录组件：$afh_directory"
        fi
        [ -d "$afh_directory" ] && [ ! -L "$afh_directory" ] || fail 24 "用户级工具目录组件在创建时发生变化：$afh_directory"
    done
    USER_BIN_DIR=$afh_user_bin
}

# 把存在父目录的路径归一化为物理绝对路径；不能证明时保持失败关闭。
physical_path_with_existing_parent() {
    afh_candidate=$1
    case "$afh_candidate" in
        /*) ;;
        *) return 1 ;;
    esac
    afh_parent=${afh_candidate%/*}
    afh_name=${afh_candidate##*/}
    afh_physical_parent=$(CDPATH= cd -P "$afh_parent" 2>/dev/null && pwd -P) || return 1
    printf '%s/%s\n' "$afh_physical_parent" "$afh_name"
}

# 解析最终组件及其 symlink 链，确保 containment 检查不会停在可逃逸的末端链接上。
physical_existing_path() {
    afh_resolved_candidate=$1
    afh_symlink_hops=0
    while [ -L "$afh_resolved_candidate" ]; do
        afh_symlink_hops=$((afh_symlink_hops + 1))
        [ "$afh_symlink_hops" -le 40 ] || return 1
        afh_link_target=$(readlink "$afh_resolved_candidate") || return 1
        case "$afh_link_target" in
            /*) afh_resolved_candidate=$afh_link_target ;;
            *) afh_resolved_candidate=${afh_resolved_candidate%/*}/$afh_link_target ;;
        esac
        afh_resolved_candidate=$(physical_path_with_existing_parent "$afh_resolved_candidate") || return 1
    done
    [ -e "$afh_resolved_candidate" ] || return 1
    physical_path_with_existing_parent "$afh_resolved_candidate"
}

# 把已经逐级验证过的用户安装根映射为物理路径；末端尚未创建时也不需要提前产生写入。
physical_managed_path() {
    afh_managed_candidate=$1
    case "$afh_managed_candidate" in
        "$USER_HOME"/*)
            afh_managed_trusted_root=$USER_HOME
            afh_managed_relative=${afh_managed_candidate#"$USER_HOME"/}
            ;;
        *)
            [ "$TEST_MODE" = 1 ] || return 1
            afh_managed_trusted_root=${USER_HOME%/*}
            case "$afh_managed_candidate" in
                "$afh_managed_trusted_root"/*)
                    afh_managed_relative=${afh_managed_candidate#"$afh_managed_trusted_root"/}
                    ;;
                *) return 1 ;;
            esac
            ;;
    esac
    afh_managed_physical_root=$(CDPATH= cd -P "$afh_managed_trusted_root" 2>/dev/null && pwd -P) || return 1
    printf '%s/%s\n' "$afh_managed_physical_root" "$afh_managed_relative"
}

# 在访问 Node.js 发布索引前逐项核对既有版本根；未知版本尚不能核对官方摘要，但所有权、
# 目录形态、摘要格式、可执行文件范围与本地报告版本都必须已经自洽。
validate_existing_node_version_root() {
    afh_node_version_root=$1
    afh_node_version_name=${afh_node_version_root##*/}
    [ ! -L "$afh_node_version_root" ] && [ -d "$afh_node_version_root" ] || \
        fail 24 "Node.js 版本根必须是普通目录：$afh_node_version_root"
    case "$afh_node_version_name" in
        v*) afh_node_version_release=${afh_node_version_name#v} ;;
        *) fail 24 "Node.js 版本根名称不可识别：$afh_node_version_root" ;;
    esac
    case "$afh_node_version_release" in
        *-*|*+*|*[!0-9.]*) fail 24 "Node.js 版本根名称不可识别：$afh_node_version_root" ;;
        *.*.*) ;;
        *) fail 24 "Node.js 版本根名称不可识别：$afh_node_version_root" ;;
    esac
    afh_node_version_parts=$(printf '%s\n' "$afh_node_version_release" | awk -F. '{print NF ":" $1 ":" $2 ":" $3}')
    case "$afh_node_version_parts" in
        3:[0-9]*:[0-9]*:[0-9]*) ;;
        *) fail 24 "Node.js 版本根名称不可识别：$afh_node_version_root" ;;
    esac

    afh_node_marker=$afh_node_version_root/.agent-first-harness-managed
    validate_managed_file_shape "$afh_node_marker" "Node.js 版本所有权标记"
    [ -f "$afh_node_marker" ] || fail 24 "Node.js 版本根缺少受管所有权标记：$afh_node_version_root"
    afh_node_marker_lines=$(awk 'END { print NR + 0 }' "$afh_node_marker")
    [ "$afh_node_marker_lines" -eq 3 ] || fail 24 "Node.js 版本所有权标记格式无效：$afh_node_version_root"
    afh_node_marker_version=$(sed -n '2p' "$afh_node_marker")
    [ "$afh_node_marker_version" = "node.version=$afh_node_version_name" ] || \
        fail 24 "Node.js 版本所有权标记与目录名称不一致：$afh_node_version_root"
    afh_node_marker_sum_line=$(sed -n '3p' "$afh_node_marker")
    case "$afh_node_marker_sum_line" in
        node.archive.sha256=*) afh_node_marker_sum=${afh_node_marker_sum_line#node.archive.sha256=} ;;
        *) fail 24 "Node.js 版本所有权标记摘要格式无效：$afh_node_version_root" ;;
    esac
    case "$afh_node_marker_sum" in
        ''|*[!0-9A-Fa-f]*) fail 24 "Node.js 版本所有权标记摘要格式无效：$afh_node_version_root" ;;
    esac
    [ "${#afh_node_marker_sum}" -eq 64 ] || fail 24 "Node.js 版本所有权标记摘要格式无效：$afh_node_version_root"

    afh_node_version_root_physical=$(physical_managed_path "$afh_node_version_root") || \
        fail 24 "无法确认 Node.js 版本根：$afh_node_version_root"
    for afh_node_tool in node npm; do
        afh_node_tool_path=$afh_node_version_root/bin/$afh_node_tool
        [ -x "$afh_node_tool_path" ] && [ ! -d "$afh_node_tool_path" ] || \
            fail 24 "Node.js 版本根缺少可执行文件 $afh_node_tool：$afh_node_version_root"
        afh_node_tool_physical=$(physical_existing_path "$afh_node_tool_path") || \
            fail 24 "无法确认 Node.js 版本根中的 $afh_node_tool：$afh_node_version_root"
        case "$afh_node_tool_physical" in
            "$afh_node_version_root_physical"/*) ;;
            *) fail 24 "Node.js 版本根中的 $afh_node_tool 逃逸受管目录：$afh_node_version_root" ;;
        esac
    done
    afh_existing_node_version=$("$afh_node_version_root/bin/node" --version 2>/dev/null) || \
        fail 24 "Node.js 版本根中的 node 无法执行：$afh_node_version_root"
    [ "$afh_existing_node_version" = "$afh_node_version_name" ] || \
        fail 24 "Node.js 版本根中的 node 版本与目录名称不一致：期望 ${afh_node_version_name}，实际 $afh_existing_node_version"
}

# NODE_HOME 是专用的版本根；在任何网络读取前枚举所有既有对象，拒绝未知、未标记或逃逸内容。
preflight_existing_node_version_roots() {
    validate_managed_directory_path "$NODE_HOME" "Node.js 受管安装根"
    [ -d "$NODE_HOME" ] || return 0
    for afh_existing_node_root in "$NODE_HOME"/* "$NODE_HOME"/.[!.]* "$NODE_HOME"/..?*; do
        [ -e "$afh_existing_node_root" ] || [ -L "$afh_existing_node_root" ] || continue
        validate_existing_node_version_root "$afh_existing_node_root"
    done
}

# 在产生下载或安装副作用前确认同名稳定入口可由本门禁安全替换。
validate_user_tool_destination() {
    tool_name=$1
    managed_root=$2
    validate_user_tool_directory_path
    destination=$afh_user_bin/$tool_name
    if [ -e "$destination" ] && [ ! -L "$destination" ]; then
        fail 24 "用户级工具目标已存在且不受门禁管理：$destination"
    fi
    [ -L "$destination" ] || return 0
    managed_root_physical=$(physical_managed_path "$managed_root") || fail 24 "无法确认受管安装根：$managed_root"
    existing_target=$(readlink "$destination") || fail 24 "无法读取既有用户级工具链接：$destination"
    case "$existing_target" in
        /*) existing_target_path=$existing_target ;;
        *) existing_target_path=$afh_user_bin/$existing_target ;;
    esac
    existing_target_physical=$(physical_existing_path "$existing_target_path") || fail 24 "既有用户级工具链接无法证明受管归属：$destination"
    case "$existing_target_physical" in
        "$managed_root_physical"/*) ;;
        *) fail 24 "既有用户级工具链接不属于当前受管安装根：$destination" ;;
    esac
}

# npm 返回后再次解析包装器最终目标；安装前安全不代表安装器没有新建逃逸链接。
validate_installed_user_tool_range() {
    installed_tool_path=$1
    installed_tool_name=$2
    installed_managed_root=$3
    [ -x "$installed_tool_path" ] && [ ! -d "$installed_tool_path" ] || \
        fail 24 "npm 生成的 $installed_tool_name 包装器不可执行或不是普通工具：$installed_tool_path"
    installed_root_physical=$(physical_managed_path "$installed_managed_root") || \
        fail 24 "无法确认 npm 当前用户全局前缀：$installed_managed_root"
    installed_tool_physical=$(physical_existing_path "$installed_tool_path") || \
        fail 24 "无法确认 npm 生成的 $installed_tool_name 包装器最终目标：$installed_tool_path"
    case "$installed_tool_physical" in
        "$installed_root_physical"/*) ;;
        *) fail 24 "npm 生成的 $installed_tool_name 包装器逃逸标准当前用户前缀：$installed_tool_path" ;;
    esac
}

# 在门禁拥有的稳定用户 bin 中原子替换单个工具链接；既有链接也必须指向同一受管安装根。
link_user_tool() {
    source_path=$1
    tool_name=$2
    managed_root=$3
    [ -x "$source_path" ] && [ ! -d "$source_path" ] || fail 24 "用户级工具源不可执行：$source_path"
    validate_user_tool_destination "$tool_name" "$managed_root"
    prepare_user_tool_directory
    user_bin=$USER_BIN_DIR
    managed_root_physical=$(physical_managed_path "$managed_root") || fail 24 "无法确认受管安装根：$managed_root"
    source_physical=$(physical_existing_path "$source_path") || fail 24 "无法确认用户级工具源范围：$source_path"
    case "$source_physical" in
        "$managed_root_physical"/*) ;;
        *) fail 24 "用户级工具源不在受管安装根内：$source_path" ;;
    esac
    destination=$user_bin/$tool_name
    LINK_TEMP_DIR=$(mktemp -d "$user_bin/.link-$tool_name.XXXXXX") || fail 24 "无法创建用户级工具临时目录：$tool_name"
    [ -d "$LINK_TEMP_DIR" ] && [ ! -L "$LINK_TEMP_DIR" ] || fail 24 "用户级工具临时目录不安全：$tool_name"
    temporary_link=$LINK_TEMP_DIR/$tool_name
    ln -s "$source_path" "$temporary_link" || fail 24 "无法建立用户级工具链接：$tool_name"
    if ! mv -f "$temporary_link" "$destination"; then
        rm -f "$temporary_link"
        fail 24 "无法提交用户级工具链接：$tool_name"
    fi
    rmdir "$LINK_TEMP_DIR" || fail 24 "无法清理用户级工具临时目录：$tool_name"
    LINK_TEMP_DIR=
}

# 以同目录临时文件把标准当前用户目录直接加入 PATH；不创建 Harness 私有环境文件或变量。
ensure_profile_path() {
    profile_path=$1
    path_marker='# agent-first-harness: standard current-user tool PATH'
    path_end_marker='# agent-first-harness: end standard current-user tool PATH'
    legacy_source_line='[ -r "$HOME/.config/agent-first-harness/env.sh" ] && . "$HOME/.config/agent-first-harness/env.sh"'
    if [ -e "$profile_path" ] && { [ ! -f "$profile_path" ] || [ -L "$profile_path" ]; }; then
        fail 24 "shell profile 不是可安全更新的普通文件：$profile_path"
    fi
    has_path_marker=0
    has_path_end_marker=0
    has_legacy_source=0
    if [ -f "$profile_path" ]; then
        grep -Fqx "$path_marker" "$profile_path" && has_path_marker=1
        grep -Fqx "$path_end_marker" "$profile_path" && has_path_end_marker=1
        grep -Fqx "$legacy_source_line" "$profile_path" && has_legacy_source=1
    fi
    [ "$has_path_marker" -eq "$has_path_end_marker" ] || fail 24 "shell profile 中的标准用户 PATH 管理块不完整：$profile_path"
    if [ "$has_path_marker" -eq 1 ] && [ "$has_legacy_source" -eq 0 ]; then
        return
    fi
    profile_parent=${profile_path%/*}
    profile_name=${profile_path##*/}
    umask 077
    profile_temp=$(mktemp "$profile_parent/.$profile_name.agent-first-harness.tmp.XXXXXX") || fail 24 "无法创建 shell profile 临时文件：$profile_path"
    PROFILE_TEMP_FILE=$profile_temp
    profile_snapshot=
    profile_existed=0
    if [ -f "$profile_path" ]; then
        profile_existed=1
        profile_snapshot=$(mktemp "$profile_parent/.$profile_name.agent-first-harness.snapshot.XXXXXX") || fail 24 "无法创建 shell profile 快照：$profile_path"
        PROFILE_SNAPSHOT_FILE=$profile_snapshot
        cp -p "$profile_path" "$profile_snapshot" || fail 24 "无法快照 shell profile：$profile_path"
        cp -p "$profile_path" "$profile_temp" || fail 24 "无法复制 shell profile：$profile_path"
        if [ "$has_legacy_source" -eq 1 ]; then
            : > "$profile_temp" || fail 24 "无法准备旧环境入口迁移：$profile_path"
            while IFS= read -r profile_line || [ -n "$profile_line" ]; do
                [ "$profile_line" = "$legacy_source_line" ] || printf '%s\n' "$profile_line" >> "$profile_temp"
            done < "$profile_snapshot"
        fi
    fi
    if [ "$has_path_marker" -eq 0 ]; then
        printf '\n' >> "$profile_temp" || fail 24 "无法准备 shell profile 更新：$profile_path"
        profile_managed_block >> "$profile_temp" || fail 24 "无法准备 shell profile 更新：$profile_path"
    fi
    [ -f "$profile_temp" ] && [ ! -L "$profile_temp" ] || fail 24 "shell profile 临时文件不安全：$profile_path"
    validate_profile_file_shape "$profile_path"
    if [ "$profile_existed" -eq 1 ]; then
        cmp -s "$profile_path" "$profile_snapshot" || fail 24 "shell profile 在提交前被并发修改：$profile_path"
    elif [ -e "$profile_path" ] || [ -L "$profile_path" ]; then
        fail 24 "shell profile 在提交前被并发创建：$profile_path"
    fi
    mv -f "$profile_temp" "$profile_path" || fail 24 "无法原子提交 shell profile：$profile_path"
    PROFILE_TEMP_FILE=
    if [ -n "$profile_snapshot" ]; then
        rm -f "$profile_snapshot" || fail 24 "无法清理 shell profile 快照：$profile_path"
        PROFILE_SNAPSHOT_FILE=
    fi
    [ -f "$profile_path" ] && [ ! -L "$profile_path" ] && \
        grep -Fqx "$path_marker" "$profile_path" && grep -Fqx "$path_end_marker" "$profile_path" || \
        fail 24 "shell profile 写入后复核失败：$profile_path"
    ! grep -Fqx "$legacy_source_line" "$profile_path" || fail 24 "shell profile 仍引用旧的 Harness 私有环境文件：$profile_path"
}

# 只为可证明会加载所写用户配置的常见 shell 建立持久化；未知 shell 在任何安装前失败关闭。
validate_user_login_shell() {
    LOGIN_SHELL=${SHELL:-/bin/sh}
    [ -x "$LOGIN_SHELL" ] || fail 24 "无法执行用户登录 shell：$LOGIN_SHELL"
    LOGIN_SHELL_NAME=${LOGIN_SHELL##*/}
    case "$LOGIN_SHELL_NAME" in
        sh|sh.exe|dash|ash|ksh|ksh93|mksh|bash|bash.exe|zsh|fish) ;;
        *) fail 24 "不支持自动持久化 PATH 的用户 shell：$LOGIN_SHELL_NAME" ;;
    esac
}

# 任何安装或持久配置写入前，先从不含瞬时探测目录的全新 login shell 复核所有工具。
# passed 项必须路径与版本完全相同；待安装/升级项的当前探测与持久 PATH 也必须同时缺席或解析到同一路径。
# 如果本轮将写入标准用户 PATH 块，还要先按写入后的前缀顺序复核，不得先安装再发现隐藏的同名工具。
verify_persisted_passed_tools_before_write() {
    preflight_git_path=
    preflight_rustup_path=
    preflight_rustc_path=
    preflight_cargo_path=
    preflight_node_path=
    preflight_npm_path=
    preflight_pnpm_path=
    preflight_check_git_shadow=0
    preflight_check_rust_shadow=0
    preflight_check_node_shadow=0
    preflight_check_pnpm_shadow=0

    if [ "$GIT_STATUS" = passed ]; then
        preflight_git_path=$git_path
    else
        preflight_check_git_shadow=1
    fi
    if [ "$RUST_STATUS" = passed ]; then
        preflight_rustup_path=$rustup_path
        preflight_rustc_path=$rustc_path
        preflight_cargo_path=$cargo_path
    else
        preflight_check_rust_shadow=1
    fi
    if [ "$NODE_STATUS" = passed ]; then
        preflight_node_path=$node_path
        preflight_npm_path=$npm_path
    else
        preflight_check_node_shadow=1
    fi
    if [ "$PNPM_REQUIRED" -eq 1 ]; then
        if [ "$PNPM_STATUS" = passed ]; then
            preflight_pnpm_path=$pnpm_path
        else
            preflight_check_pnpm_shadow=1
        fi
    fi

    preflight_projected_path=
    if { [ "$RUST_STATUS" != passed ] || [ "$NODE_STATUS" != passed ] || {
        [ "$PNPM_REQUIRED" -eq 1 ] && [ "$PNPM_STATUS" != passed ];
    }; } && ! { [ "$TEST_MODE" = 1 ] && [ "${AFH_SKIP_PERSIST_PATH:-0}" = 1 ]; }; then
        preflight_cargo_bin=$MANAGED_CARGO_HOME/bin
        preflight_user_bin=$PNPM_HOME/bin
        case "$LOGIN_SHELL_NAME" in
            fish) preflight_projected_path=$preflight_user_bin:$preflight_cargo_bin ;;
            *) preflight_projected_path=$preflight_cargo_bin:$preflight_user_bin ;;
        esac
    fi

    FRESH_VERIFY_DIR=$(mktemp -d) || fail 24 "无法创建写入前新 shell 复探临时目录"
    [ -d "$FRESH_VERIFY_DIR" ] && [ ! -L "$FRESH_VERIFY_DIR" ] || fail 24 "写入前新 shell 复探临时目录不安全"
    preflight_verifier=$(mktemp "$FRESH_VERIFY_DIR/check.XXXXXX") || fail 24 "无法创建写入前新 shell 复探脚本"
    preflight_launcher=$(mktemp "$FRESH_VERIFY_DIR/launch.XXXXXX") || fail 24 "无法创建写入前新 shell 复探启动器"
    FRESH_VERIFY_FILE=$preflight_verifier
    {
        printf '%s\n' '#!/bin/sh'
        printf '%s\n' 'set -eu'
        printf '%s\n' 'if [ -n "${AFH_PROJECTED_PATH-}" ]; then PATH=$AFH_PROJECTED_PATH${PATH:+:$PATH}; export PATH; fi'
        printf '%s\n' 'check_afh_preflight_tool() {'
        printf '%s\n' '    afh_tool_name=$1'
        printf '%s\n' '    afh_expected_path=$2'
        printf '%s\n' '    afh_expected_version=$3'
        printf '%s\n' '    afh_resolved=$(command -v "$afh_tool_name" 2>/dev/null) || { printf "ERROR_TOOL=%s\n" "$afh_tool_name"; exit 51; }'
        printf '%s\n' '    [ "$afh_resolved" = "$afh_expected_path" ] || { printf "ERROR_TOOL=%s\n" "$afh_tool_name"; exit 52; }'
        printf '%s\n' '    afh_actual_version=$("$afh_resolved" --version 2>/dev/null) || { printf "ERROR_TOOL=%s\n" "$afh_tool_name"; exit 53; }'
        printf '%s\n' '    [ "$afh_actual_version" = "$afh_expected_version" ] || { printf "ERROR_TOOL=%s\n" "$afh_tool_name"; exit 54; }'
        printf '%s\n' '    if [ "$afh_tool_name" = rustc ]; then'
        printf '%s\n' '        afh_verbose=$("$afh_resolved" -vV 2>/dev/null) || { printf "ERROR_TOOL=%s\n" "$afh_tool_name"; exit 55; }'
        printf '%s\n' '        afh_release_tail=${afh_expected_version#rustc }'
        printf '%s\n' '        afh_expected_release=${afh_release_tail%% *}'
        printf '%s\n' "        afh_release_count=\$(printf '%s\\n' \"\$afh_verbose\" | awk -F': ' '\$1 == \"release\" { count++ } END { print count + 0 }')"
        printf '%s\n' "        afh_host_count=\$(printf '%s\\n' \"\$afh_verbose\" | awk -F': ' '\$1 == \"host\" { count++ } END { print count + 0 }')"
        printf '%s\n' "        afh_verbose_release=\$(printf '%s\\n' \"\$afh_verbose\" | awk -F': ' '\$1 == \"release\" { print \$2 }')"
        printf '%s\n' "        afh_verbose_host=\$(printf '%s\\n' \"\$afh_verbose\" | awk -F': ' '\$1 == \"host\" { print \$2 }')"
        printf '%s\n' '        [ "$afh_release_count" -eq 1 ] && [ "$afh_host_count" -eq 1 ] && [ "$afh_verbose_release" = "$afh_expected_release" ] && [ "$afh_verbose_host" = "$AFH_EXPECTED_RUST_HOST" ] || { printf "ERROR_TOOL=%s\n" "$afh_tool_name"; exit 55; }'
        printf '%s\n' '    fi'
        printf '%s\n' '}'
        printf '%s\n' 'check_afh_no_shell_shadow() {'
        printf '%s\n' '    afh_tool_name=$1'
        printf '%s\n' '    afh_current_path=$2'
        printf '%s\n' '    if afh_resolved=$(command -v "$afh_tool_name" 2>/dev/null); then'
        printf '%s\n' '        case "$afh_resolved" in /*) ;; *) printf "ERROR_TOOL=%s\n" "$afh_tool_name"; exit 56 ;; esac'
        printf '%s\n' '        [ -n "$afh_current_path" ] && [ "$afh_resolved" = "$afh_current_path" ] || { printf "ERROR_TOOL=%s\n" "$afh_tool_name"; exit 57; }'
        printf '%s\n' '    elif [ -n "$afh_current_path" ]; then'
        printf '%s\n' '        printf "ERROR_TOOL=%s\n" "$afh_tool_name"; exit 58'
        printf '%s\n' '    fi'
        printf '%s\n' '}'
        printf '%s\n' 'if [ -n "${AFH_EXPECTED_GIT_PATH-}" ]; then check_afh_preflight_tool git "$AFH_EXPECTED_GIT_PATH" "$AFH_EXPECTED_GIT"; elif [ "$AFH_CHECK_GIT_SHADOW" = 1 ]; then check_afh_no_shell_shadow git "$AFH_CURRENT_GIT_PATH"; fi'
        printf '%s\n' 'if [ -n "${AFH_EXPECTED_RUSTUP_PATH-}" ]; then'
        printf '%s\n' '    check_afh_preflight_tool rustup "$AFH_EXPECTED_RUSTUP_PATH" "$AFH_EXPECTED_RUSTUP"'
        printf '%s\n' '    check_afh_preflight_tool rustc "$AFH_EXPECTED_RUSTC_PATH" "$AFH_EXPECTED_RUSTC"'
        printf '%s\n' '    check_afh_preflight_tool cargo "$AFH_EXPECTED_CARGO_PATH" "$AFH_EXPECTED_CARGO"'
        printf '%s\n' 'elif [ "$AFH_CHECK_RUST_SHADOW" = 1 ]; then'
        printf '%s\n' '    check_afh_no_shell_shadow rustup "$AFH_CURRENT_RUSTUP_PATH"; check_afh_no_shell_shadow rustc "$AFH_CURRENT_RUSTC_PATH"; check_afh_no_shell_shadow cargo "$AFH_CURRENT_CARGO_PATH"'
        printf '%s\n' 'fi'
        printf '%s\n' 'if [ -n "${AFH_EXPECTED_NODE_PATH-}" ]; then'
        printf '%s\n' '    check_afh_preflight_tool node "$AFH_EXPECTED_NODE_PATH" "$AFH_EXPECTED_NODE"'
        printf '%s\n' '    check_afh_preflight_tool npm "$AFH_EXPECTED_NPM_PATH" "$AFH_EXPECTED_NPM"'
        printf '%s\n' 'elif [ "$AFH_CHECK_NODE_SHADOW" = 1 ]; then'
        printf '%s\n' '    check_afh_no_shell_shadow node "$AFH_CURRENT_NODE_PATH"; check_afh_no_shell_shadow npm "$AFH_CURRENT_NPM_PATH"'
        printf '%s\n' 'fi'
        printf '%s\n' 'if [ -n "${AFH_EXPECTED_PNPM_PATH-}" ]; then check_afh_preflight_tool pnpm "$AFH_EXPECTED_PNPM_PATH" "$AFH_EXPECTED_PNPM"; elif [ "$AFH_CHECK_PNPM_SHADOW" = 1 ]; then check_afh_no_shell_shadow pnpm "$AFH_CURRENT_PNPM_PATH"; fi'
        printf '%s\n' 'printf "AFH_PREFLIGHT_PERSISTED=passed\n"'
    } > "$preflight_verifier" || fail 24 "无法写入写入前新 shell 复探脚本"
    chmod 700 "$preflight_verifier" || fail 24 "无法保护写入前新 shell 复探脚本"
    {
        printf '%s\n' '#!/bin/sh'
        printf '%s\n' 'set -eu'
        printf '%s\n' 'unset CARGO_HOME RUSTUP_HOME'
        printf '%s\n' 'exec "$AFH_LOGIN_SHELL" -l -c "$AFH_LOGIN_COMMAND"'
    } > "$preflight_launcher" || fail 24 "无法写入写入前新 shell 复探启动器"
    chmod 700 "$preflight_launcher" || fail 24 "无法保护写入前新 shell 复探启动器"

    preflight_login_command='"$AFH_PREFLIGHT_VERIFY"'
    case "$LOGIN_SHELL_NAME" in
        fish) ;;
        *) preflight_login_command='. "$AFH_PREFLIGHT_VERIFY"' ;;
    esac
    preflight_failure=0
    preflight_environment=$(
        AFH_LOGIN_SHELL=$LOGIN_SHELL \
        AFH_LOGIN_COMMAND=$preflight_login_command \
        AFH_PREFLIGHT_VERIFY=$preflight_verifier \
        AFH_EXPECTED_GIT_PATH=$preflight_git_path \
        AFH_EXPECTED_RUSTUP_PATH=$preflight_rustup_path \
        AFH_EXPECTED_RUSTC_PATH=$preflight_rustc_path \
        AFH_EXPECTED_CARGO_PATH=$preflight_cargo_path \
        AFH_EXPECTED_NODE_PATH=$preflight_node_path \
        AFH_EXPECTED_NPM_PATH=$preflight_npm_path \
        AFH_EXPECTED_PNPM_PATH=$preflight_pnpm_path \
        AFH_CURRENT_GIT_PATH=$git_path \
        AFH_CURRENT_RUSTUP_PATH=$rustup_path \
        AFH_CURRENT_RUSTC_PATH=$rustc_path \
        AFH_CURRENT_CARGO_PATH=$cargo_path \
        AFH_CURRENT_NODE_PATH=${node_path-} \
        AFH_CURRENT_NPM_PATH=${npm_path-} \
        AFH_CURRENT_PNPM_PATH=${pnpm_path-} \
        AFH_PROJECTED_PATH=$preflight_projected_path \
        AFH_EXPECTED_GIT=$GIT_VERSION \
        AFH_EXPECTED_RUSTUP=$RUSTUP_VERSION \
        AFH_EXPECTED_RUSTC=$RUST_VERSION \
        AFH_EXPECTED_CARGO=$CARGO_VERSION \
        AFH_EXPECTED_RUST_HOST=$RUST_HOST \
        AFH_EXPECTED_NODE=$NODE_VERSION \
        AFH_EXPECTED_NPM=$NPM_VERSION \
        AFH_EXPECTED_PNPM=$PNPM_VERSION \
        AFH_CHECK_GIT_SHADOW=$preflight_check_git_shadow \
        AFH_CHECK_RUST_SHADOW=$preflight_check_rust_shadow \
        AFH_CHECK_NODE_SHADOW=$preflight_check_node_shadow \
        AFH_CHECK_PNPM_SHADOW=$preflight_check_pnpm_shadow \
        PATH=$FRESH_BASE_PATH \
        "$preflight_launcher" 2>/dev/null
    ) || preflight_failure=$?
    preflight_success_count=$(printf '%s\n' "$preflight_environment" | awk -F= '$1 == "AFH_PREFLIGHT_PERSISTED" && $2 == "passed" { count++ } END { print count + 0 }')
    if [ "$preflight_failure" -ne 0 ] || [ "$preflight_success_count" -ne 1 ]; then
        preflight_failed_tool=$(printf '%s\n' "$preflight_environment" | awk -F= '$1 == "ERROR_TOOL" { print $2; exit }')
        fail 24 "写入前新 shell 无法从持久 PATH 精确解析并执行既有工具 ${preflight_failed_tool:-unknown}（子进程退出码 ${preflight_failure}）"
    fi
    rm -f "$preflight_verifier" "$preflight_launcher" || fail 24 "无法清理写入前新 shell 复探脚本"
    FRESH_VERIFY_FILE=
    rmdir "$FRESH_VERIFY_DIR" || fail 24 "无法清理写入前新 shell 复探临时目录"
    FRESH_VERIFY_DIR=
}

# 非默认 Rust 用户根必须能由全新 login shell 自己恢复；当前进程的一次性环境变量不能决定持久安装位置。
validate_durable_rust_homes() {
    default_cargo_home=$USER_HOME/.cargo
    default_rustup_home=$USER_HOME/.rustup
    if [ "$MANAGED_CARGO_HOME" = "$default_cargo_home" ] && [ "$MANAGED_RUSTUP_HOME" = "$default_rustup_home" ]; then
        return
    fi
    [ "$TEST_MODE" = 1 ] && [ "${AFH_SKIP_PERSIST_PATH:-0}" = 1 ] && return

    RUST_HOME_VERIFY_DIR=$(mktemp -d) || fail 24 "无法创建 Rust 用户根持久化复探临时目录"
    [ -d "$RUST_HOME_VERIFY_DIR" ] && [ ! -L "$RUST_HOME_VERIFY_DIR" ] || fail 24 "Rust 用户根持久化复探临时目录不安全"
    rust_home_verifier=$(mktemp "$RUST_HOME_VERIFY_DIR/check.XXXXXX") || fail 24 "无法创建 Rust 用户根持久化复探脚本"
    rust_home_launcher=$(mktemp "$RUST_HOME_VERIFY_DIR/launch.XXXXXX") || fail 24 "无法创建 Rust 用户根持久化复探启动器"
    RUST_HOME_VERIFY_FILE=$rust_home_verifier
    {
        printf '%s\n' '#!/bin/sh'
        printf '%s\n' 'set -eu'
        printf '%s\n' 'printf "AFH_CARGO_HOME=%s\\n" "${CARGO_HOME:-$HOME/.cargo}"'
        printf '%s\n' 'printf "AFH_RUSTUP_HOME=%s\\n" "${RUSTUP_HOME:-$HOME/.rustup}"'
    } > "$rust_home_verifier" || fail 24 "无法写入 Rust 用户根持久化复探脚本"
    chmod 700 "$rust_home_verifier" || fail 24 "无法保护 Rust 用户根持久化复探脚本"
    {
        printf '%s\n' '#!/bin/sh'
        printf '%s\n' 'set -eu'
        printf '%s\n' 'unset CARGO_HOME RUSTUP_HOME'
        printf '%s\n' 'exec "$AFH_LOGIN_SHELL" -l -c "$AFH_LOGIN_COMMAND"'
    } > "$rust_home_launcher" || fail 24 "无法写入 Rust 用户根持久化复探启动器"
    chmod 700 "$rust_home_launcher" || fail 24 "无法保护 Rust 用户根持久化复探启动器"

    rust_home_failure=0
    durable_rust_homes=$(
        AFH_LOGIN_SHELL=$LOGIN_SHELL \
        AFH_LOGIN_COMMAND='"$AFH_RUST_HOME_VERIFY"' \
        AFH_RUST_HOME_VERIFY=$rust_home_verifier \
        PATH=$FRESH_BASE_PATH \
        "$rust_home_launcher" 2>/dev/null
    ) || rust_home_failure=$?
    if [ "$rust_home_failure" -ne 0 ]; then
        fail 24 "全新 login shell 无法复探非默认 Rust 用户安装根（子进程退出码 ${rust_home_failure}）"
    fi
    durable_cargo_count=$(printf '%s\n' "$durable_rust_homes" | awk -F= '$1 == "AFH_CARGO_HOME" { count++ } END { print count + 0 }')
    durable_rustup_count=$(printf '%s\n' "$durable_rust_homes" | awk -F= '$1 == "AFH_RUSTUP_HOME" { count++ } END { print count + 0 }')
    durable_cargo_home=$(printf '%s\n' "$durable_rust_homes" | awk -F= '$1 == "AFH_CARGO_HOME" { sub(/^AFH_CARGO_HOME=/, ""); print; exit }')
    durable_rustup_home=$(printf '%s\n' "$durable_rust_homes" | awk -F= '$1 == "AFH_RUSTUP_HOME" { sub(/^AFH_RUSTUP_HOME=/, ""); print; exit }')
    [ "$durable_cargo_count" -eq 1 ] && [ "$durable_rustup_count" -eq 1 ] && \
        [ "$durable_cargo_home" = "$MANAGED_CARGO_HOME" ] && \
        [ "$durable_rustup_home" = "$MANAGED_RUSTUP_HOME" ] || \
        fail 24 "非默认 CARGO_HOME/RUSTUP_HOME 必须由用户级 login shell 持久恢复，不能只存在于当前进程"

    rm -f "$rust_home_verifier" "$rust_home_launcher" || fail 24 "无法清理 Rust 用户根持久化复探脚本"
    RUST_HOME_VERIFY_FILE=
    rmdir "$RUST_HOME_VERIFY_DIR" || fail 24 "无法清理 Rust 用户根持久化复探临时目录"
    RUST_HOME_VERIFY_DIR=
}

# 汇总所有将写入的安装根、profile、受管配置与稳定链接；任何冲突都在下载/安装前失败。
preflight_user_installation() {
    validate_user_login_shell
    # 任一工具变化都会写入同时包含 Cargo bin 与 ~/.local/bin 的规范 PATH 块；
    # 两个前缀必须作为一个集合在下载前验证，不能只检查本轮待安装工具所属的一侧。
    if ! { [ "$TEST_MODE" = 1 ] && [ "${AFH_SKIP_PERSIST_PATH:-0}" = 1 ]; }; then
        validate_managed_directory_path "$MANAGED_CARGO_HOME" "Rust Cargo 当前用户 PATH 根"
        validate_user_tool_directory_path
    fi
    if [ "$RUST_STATUS" != passed ]; then
        validate_managed_directory_path "$MANAGED_CARGO_HOME" "Rust Cargo 当前用户安装根"
        validate_managed_directory_path "$MANAGED_RUSTUP_HOME" "Rust rustup 当前用户安装根"
    fi
    if [ "$NODE_STATUS" != passed ] && [ "$NODE_STATUS" != not-required ]; then
        preflight_existing_node_version_roots
        for afh_tool in node npm npx corepack; do
            validate_user_tool_destination "$afh_tool" "$NODE_HOME"
        done
    fi
    if [ "$PNPM_STATUS" != passed ] && [ "$PNPM_STATUS" != not-required ]; then
        validate_user_tool_directory_path
        for afh_tool in pnpm pnpx; do
            validate_user_tool_destination "$afh_tool" "$PNPM_HOME"
        done
        validate_managed_directory_path "$PNPM_HOME" "npm 当前用户全局前缀"
        validate_managed_directory_path "$PNPM_HOME/lib" "npm 当前用户全局 lib 目录"
        validate_managed_directory_path "$PNPM_HOME/lib/node_modules" "npm 当前用户全局包目录"
        validate_managed_directory_path "$PNPM_HOME/lib/node_modules/pnpm" "pnpm 当前用户全局包目标"
    fi
    [ "$TEST_MODE" = 1 ] && [ "${AFH_SKIP_PERSIST_PATH:-0}" = 1 ] && return
    command -v cp >/dev/null 2>&1 || fail 24 "原子维护 shell profile 需要 cp"
    command -v cmp >/dev/null 2>&1 || fail 24 "原子维护 shell profile 需要 cmp"
    validate_profile_file_shape "$USER_HOME/.profile"
    case "$LOGIN_SHELL_NAME" in
        bash|bash.exe)
            validate_profile_file_shape "$USER_HOME/.bashrc"
            validate_profile_file_shape "$USER_HOME/.bash_profile"
            validate_profile_file_shape "$USER_HOME/.bash_login"
            ;;
        zsh)
            validate_profile_file_shape "$USER_HOME/.zprofile"
            validate_profile_file_shape "$USER_HOME/.zshrc"
            ;;
        fish)
            validate_managed_directory_path "$USER_HOME/.config/fish" "fish 用户配置目录"
            validate_managed_directory_path "$USER_HOME/.config/fish/conf.d" "fish 用户配置目录"
            validate_managed_file_shape "$USER_HOME/.config/fish/conf.d/agent-first-harness.fish" "fish 用户 PATH 配置"
            ;;
    esac
    validate_durable_rust_homes
}

# fish 的层级目录先于任何下载或安装逐级建立并复核，后续配置提交不会因新用户缺少 ~/.config 而半途失败。
prepare_user_configuration_directories() {
    if [ "$LOGIN_SHELL_NAME" = fish ]; then
        prepare_managed_directory_path "$USER_HOME/.config/fish/conf.d" "fish 用户配置目录"
    fi
}

# fish 使用原生命令把标准当前用户工具目录加入 PATH，不创建私有环境变量。
write_fish_path_config() {
    fish_root=${HOME:?必须设置 HOME}/.config/fish
    fish_config_dir=$fish_root/conf.d
    prepare_managed_directory_path "$fish_config_dir" "fish 用户配置目录"
    fish_file=$fish_config_dir/agent-first-harness.fish
    [ ! -e "$fish_file" ] || { [ -f "$fish_file" ] && [ ! -L "$fish_file" ]; } || fail 24 "fish 用户 PATH 配置不是普通文件：$fish_file"
    fish_marker='# managed by agent-first-harness development environment gate'
    if [ -f "$fish_file" ] && [ "$(sed -n '1p' "$fish_file")" != "$fish_marker" ]; then
        fail 24 "fish 用户 PATH 配置已存在且不受门禁管理：$fish_file"
    fi
    umask 077
    fish_temp=$(mktemp "$fish_config_dir/.agent-first-harness.fish.tmp.XXXXXX") || fail 24 "无法创建 fish 用户 PATH 临时文件"
    FISH_TEMP_FILE=$fish_temp
    {
        printf '%s\n' "$fish_marker"
        printf '%s\n' 'set -l user_cargo_home "$HOME/.cargo"'
        printf '%s\n' 'if set -q CARGO_HOME'
        printf '%s\n' '    set user_cargo_home "$CARGO_HOME"'
        printf '%s\n' 'end'
        printf '%s\n' 'set -l user_cargo_safe 1'
        printf '%s\n' 'if not string match -q -- "$HOME/*" "$user_cargo_home"'
        printf '%s\n' '    set user_cargo_safe 0'
        printf '%s\n' 'end'
        printf '%s\n' 'for user_cargo_unsafe_pattern in "*:*" "*/../*" "*/.." "*/./*" "*/."'
        printf '%s\n' '    if string match -q -- "$user_cargo_unsafe_pattern" "$user_cargo_home"'
        printf '%s\n' '        set user_cargo_safe 0'
        printf '%s\n' '    end'
        printf '%s\n' 'end'
        printf '%s\n' 'if test "$user_cargo_safe" -eq 1; and test "$user_cargo_home/bin" != "$HOME/.local/bin"'
        printf '%s\n' '    fish_add_path --path "$user_cargo_home/bin"'
        printf '%s\n' 'end'
        printf '%s\n' 'fish_add_path --path "$HOME/.local/bin"'
        printf '%s\n' 'set -e user_cargo_home user_cargo_safe user_cargo_unsafe_pattern'
    } > "$fish_temp" || fail 24 "无法写入 fish 用户 PATH 配置"
    [ -f "$fish_temp" ] && [ ! -L "$fish_temp" ] || fail 24 "fish 用户 PATH 临时文件不安全"
    mv -f "$fish_temp" "$fish_file" || fail 24 "无法提交 fish 用户 PATH 配置"
    FISH_TEMP_FILE=
}

# 工具变化后直接持久化标准当前用户 PATH，并由全新 login shell 锁定路径与版本复探。
persist_user_tool_path() {
    [ "$TEST_MODE" = 1 ] && [ "${AFH_SKIP_PERSIST_PATH:-0}" = 1 ] && return
    validate_user_login_shell
    managed_path_changed=0
    if [ "$RUST_CHANGED" != existing ] || [ "$NODE_CHANGED" != existing ] || [ "$PNPM_CHANGED" != existing ]; then
        managed_path_changed=1
        ensure_profile_path "$HOME/.profile"
        case "$LOGIN_SHELL_NAME" in
            bash|bash.exe)
                ensure_profile_path "$HOME/.bashrc"
                if [ -f "$HOME/.bash_profile" ]; then
                    ensure_profile_path "$HOME/.bash_profile"
                elif [ -f "$HOME/.bash_login" ]; then
                    ensure_profile_path "$HOME/.bash_login"
                fi
                ;;
            zsh)
                ensure_profile_path "$HOME/.zprofile"
                ensure_profile_path "$HOME/.zshrc"
                ;;
            fish)
                write_fish_path_config
                ;;
        esac
        [ -z "${USER_BIN_DIR:-}" ] || prepend_probe_path "$USER_BIN_DIR"
    fi
    FRESH_VERIFY_DIR=$(mktemp -d) || fail 24 "无法创建新 shell 工具复探临时目录"
    [ -d "$FRESH_VERIFY_DIR" ] && [ ! -L "$FRESH_VERIFY_DIR" ] || fail 24 "新 shell 工具复探临时目录不安全"
    fresh_verifier=$(mktemp "$FRESH_VERIFY_DIR/check.XXXXXX") || fail 24 "无法创建新 shell 工具复探脚本"
    fresh_launcher=$(mktemp "$FRESH_VERIFY_DIR/launch.XXXXXX") || fail 24 "无法创建新 shell 工具复探启动器"
    FRESH_VERIFY_FILE=$fresh_verifier
    {
        printf '%s\n' '#!/bin/sh'
        printf '%s\n' 'set -eu'
        printf '%s\n' 'check_afh_tool() {'
        printf '%s\n' '    afh_tool_name=$1'
        printf '%s\n' '    afh_expected_path=$2'
        printf '%s\n' '    afh_expected_version=$3'
        printf '%s\n' '    afh_resolved=$(command -v "$afh_tool_name") || { printf "ERROR_TOOL=%s\n" "$afh_tool_name"; exit 41; }'
        printf '%s\n' '    [ "$afh_resolved" = "$afh_expected_path" ] || { printf "ERROR_TOOL=%s\n" "$afh_tool_name"; exit 42; }'
        printf '%s\n' '    afh_actual_version=$("$afh_resolved" --version) || { printf "ERROR_TOOL=%s\n" "$afh_tool_name"; exit 43; }'
        printf '%s\n' '    [ "$afh_actual_version" = "$afh_expected_version" ] || { printf "ERROR_TOOL=%s\n" "$afh_tool_name"; exit 44; }'
        printf '%s\n' '    if [ "$afh_tool_name" = rustc ]; then'
        printf '%s\n' '        afh_verbose=$("$afh_resolved" -vV) || { printf "ERROR_TOOL=%s\n" "$afh_tool_name"; exit 45; }'
        printf '%s\n' '        afh_release_tail=${afh_expected_version#rustc }'
        printf '%s\n' '        afh_expected_release=${afh_release_tail%% *}'
        printf '%s\n' "        afh_release_count=\$(printf '%s\\n' \"\$afh_verbose\" | awk -F': ' '\$1 == \"release\" { count++ } END { print count + 0 }')"
        printf '%s\n' "        afh_host_count=\$(printf '%s\\n' \"\$afh_verbose\" | awk -F': ' '\$1 == \"host\" { count++ } END { print count + 0 }')"
        printf '%s\n' "        afh_verbose_release=\$(printf '%s\\n' \"\$afh_verbose\" | awk -F': ' '\$1 == \"release\" { print \$2 }')"
        printf '%s\n' "        afh_verbose_host=\$(printf '%s\\n' \"\$afh_verbose\" | awk -F': ' '\$1 == \"host\" { print \$2 }')"
        printf '%s\n' '        [ "$afh_release_count" -eq 1 ] && [ "$afh_host_count" -eq 1 ] && [ "$afh_verbose_release" = "$afh_expected_release" ] && [ "$afh_verbose_host" = "$AFH_EXPECTED_RUST_HOST" ] || { printf "ERROR_TOOL=%s\n" "$afh_tool_name"; exit 45; }'
        printf '%s\n' '    fi'
        printf '%s\n' '}'
        printf '%s\n' 'check_afh_tool git "$AFH_EXPECTED_GIT_PATH" "$AFH_EXPECTED_GIT"'
        printf '%s\n' 'check_afh_tool rustup "$AFH_EXPECTED_RUSTUP_PATH" "$AFH_EXPECTED_RUSTUP"'
        printf '%s\n' 'check_afh_tool rustc "$AFH_EXPECTED_RUSTC_PATH" "$AFH_EXPECTED_RUSTC"'
        printf '%s\n' 'check_afh_tool cargo "$AFH_EXPECTED_CARGO_PATH" "$AFH_EXPECTED_CARGO"'
        printf '%s\n' 'if [ -n "${AFH_EXPECTED_NODE_PATH-}" ]; then check_afh_tool node "$AFH_EXPECTED_NODE_PATH" "$AFH_EXPECTED_NODE"; fi'
        printf '%s\n' 'if [ -n "${AFH_EXPECTED_NPM_PATH-}" ]; then check_afh_tool npm "$AFH_EXPECTED_NPM_PATH" "$AFH_EXPECTED_NPM"; fi'
        printf '%s\n' 'if [ -n "${AFH_EXPECTED_PNPM_PATH-}" ]; then check_afh_tool pnpm "$AFH_EXPECTED_PNPM_PATH" "$AFH_EXPECTED_PNPM"; fi'
        printf '%s\n' 'printf "PATH=%s\\n" "$PATH"'
    } > "$fresh_verifier" || fail 24 "无法创建新 shell 工具复探脚本"
    chmod 700 "$fresh_verifier" || fail 24 "无法保护新 shell 工具复探脚本"
    {
        printf '%s\n' '#!/bin/sh'
        printf '%s\n' 'set -eu'
        printf '%s\n' 'unset CARGO_HOME RUSTUP_HOME'
        printf '%s\n' 'exec "$AFH_LOGIN_SHELL" -l -c "$AFH_LOGIN_COMMAND"'
    } > "$fresh_launcher" || fail 24 "无法创建新 shell 工具复探启动器"
    chmod 700 "$fresh_launcher" || fail 24 "无法保护新 shell 工具复探启动器"
    expected_git_path=$git_path
    expected_rustup_path=$rustup_path
    expected_rustc_path=$rustc_path
    expected_cargo_path=$cargo_path
    expected_node_path=
    expected_npm_path=
    expected_pnpm_path=
    expected_node_path=$node_path
    expected_npm_path=$npm_path
    [ "$NODE_CHANGED" = existing ] || expected_node_path=$USER_BIN_DIR/node
    [ "$NODE_CHANGED" = existing ] || expected_npm_path=$USER_BIN_DIR/npm
    if [ "$PNPM_REQUIRED" -eq 1 ]; then
        expected_pnpm_path=$pnpm_path
        [ "$PNPM_CHANGED" = existing ] || expected_pnpm_path=$USER_BIN_DIR/pnpm
    fi
    fresh_failure=0
    persisted_environment=$(
        AFH_LOGIN_SHELL=$LOGIN_SHELL \
        AFH_LOGIN_COMMAND='"$AFH_FRESH_VERIFY"' \
        AFH_FRESH_VERIFY=$fresh_verifier \
        AFH_EXPECTED_GIT_PATH=$expected_git_path \
        AFH_EXPECTED_RUSTUP_PATH=$expected_rustup_path \
        AFH_EXPECTED_RUSTC_PATH=$expected_rustc_path \
        AFH_EXPECTED_CARGO_PATH=$expected_cargo_path \
        AFH_EXPECTED_NODE_PATH=$expected_node_path \
        AFH_EXPECTED_NPM_PATH=$expected_npm_path \
        AFH_EXPECTED_PNPM_PATH=$expected_pnpm_path \
        AFH_EXPECTED_GIT=$GIT_VERSION \
        AFH_EXPECTED_RUSTUP=$RUSTUP_VERSION \
        AFH_EXPECTED_RUSTC=$RUST_VERSION \
        AFH_EXPECTED_CARGO=$CARGO_VERSION \
        AFH_EXPECTED_RUST_HOST=$RUST_HOST \
        AFH_EXPECTED_NODE=$NODE_VERSION \
        AFH_EXPECTED_NPM=$NPM_VERSION \
        AFH_EXPECTED_PNPM=$PNPM_VERSION \
        PATH=$FRESH_BASE_PATH \
        "$fresh_launcher" 2>/dev/null
    ) || fresh_failure=$?
    if [ "$fresh_failure" -ne 0 ]; then
        rm -f "$fresh_verifier"
        FRESH_VERIFY_FILE=
        fresh_failed_tool=$(printf '%s\n' "$persisted_environment" | awk -F= '$1 == "ERROR_TOOL" { print $2; exit }')
        fail 24 "新 shell 无法从持久 PATH 解析并执行同一受管工具 ${fresh_failed_tool:-unknown}（子进程退出码 ${fresh_failure}）"
    fi
    rm -f "$fresh_verifier" "$fresh_launcher" || fail 24 "无法清理新 shell 工具复探脚本"
    FRESH_VERIFY_FILE=
    rmdir "$FRESH_VERIFY_DIR" || fail 24 "无法清理新 shell 工具复探临时目录"
    FRESH_VERIFY_DIR=
    persisted_probe=$(printf '%s\n' "$persisted_environment" | awk -F= '$1 == "PATH" { sub(/^PATH=/, ""); print; exit }')
    if [ "$managed_path_changed" -eq 1 ]; then
        if [ "$RUST_CHANGED" != existing ]; then
            case ":$persisted_probe:" in
                *":$CARGO_BIN_DIR:"*) ;;
                *) fail 24 "Rust 当前用户 bin 写入后无法由新 shell 读取" ;;
            esac
        fi
        if [ "$NODE_CHANGED" != existing ] || [ "$PNPM_CHANGED" != existing ]; then
            case ":$persisted_probe:" in
                *":$USER_BIN_DIR:"*) ;;
                *) fail 24 "标准 ~/.local/bin 写入后无法由新 shell 读取" ;;
            esac
        fi
    fi
    [ "$(sanitize_path "$persisted_probe")" = "$persisted_probe" ] || fail 24 "新 shell 的用户级 PATH 仍包含空段或重复项"
    FRESH_SHELL_STATUS=passed
}

git_path=$(find_tool git 2>/dev/null || true)
if [ -n "$git_path" ]; then
    validate_git "$git_path"
else
    GIT_VERSION=Missing
    GIT_STATUS=missing
fi

rustup_path=$(find_tool rustup 2>/dev/null || true)
rustc_path=$(find_tool rustc 2>/dev/null || true)
cargo_path=$(find_tool cargo 2>/dev/null || true)
if [ -n "$rustup_path" ] && [ -n "$rustc_path" ] && [ -n "$cargo_path" ]; then
    validate_rust "$rustup_path" "$rustc_path" "$cargo_path"
else
    RUSTUP_VERSION=Missing
    RUST_VERSION=Missing
    CARGO_VERSION=Missing
    RUST_HOST=Missing
    RUST_STATUS=missing
fi

node_path=$(find_tool node 2>/dev/null || true)
npm_path=$(find_tool npm 2>/dev/null || true)
if [ -n "$node_path" ] && [ -n "$npm_path" ]; then
    validate_node "$node_path"
    validate_npm "$npm_path"
else
    NODE_VERSION=Missing
    NPM_VERSION=Missing
    NODE_STATUS=missing
fi
if [ "$PNPM_REQUIRED" -eq 1 ]; then
    pnpm_path=$(find_tool pnpm 2>/dev/null || true)
    if [ -n "$pnpm_path" ]; then
        validate_pnpm "$pnpm_path"
    else
        PNPM_VERSION=Missing
        PNPM_STATUS=missing
    fi
else
    PNPM_VERSION=Not-required
    PNPM_STATUS=not-required
fi

if [ "$MODE" = check ]; then
    printf 'gate.git.status=%s\n' "$GIT_STATUS"
    printf 'gate.git.requirement=%s\n' "$GIT_REQUIREMENT"
    printf 'gate.git.version=%s\n' "$GIT_VERSION"
    printf 'gate.rust.status=%s\n' "$RUST_STATUS"
    printf 'gate.rust.version=%s\n' "$RUST_VERSION"
    printf 'gate.rust.rustup_version=%s\n' "$RUSTUP_VERSION"
    printf 'gate.rust.cargo_version=%s\n' "$CARGO_VERSION"
    printf 'gate.rust.host=%s\n' "$RUST_HOST"
    printf 'gate.node.status=%s\n' "$NODE_STATUS"
    printf 'gate.node.requirement=%s\n' "$NODE_REQUIREMENT"
    printf 'gate.node.version=%s\n' "$NODE_VERSION"
    printf 'gate.pnpm.status=%s\n' "$PNPM_STATUS"
    printf 'gate.pnpm.requirement=%s\n' "$PNPM_REQUIREMENT"
    printf 'gate.pnpm.version=%s\n' "$PNPM_VERSION"
    [ "$GIT_STATUS" = passed ] &&
        [ "$RUST_STATUS" = passed ] &&
        [ "$NODE_STATUS" = passed ] &&
        { [ "$PNPM_STATUS" = passed ] || [ "$PNPM_STATUS" = not-required ]; } || exit 20
    exit 0
fi

# 受管用户级工具先完成全部只读目标预检，再从真实持久 login shell 精确复核本轮不变工具；
# 通过后才允许为 fish 新用户安全准备配置目录，瞬时 AFH_PREREQ_PATH 不得决定任何持久写入。
if [ "$RUST_STATUS" != passed ] || [ "$NODE_STATUS" != passed ] || {
    [ "$PNPM_REQUIRED" -eq 1 ] && [ "$PNPM_STATUS" != passed ];
}; then
    preflight_user_installation
    verify_persisted_passed_tools_before_write
    prepare_user_configuration_directories
elif [ "$GIT_STATUS" != passed ]; then
    validate_user_login_shell
    validate_durable_rust_homes
    verify_persisted_passed_tools_before_write
fi

if [ "$GIT_STATUS" != passed ]; then
    git_change=upgraded
    [ "$GIT_STATUS" = missing ] && git_change=installed
    install_git "$git_change"
    git_path=$(find_tool git 2>/dev/null || true)
    [ -n "$git_path" ] || fail 29 "Git 安装完成后仍无法调用 git 可执行文件"
    validate_git "$git_path"
    [ "$GIT_STATUS" = passed ] || fail 29 "Git 安装或升级后仍低于门禁 ${GIT_REQUIREMENT}：$GIT_VERSION"
fi

if [ "$RUST_STATUS" != passed ]; then
    rust_change=upgraded
    [ "$RUST_STATUS" = missing ] && rust_change=installed
    install_rust "$rust_change"
    rustup_path=$(find_tool rustup 2>/dev/null || true)
    rustc_path=$(find_tool rustc 2>/dev/null || true)
    cargo_path=$(find_tool cargo 2>/dev/null || true)
    [ -n "$rustup_path" ] && [ -n "$rustc_path" ] && [ -n "$cargo_path" ] || fail 22 "Rust 安装完成后仍无法调用 rustup、rustc 和 cargo"
    validate_rust "$rustup_path" "$rustc_path" "$cargo_path"
    [ "$RUST_STATUS" = passed ] || fail 22 "Rust 安装或升级后仍低于 MSRV $MIN_RUST_MAJOR.$MIN_RUST_MINOR.${MIN_RUST_PATCH}：$RUST_VERSION"
fi

if [ "$NODE_STATUS" != passed ] && [ "$NODE_STATUS" != not-required ]; then
    node_change=upgraded
    [ "$NODE_STATUS" = missing ] && node_change=installed
    install_node "$node_change"
    node_path=$(find_tool node 2>/dev/null || true)
    npm_path=$(find_tool npm 2>/dev/null || true)
    [ -n "$node_path" ] && [ -n "$npm_path" ] || fail 26 "Node.js 安装完成后仍无法调用 node 和 npm"
    validate_node "$node_path"
    validate_npm "$npm_path"
    [ "$NODE_STATUS" = passed ] || fail 26 "Node.js 安装或升级后仍不满足门禁 ${NODE_REQUIREMENT}：$NODE_VERSION"
    [ "$NODE_VERSION" = "$SELECTED_NODE_VERSION" ] || fail 26 "Node.js 安装或升级后的版本与已选择稳定版不一致：期望 ${SELECTED_NODE_VERSION}，实际 $NODE_VERSION"
fi

if [ "$PNPM_STATUS" != passed ] && [ "$PNPM_STATUS" != not-required ]; then
    pnpm_change=upgraded
    [ "$PNPM_STATUS" = missing ] && pnpm_change=installed
    install_pnpm "$pnpm_change"
    pnpm_path=$(find_tool pnpm 2>/dev/null || true)
    [ -n "$pnpm_path" ] || fail 28 "pnpm 安装完成后仍无法调用 pnpm 可执行文件"
    validate_pnpm "$pnpm_path"
    [ "$PNPM_STATUS" = passed ] || fail 28 "pnpm 安装或升级后仍低于门禁 ${PNPM_REQUIREMENT}：$PNPM_VERSION"
fi

if [ "$GIT_CHANGED" != existing ] || [ "$RUST_CHANGED" != existing ] || [ "$NODE_CHANGED" != existing ] || [ "$PNPM_CHANGED" != existing ]; then
    persist_user_tool_path
fi

changed=false
[ "$GIT_CHANGED" = existing ] || changed=true
[ "$RUST_CHANGED" = existing ] || changed=true
[ "$NODE_CHANGED" = existing ] || changed=true
[ "$PNPM_CHANGED" = existing ] || changed=true

printf 'gate.git.status=passed\n'
printf 'gate.git.requirement=%s\n' "$GIT_REQUIREMENT"
printf 'gate.git.version=%s\n' "$GIT_VERSION"
printf 'gate.git.change=%s\n' "$GIT_CHANGED"
printf 'gate.rust.status=passed\n'
printf 'gate.rust.version=%s\n' "$RUST_VERSION"
printf 'gate.rust.rustup_version=%s\n' "$RUSTUP_VERSION"
printf 'gate.rust.cargo_version=%s\n' "$CARGO_VERSION"
printf 'gate.rust.host=%s\n' "$RUST_HOST"
printf 'gate.rust.change=%s\n' "$RUST_CHANGED"
printf 'gate.node.status=passed\n'
printf 'gate.node.requirement=%s\n' "$NODE_REQUIREMENT"
printf 'gate.node.version=%s\n' "$NODE_VERSION"
printf 'gate.node.change=%s\n' "$NODE_CHANGED"
printf 'gate.pnpm.status=%s\n' "$([ "$PNPM_REQUIRED" -eq 1 ] && printf passed || printf not-required)"
printf 'gate.pnpm.requirement=%s\n' "$PNPM_REQUIREMENT"
printf 'gate.pnpm.version=%s\n' "$PNPM_VERSION"
printf 'gate.pnpm.change=%s\n' "$PNPM_CHANGED"
printf 'gate.changed=%s\n' "$changed"
printf 'gate.fresh_shell.status=%s\n' "$FRESH_SHELL_STATUS"
if [ -n "$GIT_BIN_DIR" ] || [ -n "$CARGO_BIN_DIR" ] || [ -n "$NODE_BIN_DIR" ] || [ -n "$PNPM_BIN_DIR" ] || [ -n "${USER_BIN_DIR:-}" ]; then
    path_prepend=
    for candidate in "${USER_BIN_DIR:-}" "$PNPM_BIN_DIR" "$NODE_BIN_DIR" "$CARGO_BIN_DIR" "$GIT_BIN_DIR"; do
        [ -n "$candidate" ] || continue
        case ":$path_prepend:" in
            *":$candidate:"*) ;;
            *) [ -n "$path_prepend" ] && path_prepend=$path_prepend:$candidate || path_prepend=$candidate ;;
        esac
    done
    printf 'gate.path.prepend=%s\n' "$path_prepend"
fi
