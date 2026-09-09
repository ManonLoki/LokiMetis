/** 把权威机器版本转换为仅带一个小写 `v` 的用户可见版本。 */
export function formatDisplayVersion(version: string): string {
  const normalized = version.trim().replace(/^[vV]+/, "");
  if (!normalized) {
    throw new Error("display version must not be empty");
  }
  return `v${normalized}`;
}

/** 使用权威应用名和版本组装固定窗口标题。 */
export function formatAppWindowTitle(
  applicationName: string,
  version: string,
): string {
  const normalizedName = applicationName.trim();
  const normalizedVersion = formatDisplayVersion(version);
  if (!normalizedName) {
    throw new Error("window title requires application name and version");
  }
  return `${normalizedName} ${normalizedVersion}`;
}
