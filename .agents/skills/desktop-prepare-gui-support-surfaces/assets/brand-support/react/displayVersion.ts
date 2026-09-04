/** 把权威机器版本转换为仅带一个小写 `v` 的用户可见版本。 */
export function formatDisplayVersion(version: string): string {
  const normalized = version.trim().replace(/^[vV]+/, "");
  if (!normalized) {
    throw new Error("display version must not be empty");
  }
  return `v${normalized}`;
}
