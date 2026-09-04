import profileJson from "../brand-support-profile.json";

import { formatDisplayVersion } from "./displayVersion";

/** 品牌联系人及其用户可见渠道。 */
export interface BrandSupportContact {
  channel: string;
  value: string;
}

/** 单条赞助权益使用的翻译键。 */
export interface BrandSupportBenefit {
  mainKey: string;
  noteKey?: string;
}

/** 固定赞助档位、价格、头图和权益键。 */
export interface BrandSupportTier {
  id: string;
  price: number;
  currencyCode: string;
  image: string;
  nameKey: string;
  imageAltKey: string;
  benefits: BrandSupportBenefit[];
}

/** 品牌支付码及其本地资源与替代文本键。 */
export interface BrandSupportPayment {
  id: string;
  image: string;
  altKey: string;
}

/** 赞助页面使用的共享品牌事实。 */
export interface BrandSupportProfile {
  schemaVersion: number;
  scope: string;
  publicBasePath: string;
  brand: {
    displayNameZhCN: string;
    displayNameEnUS: string;
  };
  contacts: {
    support: BrandSupportContact;
  };
  sponsor: {
    background: string;
    tiers: BrandSupportTier[];
    payments: BrandSupportPayment[];
  };
  optionalAssets: string[];
}

/** 由 validator 机械校验后供页面模板消费的品牌事实。 */
export const BRAND_SUPPORT_PROFILE = profileJson as BrandSupportProfile;

/** 使用权威应用名和版本组装固定窗口标题。 */
export function formatBrandWindowTitle(
  applicationName: string,
  version: string,
): string {
  const normalizedName = applicationName.trim();
  const normalizedVersion = formatDisplayVersion(version);
  if (!normalizedName || !normalizedVersion) {
    throw new Error("window title requires application name and version");
  }
  return `${normalizedName} ${normalizedVersion}`;
}

/** 判断路径是否为不含远程 scheme、反斜杠或上级跳转的本地绝对路径。 */
export function isLocalSupportPath(value: string): boolean {
  return (
    value.startsWith("/") &&
    !value.startsWith("//") &&
    !value.includes("..") &&
    !value.includes("\\") &&
    !value.includes("://")
  );
}

/** 把清单中的相对资源路径解析到应用本地 public 根。 */
export function resolveBrandAssetPath(
  basePath: string,
  relativePath: string,
): string {
  if (!isLocalSupportPath(basePath)) {
    throw new Error("brand support base path must be a local absolute path");
  }
  if (
    relativePath.startsWith("/") ||
    relativePath.includes("..") ||
    relativePath.includes("\\") ||
    relativePath.includes("://")
  ) {
    throw new Error("brand support asset path must be a safe relative path");
  }
  return `${basePath.replace(/\/$/, "")}/${relativePath}`;
}
