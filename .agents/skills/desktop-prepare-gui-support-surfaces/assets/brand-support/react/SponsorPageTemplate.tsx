import {
  Badge,
  Box,
  Group,
  Image,
  Paper,
  SimpleGrid,
  Stack,
  Text,
  Title,
  useComputedColorScheme,
} from "@mantine/core";
import type { ReactElement } from "react";
import { useTranslation } from "react-i18next";

import {
  BRAND_SUPPORT_PROFILE,
  type BrandSupportProfile,
  type BrandSupportTier,
  resolveBrandAssetPath,
} from "./brandSupportProfile";
import { SupportMedia } from "./SupportMedia";

/** 单个品牌赞助档位卡片的渲染参数。 */
interface TierCardProps {
  tier: BrandSupportTier;
  assetBasePath: string;
  colorScheme: SponsorColorScheme;
}

/** 赞助页只消费 Mantine 已解析后的亮色或暗色主题。 */
type SponsorColorScheme = "light" | "dark";

/** 为当前主题生成不依赖新 CSS 函数的本地背景叠层。 */
function sponsorBackgroundImage(
  colorScheme: SponsorColorScheme,
  background: string,
): string {
  const overlay =
    colorScheme === "dark"
      ? "linear-gradient(rgba(9, 11, 18, 0.84), rgba(9, 11, 18, 0.92))"
      : "linear-gradient(rgba(248, 250, 255, 0.68), rgba(248, 250, 255, 0.82))";
  return `${overlay}, url("${background}")`;
}

/** 卡片与说明区块的表面背景随主题切换。 */
function sponsorSurfaceColor(colorScheme: SponsorColorScheme): string {
  return colorScheme === "dark" ? "dark.7" : "white";
}

/** 标题与强调文本的强调色随主题切换。 */
function sponsorAccentColor(colorScheme: SponsorColorScheme): string {
  return colorScheme === "dark" ? "blue.3" : "blue.8";
}

/** 渲染固定价格、档位插图与本地化权益。 */
function TierCard({
  tier,
  assetBasePath,
  colorScheme,
}: TierCardProps): ReactElement {
  const { t } = useTranslation("brandSupport");
  const name = t(tier.nameKey);

  return (
    <Paper
      aria-label={name}
      bg={sponsorSurfaceColor(colorScheme)}
      component="article"
      data-color-scheme={colorScheme}
      data-price={tier.price}
      p="lg"
      radius="lg"
      shadow="sm"
      withBorder
    >
      <Stack gap="md">
        <Badge size="lg" variant="filled">
          {t("sponsor.scan_to_sponsor")}
        </Badge>
        <Group align="center" gap="md" wrap="nowrap">
          <Paper bg="gray.0" p={0} radius="lg" withBorder>
            <Image
              alt={t(tier.imageAltKey)}
              fit="contain"
              h={86}
              radius="lg"
              src={resolveBrandAssetPath(assetBasePath, tier.image)}
              w={86}
            />
          </Paper>
          <Box>
            <Text c={sponsorAccentColor(colorScheme)} fw={600} size="sm">
              {name}
            </Text>
            <Group align="baseline" gap={4} wrap="nowrap">
              <Text fw={900} size="3rem">
                {tier.price}
              </Text>
              <Text c="dimmed" size="lg">
                {t("sponsor.currency_unit")}
              </Text>
            </Group>
          </Box>
        </Group>
        <Stack gap="sm">
          {tier.benefits.map((benefit) => (
            <Box key={benefit.mainKey}>
              <Group align="flex-start" gap="xs" wrap="nowrap">
                <Text aria-hidden="true" c="green" fw={900}>
                  ✓
                </Text>
                <Text fw={600} size="sm">
                  {t(benefit.mainKey)}
                </Text>
              </Group>
              {benefit.noteKey ? (
                <Text
                  c="dimmed"
                  ml="xl"
                  size="xs"
                  style={{ whiteSpace: "pre-line" }}
                >
                  {t(benefit.noteKey)}
                </Text>
              ) : null}
            </Box>
          ))}
        </Stack>
      </Stack>
    </Paper>
  );
}

