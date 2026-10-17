#!/bin/sh
# Linux双架构构建依赖入口；只使用既有apt/rustup，不安装新包管理器或静默绕过sudo。
set -eu
mode=check
target=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --check-only) mode=check ;;
        --install) mode=install ;;
        --target) shift; target=${1:?--target需要Rust目标三元组} ;;
        *) printf '未知参数：%s\n' "$1" >&2; exit 2 ;;
    esac
    shift
done
case "$target" in
    aarch64-unknown-linux-gnu) arch=arm64; tuple=aarch64-linux-gnu ;;
    x86_64-unknown-linux-gnu) arch=amd64; tuple=x86_64-linux-gnu ;;
    *) printf '仅支持aarch64-unknown-linux-gnu或x86_64-unknown-linux-gnu\n' >&2; exit 2 ;;
esac
[ "$(uname -s)" = Linux ] || { printf '需要Linux宿主\n' >&2; exit 2; }
command -v dpkg >/dev/null && command -v apt-get >/dev/null || { printf '自动安装仅适用于既有Debian/Ubuntu apt宿主\n' >&2; exit 2; }
native_arch=$(dpkg --print-architecture)
case "$native_arch" in arm64|amd64) ;; *) printf '不支持的宿主架构\n' >&2; exit 2 ;; esac

# 权限由真实交互终端处理；不使用sudo -S、缓存密码或调整系统权限。
as_root() {
    if [ "$(id -u)" -eq 0 ]; then "$@"; else sudo -- "$@"; fi
}

if [ "$mode" = install ]; then
    if [ "$native_arch" != "$arch" ] && ! dpkg --print-foreign-architectures | awk -v expected="$arch" '$0 == expected {found=1} END {exit !found}'; then
        as_root /usr/bin/dpkg --add-architecture "$arch"
    fi
    as_root apt-get update
    as_root apt-get install -y build-essential pkg-config curl binutils tar dpkg-dev \
        "gcc-$tuple" "g++-$tuple" \
        "libfontconfig1-dev:$arch" "libfreetype-dev:$arch" \
        "libxkbcommon-dev:$arch" "libxkbcommon-x11-dev:$arch" \
        "libwayland-dev:$arch" "libx11-dev:$arch" "libx11-xcb-dev:$arch" "libxcb1-dev:$arch"
    rustup target add "$target"
fi

# 检查真实编译器及目标pkg-config目录，不允许把宿主头文件冒充交叉目标依赖。
missing=0
for tool in rustup cargo rustc "$tuple-gcc" "$tuple-g++" pkg-config readelf tar dpkg-deb; do
    if command -v "$tool" >/dev/null 2>&1; then printf 'linux.tool.%s=present\n' "$tool";
    else printf 'linux.tool.%s=missing\n' "$tool"; missing=1; fi
done
if ! rustup target list --installed | awk -v expected="$target" '$0 == expected {found=1} END {exit !found}'; then
    printf 'linux.rust.target=missing\n'; missing=1
else printf 'linux.rust.target=%s\n' "$target"; fi
for library in fontconfig freetype2 xkbcommon xkbcommon-x11 wayland-client x11 x11-xcb xcb; do
    if version=$(PKG_CONFIG_SYSROOT_DIR=/ PKG_CONFIG_LIBDIR="/usr/lib/$tuple/pkgconfig:/usr/share/pkgconfig" PKG_CONFIG_PATH= pkg-config --modversion "$library" 2>/dev/null); then
        printf 'linux.library.%s=%s\n' "$library" "$version"
    else printf 'linux.library.%s=missing\n' "$library"; missing=1; fi
done
printf 'linux.target=%s\n' "$target"
[ "$missing" -eq 0 ] || exit 20
