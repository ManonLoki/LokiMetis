#!/bin/sh
set -eu

# MSRV 的唯一事实来源见 docs/RUST_GUI_TEMPLATE.md；修改此值时必须同步更新 development-environment-gates.ps1。
MIN_RUST_MAJOR=1
MIN_RUST_MINOR=95
NODE_REQUIREMENT='^24.15.0 || >=26.0.0'
PNPM_REQUIREMENT='>=11.24.0'
PNPM_INSTALL_REQUIREMENT='pnpm@>=11.24.0'
GIT_REQUIREMENT='>=2.0.0'
PNPM_REGISTRY='https://registry.npmjs.org/'
TEST_MODE=${AFH_TEST_MODE:-0}
MODE=install

# 输出稳定的命令入口说明，避免调用方误把只读模式当成首次开发安装模式。
usage() {
    printf '%s\n' "用法：development-environment-gates.sh [--install-missing|--check-only]"
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --install-missing) MODE=install ;;
        --check-only) MODE=check ;;
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
ENV_TEMP_FILE=
FISH_TEMP_FILE=
FRESH_VERIFY_FILE=
FRESH_VERIFY_DIR=
LINK_TEMP_DIR=
PROFILE_TEMP_FILE=
PROFILE_SNAPSHOT_FILE=
SELECTED_NODE_VERSION=
FRESH_SHELL_STATUS=not-required