/** 品牌赞助页模板的资源根和可替换 profile。 */
export interface SponsorPageTemplateProps {
  assetBasePath?: string;
  profile?: BrandSupportProfile;
}

/** 展示共享品牌文案、固定三档价格和双支付码，并适配窄窗与主题。 */
export function SponsorPageTemplate({
  assetBasePath = BRAND_SUPPORT_PROFILE.publicBasePath,
  profile = BRAND_SUPPORT_PROFILE,
}: SponsorPageTemplateProps): ReactElement {
  const { t } = useTranslation("brandSupport");
  const colorScheme = useComputedColorScheme("light");
  const background = resolveBrandAssetPath(
    assetBasePath,
    profile.sponsor.background,
  );
  const capabilities = [
    "sponsor.capability_browser",
    "sponsor.capability_proxy",
    "sponsor.capability_cloud",
    "sponsor.capability_rpa",
  ];

  return (
    <Box
      data-color-scheme={colorScheme}
      data-testid="brand-sponsor-page"
      p={{ base: "sm", sm: "lg" }}
      style={{
        backgroundColor:
          colorScheme === "dark"
            ? "var(--mantine-color-dark-9)"
            : "var(--mantine-color-blue-0)",
        backgroundImage: sponsorBackgroundImage(colorScheme, background),
        backgroundPosition: "center",
        backgroundSize: "cover",
        minHeight: "100%",
        overflowY: "auto",
      }}
    >
      <Stack gap="lg" maw={1180} mx="auto">
        <Stack align="center" gap="xs" ta="center">
          <Title c={sponsorAccentColor(colorScheme)} order={2}>
            {t("sponsor.title")}
          </Title>
          <Text fw={600}>
            {t("sponsor.subtitle_no_service")}
            <Text component="span" fw={400}>
              {t("sponsor.subtitle_thanks")}
            </Text>
          </Text>
        </Stack>

        <Stack align="center" gap="xs">
          <Group gap="xs" justify="center" wrap="wrap">
            <Text c={sponsorAccentColor(colorScheme)} fw={600} size="sm">
              {t("sponsor.capabilities_label")}
            </Text>
            {capabilities.map((key) => (
              <Badge key={key} variant="light">
                {t(key)}
              </Badge>
            ))}
          </Group>
          <Text size="sm" ta="center">
            <Text component="span" fw={700}>
              {t("sponsor.pc_focus")}
            </Text>
            {t("sponsor.pc_platform")}
            <Text component="span" fw={700}>
              {t("sponsor.pc_growing")}
            </Text>
          </Text>
        </Stack>

        <SimpleGrid
          data-testid="brand-sponsor-grid"
          cols={{ base: 1, sm: 2, lg: 3 }}
          spacing="md"
        >
          {profile.sponsor.tiers.map((tier) => (
            <TierCard
              key={tier.id}
              assetBasePath={assetBasePath}
              colorScheme={colorScheme}
              tier={tier}
            />
          ))}
        </SimpleGrid>

        <Paper
          bg={sponsorSurfaceColor(colorScheme)}
          p="lg"
          radius="lg"
          withBorder
        >
          <SimpleGrid
            cols={{ base: 1, md: 2 }}
            spacing="lg"
            verticalSpacing="lg"
          >
            <Stack gap="sm" justify="center">
              <Title order={3}>{t("sponsor.payment_instructions")}</Title>
              <Text c="dimmed" size="sm">
                {t("sponsor.sponsor_message")}
              </Text>
              <Text size="sm">
                {t("sponsor.contact_label", {
                  contact: profile.contacts.support.value,
                })}
              </Text>
            </Stack>
            <SimpleGrid cols={{ base: 1, xs: 2 }} spacing="md">
              {profile.sponsor.payments.map((payment) => (
                <Box key={payment.id} maw={160} mx="auto">
                  <SupportMedia
                    media={{
                      alt: t(payment.altKey),
                      kind: "image",
                      src: resolveBrandAssetPath(assetBasePath, payment.image),
                    }}
                  />
                </Box>
              ))}
            </SimpleGrid>
          </SimpleGrid>
        </Paper>
      </Stack>
    </Box>
  );
}
