#!/bin/sh
# 在可信 Git 项目根中原子替换精确 release 目录，拒绝符号链接与路径漂移。
set -eu

project_root=${1:-.}
canonical_root=$(CDPATH= cd -- "$project_root" 2>/dev/null && pwd -P) || {
  echo "release 准备失败：项目根目录不可读或不是目录" >&2
  exit 2
}
git_top=$(git -C "$canonical_root" rev-parse --show-toplevel 2>/dev/null) || {
  echo "release 准备失败：项目根目录不在 Git 仓库中" >&2
  exit 2
}
canonical_git_top=$(CDPATH= cd -- "$git_top" 2>/dev/null && pwd -P) || {
  echo "release 准备失败：无法解析 Git 顶层目录" >&2
  exit 2
}
if [ "$canonical_git_top" != "$canonical_root" ]; then
  echo "release 准备失败：项目根目录不是独立 Git 顶层目录" >&2
  exit 2
fi
release_path="$canonical_root/release"
if [ -L "$release_path" ]; then
  echo "release 准备失败：release 是符号链接" >&2
  exit 2
fi
if [ -e "$release_path" ] && [ ! -d "$release_path" ]; then
  echo "release 准备失败：release 已存在但不是目录" >&2
  exit 2
fi
source_commit=$(git -C "$canonical_root" rev-parse --verify 'HEAD^{commit}' 2>/dev/null) || {
  echo "release 准备失败：HEAD 不能解析为源码提交" >&2
  exit 2
}
if [ "${#source_commit}" -ne 40 ]; then
  echo "release 准备失败：HEAD 必须是 40 位小写源码提交" >&2
  exit 2
fi
case "$source_commit" in
  *[!0-9a-f]*) echo "release 准备失败：HEAD 必须是 40 位小写源码提交" >&2; exit 2 ;;
esac
if [ -n "$(git -C "$canonical_root" status --porcelain=v1 --untracked-files=all)" ]; then
  echo "release 准备失败：工作树不干净；请先完成并提交发布范围" >&2
  exit 2
fi

# 先在同一文件系统内挪走旧目录，再创建全新的 release，避免在已校验路径上
# 原地递归删除时被并发替换成外部符号链接。临时父目录权限由 mktemp 限制。
staging_parent=$(mktemp -d "$canonical_root/.release-clean.XXXXXX") || {
  echo "release 准备失败：无法预留清理暂存目录" >&2
  exit 2
}
cleanup_staging() {
  if [ -L "$staging_parent" ]; then
    rm -f -- "$staging_parent"
  elif [ -d "$staging_parent" ]; then
    find "$staging_parent" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +
    rmdir -- "$staging_parent"
  elif [ -e "$staging_parent" ]; then
    rm -f -- "$staging_parent"
  fi
}
trap cleanup_staging EXIT HUP INT TERM

if [ -d "$release_path" ]; then
  mv -- "$release_path" "$staging_parent/previous-release"
fi
if [ -e "$release_path" ] || [ -L "$release_path" ]; then
  echo "release 准备失败：原子刷新期间 release 发生变化" >&2
  exit 2
fi
mkdir -- "$release_path"
canonical_release=$(CDPATH= cd -- "$release_path" 2>/dev/null && pwd -P) || {
  echo "release 准备失败：无法解析 release" >&2
  exit 2
}
if [ "$canonical_release" != "$release_path" ] || [ "$canonical_release" != "$canonical_root/release" ]; then
  echo "release 准备失败：release 越出项目根目录" >&2
  exit 2
fi
if find "$canonical_release" -mindepth 1 -maxdepth 1 -print -quit | grep -q .; then
  echo "release 准备失败：清理后 release 仍不为空" >&2
  exit 3
fi

cleanup_staging
trap - EXIT HUP INT TERM
final_commit=$(git -C "$canonical_root" rev-parse --verify 'HEAD^{commit}' 2>/dev/null) || {
  echo "release 准备失败：清理后无法复核 HEAD" >&2
  exit 2
}
if [ "$final_commit" != "$source_commit" ] || [ -n "$(git -C "$canonical_root" status --porcelain=v1 --untracked-files=all)" ]; then
  echo "release 准备失败：清理期间 HEAD 或工作树发生变化" >&2
  exit 2
fi
printf 'release.path=%s\nrelease.cleaned=true\nrelease.source_commit=%s\n' "$canonical_release" "$source_commit"