# 只清理本进程通过 mktemp 创建的下载目录，不触碰安装目标或用户已有文件。
cleanup() {
    for gate_temp_file in "$ENV_TEMP_FILE" "$FISH_TEMP_FILE" "$FRESH_VERIFY_FILE" "$PROFILE_TEMP_FILE" "$PROFILE_SNAPSHOT_FILE"; do
        [ -n "$gate_temp_file" ] || continue
        if [ -f "$gate_temp_file" ] || [ -L "$gate_temp_file" ]; then rm -f "$gate_temp_file" 2>/dev/null || true; fi
    done
    if [ -n "$LINK_TEMP_DIR" ] && [ -d "$LINK_TEMP_DIR" ] && [ ! -L "$LINK_TEMP_DIR" ]; then
        find "$LINK_TEMP_DIR" -depth -delete 2>/dev/null || true
    fi
    if [ -n "$FRESH_VERIFY_DIR" ] && [ -d "$FRESH_VERIFY_DIR" ] && [ ! -L "$FRESH_VERIFY_DIR" ]; then
        find "$FRESH_VERIFY_DIR" -depth -delete 2>/dev/null || true
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
    AFH_TEST_RUST_LIBC; do
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

# 生产调用始终使用当前用户的固定受管根；标准 CARGO_HOME/RUSTUP_HOME 只影响外部命令，不能重定向门禁写入。
USER_HOME=${HOME:?必须设置 HOME}
case "$USER_HOME" in
    /*) ;;
    *) fail 24 "HOME 必须是绝对路径" ;;
esac
MANAGED_CARGO_HOME=$USER_HOME/.cargo
MANAGED_RUSTUP_HOME=$USER_HOME/.rustup
NODE_HOME=$USER_HOME/.local/share/agent-first-harness/node
PNPM_HOME=$USER_HOME/.local/share/agent-first-pnpm
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
BASE_SESSION_PATH=$PATH
PROBE_PATH=$(sanitize_path "$PROBE_PATH")

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
    rustup_text=$(CARGO_HOME=$MANAGED_CARGO_HOME RUSTUP_HOME=$MANAGED_RUSTUP_HOME "$rustup_path" --version 2>/dev/null) || fail 21 "rustup 探测失败"
    rust_text=$(CARGO_HOME=$MANAGED_CARGO_HOME RUSTUP_HOME=$MANAGED_RUSTUP_HOME "$rustc_path" --version 2>/dev/null) || fail 21 "rustc 探测失败"
    cargo_text=$(CARGO_HOME=$MANAGED_CARGO_HOME RUSTUP_HOME=$MANAGED_RUSTUP_HOME "$cargo_path" --version 2>/dev/null) || fail 21 "cargo 探测失败"
    rust_verbose=$(CARGO_HOME=$MANAGED_CARGO_HOME RUSTUP_HOME=$MANAGED_RUSTUP_HOME "$rustc_path" -vV 2>/dev/null) || fail 21 "rustc -vV 探测失败"
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
        [ "$rust_major" -eq "$MIN_RUST_MAJOR" ] && [ "$rust_minor" -lt "$MIN_RUST_MINOR" ];
    } || [ "$cargo_major" -lt "$MIN_RUST_MAJOR" ] || {
        [ "$cargo_major" -eq "$MIN_RUST_MAJOR" ] && [ "$cargo_minor" -lt "$MIN_RUST_MINOR" ];
    }; then
        RUST_STATUS=upgrade-required
        return
    fi
    RUST_STATUS=passed
}

# 验证 Node.js 落在 Vite 基线的非连续范围内；低版本与 25.x 返回升级需求。
validate_node() {
    node_path=$1
    node_text=$("$node_path" --version 2>/dev/null) || fail 23 "Node.js 探测失败"
    node_release=$(printf '%s\n' "$node_text" | sed 's/^v//')
    case "$node_release" in
        *-*|*+*) fail 23 "现有 Node.js 不是可识别的稳定发布版：$node_text" ;;
        *.*.*) ;;
        *) fail 23 "无法识别 Node.js 发布版本：$node_text" ;;
    esac
    node_major=$(printf '%s\n' "$node_release" | awk -F. '{print $1}')
    node_minor=$(printf '%s\n' "$node_release" | awk -F. '{print $2}')
    node_patch=$(printf '%s\n' "$node_release" | awk -F. '{print $3}')
    case "$node_major:$node_minor:$node_patch" in
        *[!0-9:]*|::*|*::|*::*:*) fail 23 "无法识别 Node.js 发布版本：$node_text" ;;
    esac
    node_compatible=0
    if [ "$node_major" -eq 24 ] && {
        [ "$node_minor" -gt 15 ] || { [ "$node_minor" -eq 15 ] && [ "$node_patch" -ge 0 ]; };
    }; then
        node_compatible=1
    elif [ "$node_major" -ge 26 ]; then
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
    pnpm_major=$(printf '%s\n' "$pnpm_text" | awk -F. '{print $1}')
    pnpm_minor=$(printf '%s\n' "$pnpm_text" | awk -F. '{print $2}')
    pnpm_patch=$(printf '%s\n' "$pnpm_text" | awk -F. '{print $3}')
    case "$pnpm_major:$pnpm_minor:$pnpm_patch" in
        *[!0-9:]*|::*|*::|*::*:*) fail 28 "无法识别 pnpm 发布版本：$pnpm_text" ;;
    esac
    PNPM_VERSION=$pnpm_text
    if [ "$pnpm_major" -lt 11 ] || {
        [ "$pnpm_major" -eq 11 ] && [ "$pnpm_minor" -lt 24 ];
    }; then
        PNPM_STATUS=upgrade-required
        return
    fi
    PNPM_STATUS=passed
}

# Git 是所有初始化路径的基础工具；低于 2.0.0 时返回升级需求。
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
    if [ "$git_major" -ge 2 ]; then
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
            rust_libc=${AFH_TEST_RUST_LIBC:-}
            if [ -z "$rust_libc" ]; then
                if command -v getconf >/dev/null 2>&1 && getconf GNU_LIBC_VERSION >/dev/null 2>&1; then
                    rust_libc=gnu
                else
                    ldd_text=$(ldd --version 2>&1 || true)
                    case "$ldd_text" in
                        *musl*|*MUSL*) rust_libc=musl ;;
                        *glibc*|*GLIBC*|*GNU*) rust_libc=gnu ;;
                    esac
                fi
            fi
            if [ -z "$rust_libc" ]; then
                for musl_loader in /lib/ld-musl-*.so.1 /usr/lib/ld-musl-*.so.1; do
                    if [ -e "$musl_loader" ]; then rust_libc=musl; break; fi
                done
            fi
            [ -n "$rust_libc" ] || fail 22 "无法确定 rustup 所需的 Linux libc"
            case "$rust_host_arch" in
                x86_64|amd64) rustup_target=x86_64-unknown-linux-$rust_libc ;;
                arm64|aarch64) rustup_target=aarch64-unknown-linux-$rust_libc ;;
                *) fail 22 "rustup 不支持此 Linux 架构：$rust_host_arch" ;;
            esac
            ;;
        *) fail 22 "rustup 不支持此 Unix 操作系统：$rust_host_os" ;;
    esac
}

# 下载宿主匹配的官方 rustup-init，核对发布摘要后安装或升级 stable 并加入本次复探路径。
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
    printf '正在从 %s 安装或升级 Rust stable 到 %s（PATH 将添加 %s/bin）。\n' "$release_base" "$MANAGED_RUSTUP_HOME" "$MANAGED_CARGO_HOME" >&2
    download "$installer_url" "$installer_path" || fail 22 "Rust 安装器下载失败"
    download "$checksum_url" "$checksum_path" || fail 22 "Rust 安装器校验和下载失败"
    expected_sum=$(awk '{ print $1; exit }' "$checksum_path")
    [ -n "$expected_sum" ] || fail 22 "Rust 安装器校验和为空"
    actual_sum=$(sha256_file "$installer_path")
    [ "$actual_sum" = "$expected_sum" ] || fail 22 "rustup-init SHA-256 校验失败"
    chmod +x "$installer_path" || fail 22 "无法把 rustup-init 设为可执行文件"
    prepare_managed_directory_path "$MANAGED_CARGO_HOME" "Rust Cargo 受管安装根"
    prepare_managed_directory_path "$MANAGED_RUSTUP_HOME" "Rust rustup 受管安装根"
    CARGO_HOME=$MANAGED_CARGO_HOME RUSTUP_HOME=$MANAGED_RUSTUP_HOME "$installer_path" -y --profile minimal --default-toolchain stable --no-modify-path || fail 22 "Rust 安装失败"
    CARGO_BIN_DIR=$MANAGED_CARGO_HOME/bin
    prepend_probe_path "$CARGO_BIN_DIR"
    for rust_tool in rustup rustc cargo; do
        link_user_tool "$CARGO_BIN_DIR/$rust_tool" "$rust_tool" "$MANAGED_CARGO_HOME"
    done
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
        Linux) node_platform=linux ;;
        *) fail 23 "Node.js 不支持此 Unix 操作系统：$host_os" ;;
    esac
    case "$host_arch" in
        x86_64|amd64) node_arch=x64 ;;
        arm64|aarch64) node_arch=arm64 ;;
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

# 从官方倒序索引选择当前最新满足门禁的稳定版，用于安装或升级。
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
        NR > 1 && $1 ~ /^v[0-9]+\.[0-9]+\.[0-9]+$/ {
            raw = substr($1, 2)
            split(raw, parts, ".")
            if ((parts[1] == 24 && parts[2] >= 15) || parts[1] >= 26) {
                print $1
                exit
            }
        }
    ' "$index_path")
    [ -n "$node_version" ] || fail 26 "Node.js 发布版本索引中没有满足 $NODE_REQUIREMENT 的稳定版"
    archive_name=node-$node_version-$node_platform-$node_arch.tar.gz
    archive_path=$TEMP_DIR/$archive_name
    sums_path=$TEMP_DIR/SHASUMS256.txt
    release_base=$node_dist_base/$node_version
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
    install_dir=$NODE_HOME/$node_version
    extracted_dir=$TEMP_DIR/node-$node_version-$node_platform-$node_arch
    if [ -e "$install_dir" ]; then
        validate_managed_directory_path "$install_dir" "Node.js 版本目录"
        validate_managed_file_shape "$install_dir/.agent-first-harness-managed" "Node.js 版本所有权标记"
        [ -f "$install_dir/.agent-first-harness-managed" ] || fail 26 "Node.js 目标已存在但缺少受管所有权标记：$install_dir"
        [ "$(cat "$install_dir/.agent-first-harness-managed")" = "$node_marker" ] || fail 26 "Node.js 目标受管标记与已校验官方归档不一致：$install_dir"
        [ -x "$install_dir/bin/node" ] && [ -x "$install_dir/bin/npm" ] || fail 26 "Node.js 目标已存在但不可用：$install_dir"
        installed_node_version=$("$install_dir/bin/node" --version 2>/dev/null) || fail 26 "Node.js 目标版本探测失败：$install_dir"
        [ "$installed_node_version" = "$node_version" ] || fail 26 "Node.js 目标版本与已选择稳定版不一致：期望 ${node_version}，实际 $installed_node_version"
    else
        printf '正在安装或升级到最新满足门禁的稳定 Node.js %s，来源为 %s，目标为用户级目录。\n' "$node_version" "$release_base" >&2
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
    prepare_managed_directory_path "$PNPM_HOME" "pnpm 受管安装根"
    printf '正在从官方 npm 软件包仓库安装或升级 pnpm %s 到用户级目录。\n' "$PNPM_INSTALL_REQUIREMENT" >&2
    PATH=$PROBE_PATH${PATH:+:$PATH} "$npm_path" install --global --prefix "$PNPM_HOME" "$PNPM_INSTALL_REQUIREMENT" --registry "$PNPM_REGISTRY" --ignore-scripts || fail 28 "pnpm 安装失败"
    PNPM_BIN_DIR=$PNPM_HOME/bin
    prepend_probe_path "$PNPM_BIN_DIR"
    link_user_tool "$PNPM_BIN_DIR/pnpm" pnpm "$PNPM_HOME"
    if [ -x "$PNPM_BIN_DIR/pnpx" ] && [ ! -d "$PNPM_BIN_DIR/pnpx" ]; then
        link_user_tool "$PNPM_BIN_DIR/pnpx" pnpx "$PNPM_HOME"
    fi
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

# 配置文件写入前必须已经是普通文件；受管文件还必须带唯一首行 marker。
validate_profile_file_shape() {
    afh_profile_path=$1
    [ ! -L "$afh_profile_path" ] || fail 24 "shell profile 不能是符号链接：$afh_profile_path"
    if [ -e "$afh_profile_path" ] && [ ! -f "$afh_profile_path" ]; then
        fail 24 "shell profile 不是普通文件：$afh_profile_path"
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

# 验证受管用户目录的每个固定路径组件都是普通目录，禁止沿预置符号链接写出预期范围。
validate_user_tool_directory_path() {
    afh_local_dir=${HOME:?必须设置 HOME}/.local
    afh_share_dir=$afh_local_dir/share
    afh_managed_dir=$afh_share_dir/agent-first-harness
    afh_user_bin=$afh_managed_dir/bin
    for afh_directory in "$afh_local_dir" "$afh_share_dir" "$afh_managed_dir" "$afh_user_bin"; do
        [ ! -L "$afh_directory" ] || fail 24 "用户级工具目录组件不能是符号链接：$afh_directory"
        if [ -e "$afh_directory" ] && [ ! -d "$afh_directory" ]; then
            fail 24 "用户级工具目录组件不是普通目录：$afh_directory"
        fi
    done
}

# 逐级创建并复核稳定用户 bin；不用 mkdir -p 跨越未经验证的中间组件。
prepare_user_tool_directory() {
    validate_user_tool_directory_path
    for afh_directory in "$afh_local_dir" "$afh_share_dir" "$afh_managed_dir" "$afh_user_bin"; do
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
    managed_root_physical=$(CDPATH= cd -P "$managed_root" 2>/dev/null && pwd -P) || fail 24 "无法确认受管安装根：$managed_root"
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

# 在门禁拥有的稳定用户 bin 中原子替换单个工具链接；既有链接也必须指向同一受管安装根。
link_user_tool() {
    source_path=$1
    tool_name=$2
    managed_root=$3
    [ -x "$source_path" ] && [ ! -d "$source_path" ] || fail 24 "用户级工具源不可执行：$source_path"
    validate_user_tool_destination "$tool_name" "$managed_root"
    prepare_user_tool_directory
    user_bin=$USER_BIN_DIR
    managed_root_physical=$(CDPATH= cd -P "$managed_root" 2>/dev/null && pwd -P) || fail 24 "无法确认受管安装根：$managed_root"
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

# 以同目录临时文件更新 shell profile；提交前比较原始快照，避免覆盖并发写入。
ensure_profile_source() {
    profile_path=$1
    source_line='[ -r "$HOME/.config/agent-first-harness/env.sh" ] && . "$HOME/.config/agent-first-harness/env.sh"'
    if [ -e "$profile_path" ] && { [ ! -f "$profile_path" ] || [ -L "$profile_path" ]; }; then
        fail 24 "shell profile 不是可安全更新的普通文件：$profile_path"
    fi
    if [ -f "$profile_path" ] && grep -Fqx "$source_line" "$profile_path"; then
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
    fi
    printf '\n%s\n' "$source_line" >> "$profile_temp" || fail 24 "无法准备 shell profile 更新：$profile_path"
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
    [ -f "$profile_path" ] && [ ! -L "$profile_path" ] && grep -Fqx "$source_line" "$profile_path" || fail 24 "shell profile 写入后复核失败：$profile_path"
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

# 汇总所有将写入的安装根、profile、受管配置与稳定链接；任何冲突都在下载/安装前失败。
preflight_user_installation() {
    validate_user_login_shell
    validate_user_tool_directory_path
    if [ "$RUST_STATUS" != passed ]; then
        validate_managed_directory_path "$MANAGED_CARGO_HOME" "Rust Cargo 受管安装根"
        validate_managed_directory_path "$MANAGED_RUSTUP_HOME" "Rust rustup 受管安装根"
        for afh_tool in rustup rustc cargo; do
            validate_user_tool_destination "$afh_tool" "$MANAGED_CARGO_HOME"
        done
    fi
    if [ "$NODE_STATUS" != passed ]; then
        validate_managed_directory_path "$NODE_HOME" "Node.js 受管安装根"
        for afh_tool in node npm npx corepack; do
            validate_user_tool_destination "$afh_tool" "$NODE_HOME"
        done
    fi
    if [ "$PNPM_STATUS" != passed ]; then
        validate_managed_directory_path "$PNPM_HOME" "pnpm 受管安装根"
        for afh_tool in pnpm pnpx; do
            validate_user_tool_destination "$afh_tool" "$PNPM_HOME"
        done
    fi
    [ "$TEST_MODE" = 1 ] && [ "${AFH_SKIP_PERSIST_PATH:-0}" = 1 ] && return
    command -v cp >/dev/null 2>&1 || fail 24 "原子维护 shell profile 需要 cp"
    command -v cmp >/dev/null 2>&1 || fail 24 "原子维护 shell profile 需要 cmp"
    validate_managed_directory_path "$USER_HOME/.config" "用户级配置目录"
    validate_managed_directory_path "$USER_HOME/.config/agent-first-harness" "用户级环境目录"
    validate_managed_file_shape "$USER_HOME/.config/agent-first-harness/env.sh" "用户级环境文件"
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
}

# fish 使用原生列表 PATH；写入等价的去空、去重配置，不让 fish 解释 POSIX env.sh。
write_fish_path_config() {
    fish_root=${HOME:?必须设置 HOME}/.config/fish
    fish_config_dir=$fish_root/conf.d
    ensure_plain_directory "$fish_root" "fish 用户配置目录"
    ensure_plain_directory "$fish_config_dir" "fish 用户配置目录"
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
        printf '%s\n' 'set -l afh_user_tool_bin "$HOME/.local/share/agent-first-harness/bin"'
        printf '%s\n' 'set -l afh_path_result $afh_user_tool_bin'
        printf '%s\n' 'for afh_path_entry in $PATH'
        printf '%s\n' '    test -n "$afh_path_entry"; or continue'
        printf '%s\n' '    string match -qr "^/" -- "$afh_path_entry"; or continue'
        printf '%s\n' '    contains -- "$afh_path_entry" $afh_path_result; or set -a afh_path_result "$afh_path_entry"'
        printf '%s\n' 'end'
        printf '%s\n' 'set -gx PATH $afh_path_result'
    } > "$fish_temp" || fail 24 "无法写入 fish 用户 PATH 配置"
    [ -f "$fish_temp" ] && [ ! -L "$fish_temp" ] || fail 24 "fish 用户 PATH 临时文件不安全"
    mv -f "$fish_temp" "$fish_file" || fail 24 "无法提交 fish 用户 PATH 配置"
    FISH_TEMP_FILE=
}

# 有受管工具变化时持久化稳定用户级 PATH；任何工具变化后都由全新 login shell 锁定路径与版本复探。
persist_user_tool_path() {
    [ "$TEST_MODE" = 1 ] && [ "${AFH_SKIP_PERSIST_PATH:-0}" = 1 ] && return
    validate_user_login_shell
    managed_path_changed=0
    if [ "$RUST_CHANGED" != existing ] || [ "$NODE_CHANGED" != existing ] || [ "$PNPM_CHANGED" != existing ]; then
        managed_path_changed=1
        [ -n "${USER_BIN_DIR:-}" ] || fail 24 "没有可持久化的用户级工具目录"
        config_parent=${HOME:?必须设置 HOME}/.config
        config_dir=$config_parent/agent-first-harness
        ensure_plain_directory "$config_parent" "用户级配置目录"
        ensure_plain_directory "$config_dir" "用户级环境目录"
        env_file=$config_dir/env.sh
        [ ! -e "$env_file" ] || { [ -f "$env_file" ] && [ ! -L "$env_file" ]; } || fail 24 "用户级环境文件不是普通文件：$env_file"
        env_marker='# managed by agent-first-harness development environment gate'
        if [ -f "$env_file" ] && [ "$(sed -n '1p' "$env_file")" != "$env_marker" ]; then
            fail 24 "用户级环境文件已存在且不受门禁管理：$env_file"
        fi
        umask 077
        env_temp=$(mktemp "$config_dir/.env.sh.tmp.XXXXXX") || fail 24 "无法创建用户级环境临时文件"
        ENV_TEMP_FILE=$env_temp
        {
            printf '%s\n' "$env_marker"
            printf '%s\n' 'afh_user_tool_bin="$HOME/.local/share/agent-first-harness/bin"'
            printf '%s\n' 'afh_path_result="$afh_user_tool_bin"'
            printf '%s\n' 'afh_old_ifs=$IFS'
            printf '%s\n' 'afh_glob_was_enabled=0'
            printf '%s\n' 'case $- in *f*) ;; *) set -f; afh_glob_was_enabled=1 ;; esac'
            printf '%s\n' 'IFS=:'
            printf '%s\n' 'for afh_path_entry in ${PATH-}; do'
            printf '%s\n' '    [ -n "$afh_path_entry" ] || continue'
            printf '%s\n' '    case "$afh_path_entry" in /*) ;; *) continue ;; esac'
            printf '%s\n' '    [ "$afh_path_entry" = "$afh_user_tool_bin" ] && continue'
            printf '%s\n' '    case ":$afh_path_result:" in *":$afh_path_entry:"*) ;; *) afh_path_result=$afh_path_result:$afh_path_entry ;; esac'
            printf '%s\n' 'done'
            printf '%s\n' 'IFS=$afh_old_ifs'
            printf '%s\n' '[ "$afh_glob_was_enabled" -eq 0 ] || set +f'
            printf '%s\n' 'PATH=$afh_path_result'
            printf '%s\n' 'export PATH'
            printf '%s\n' 'unset afh_user_tool_bin afh_path_result afh_path_entry afh_old_ifs afh_glob_was_enabled'
        } > "$env_temp" || fail 24 "无法写入用户级环境文件"
        [ -f "$env_temp" ] && [ ! -L "$env_temp" ] || fail 24 "用户级环境临时文件不安全"
        mv -f "$env_temp" "$env_file" || fail 24 "无法提交用户级环境文件"
        ENV_TEMP_FILE=

        ensure_profile_source "$HOME/.profile"
        case "$LOGIN_SHELL_NAME" in
            bash|bash.exe)
                ensure_profile_source "$HOME/.bashrc"
                if [ -f "$HOME/.bash_profile" ]; then
                    ensure_profile_source "$HOME/.bash_profile"
                elif [ -f "$HOME/.bash_login" ]; then
                    ensure_profile_source "$HOME/.bash_login"
                fi
                ;;
            zsh)
                ensure_profile_source "$HOME/.zprofile"
                ensure_profile_source "$HOME/.zshrc"
                ;;
            fish)
                write_fish_path_config
                ;;
        esac
        prepend_probe_path "$USER_BIN_DIR"
    fi
    FRESH_VERIFY_DIR=$(mktemp -d) || fail 24 "无法创建新 shell 工具复探临时目录"
    [ -d "$FRESH_VERIFY_DIR" ] && [ ! -L "$FRESH_VERIFY_DIR" ] || fail 24 "新 shell 工具复探临时目录不安全"
    fresh_verifier=$(mktemp "$FRESH_VERIFY_DIR/check.XXXXXX") || fail 24 "无法创建新 shell 工具复探脚本"
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
    expected_git_path=$git_path
    expected_rustup_path=$rustup_path
    expected_rustc_path=$rustc_path
    expected_cargo_path=$cargo_path
    expected_node_path=
    expected_npm_path=
    expected_pnpm_path=
    if [ "$RUST_CHANGED" != existing ]; then
        expected_rustup_path=$USER_BIN_DIR/rustup
        expected_rustc_path=$USER_BIN_DIR/rustc
        expected_cargo_path=$USER_BIN_DIR/cargo
    fi
    expected_node_path=$node_path
    expected_npm_path=$npm_path
    expected_pnpm_path=$pnpm_path
    [ "$NODE_CHANGED" = existing ] || expected_node_path=$USER_BIN_DIR/node
    [ "$NODE_CHANGED" = existing ] || expected_npm_path=$USER_BIN_DIR/npm
    [ "$PNPM_CHANGED" = existing ] || expected_pnpm_path=$USER_BIN_DIR/pnpm
    fresh_failure=0
    persisted_environment=$(
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
        CARGO_HOME=$MANAGED_CARGO_HOME \
        RUSTUP_HOME=$MANAGED_RUSTUP_HOME \
        PATH=$BASE_SESSION_PATH \
        "$LOGIN_SHELL" -l -c '"$AFH_FRESH_VERIFY"' 2>/dev/null
    ) || fresh_failure=$?
    if [ "$fresh_failure" -ne 0 ]; then
        rm -f "$fresh_verifier"
        FRESH_VERIFY_FILE=
        fresh_failed_tool=$(printf '%s\n' "$persisted_environment" | awk -F= '$1 == "ERROR_TOOL" { print $2; exit }')
        fail 24 "新 shell 无法从持久 PATH 解析并执行同一受管工具 ${fresh_failed_tool:-unknown}（子进程退出码 ${fresh_failure}）"
    fi
    rm -f "$fresh_verifier" || fail 24 "无法清理新 shell 工具复探脚本"
    FRESH_VERIFY_FILE=
    rmdir "$FRESH_VERIFY_DIR" || fail 24 "无法清理新 shell 工具复探临时目录"
    FRESH_VERIFY_DIR=
    persisted_probe=$(printf '%s\n' "$persisted_environment" | awk -F= '$1 == "PATH" { sub(/^PATH=/, ""); print; exit }')
    if [ "$managed_path_changed" -eq 1 ]; then
        case ":$persisted_probe:" in
            *":$USER_BIN_DIR:"*) ;;
            *) fail 24 "用户级 PATH 写入后无法由新 shell 读取" ;;
        esac
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
pnpm_path=$(find_tool pnpm 2>/dev/null || true)
if [ -n "$node_path" ] && [ -n "$npm_path" ]; then
    validate_node "$node_path"
    validate_npm "$npm_path"
else
    NODE_VERSION=Missing
    NPM_VERSION=Missing
    NODE_STATUS=missing
fi
if [ -n "$pnpm_path" ]; then
    validate_pnpm "$pnpm_path"
else
    PNPM_VERSION=Missing
    PNPM_STATUS=missing
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
        [ "$PNPM_STATUS" = passed ] || exit 20
    exit 0
fi

# 任何安装都先验证可执行登录 shell；受管用户级工具还要在下载前预检全部持久化目标。
if [ "$RUST_STATUS" != passed ] || [ "$NODE_STATUS" != passed ] || [ "$PNPM_STATUS" != passed ]; then
    preflight_user_installation
elif [ "$GIT_STATUS" != passed ]; then
    validate_user_login_shell
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
    [ "$RUST_STATUS" = passed ] || fail 22 "Rust 安装或升级后仍低于 MSRV $MIN_RUST_MAJOR.$MIN_RUST_MINOR.0：$RUST_VERSION"
fi

if [ "$NODE_STATUS" != passed ]; then
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

if [ "$PNPM_STATUS" != passed ]; then
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
printf 'gate.pnpm.status=passed\n'
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
