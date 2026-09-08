import { Image, Paper, SimpleGrid, Stack, Text, Title } from "@mantine/core";
import type { ReactElement } from "react";
import { useTranslation } from "react-i18next";

/** 设置页只展示用户明确提供且逐字节保留的两张本地收款码。 */
const SPONSOR_PAYMENT_IMAGES = [
  {
    altKey: "settings.sponsor_wechat_alt",
    id: "wechat-pay",
    labelKey: "settings.sponsor_wechat_label",
    src: "/brand-support/sponsor/wechat-pay.png",
  },
  {
    altKey: "settings.sponsor_alipay_alt",
    id: "alipay",
    labelKey: "settings.sponsor_alipay_label",
    src: "/brand-support/sponsor/alipay.jpg",
  },
] as const;

/** 在设置页底部以响应式双列展示本地赞助收款码。 */
export function SponsorPaymentPanel(): ReactElement {
  const { t } = useTranslation();

  return (
    <Paper
      className="surface-card"
      data-testid="settings-sponsor-section"
      p="xl"
      radius="lg"
      withBorder
    >
      <Stack gap="lg">
        <Stack gap={4}>
          <Title order={2} size="h3">
            {t("settings.sponsor_title")}
          </Title>
          <Text c="dimmed" size="sm">
            {t("settings.sponsor_description")}
          </Text>
        </Stack>

        <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="lg">
          {SPONSOR_PAYMENT_IMAGES.map((payment) => (
            <Paper component="figure" key={payment.id} m={0} p="md" radius="lg" withBorder>
              <Stack align="center" gap="sm">
                <Text component="figcaption" fw={650}>
                  {t(payment.labelKey)}
                </Text>
                <Image
                  alt={t(payment.altKey)}
                  fit="contain"
                  loading="lazy"
                  mah={420}
                  radius="md"
                  src={payment.src}
                  w="100%"
                />
              </Stack>
            </Paper>
          ))}
        </SimpleGrid>
      </Stack>
    </Paper>
  );
}
