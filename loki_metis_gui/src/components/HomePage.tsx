import {
  Badge,
  Box,
  Card,
  Group,
  SimpleGrid,
  Skeleton,
  Stack,
  Text,
  ThemeIcon,
  Title,
} from "@mantine/core";
import { useQuery } from "@tanstack/react-query";
import { IconCircleCheck, IconCompass, IconSparkles } from "@tabler/icons-react";
import type { ReactElement } from "react";
import { useTranslation } from "react-i18next";

import { appMetadataQuery } from "../lib/queries";

/** 渲染已定义本机 AI 工作台的轻量欢迎页。 */
export function HomePage(): ReactElement {
  const { t } = useTranslation();
  const metadata = useQuery(appMetadataQuery);
  const productDefinitionRequired = metadata.data?.productDefinitionRequired ?? true;

  return (
    <Stack data-testid="home-page" gap={28}>
      <Box className="hero-panel" p={{ base: "xl", md: 42 }}>
        <Stack gap="lg" maw={760}>
          <Group gap="xs">
            <ThemeIcon radius="xl" size="lg" variant="light">
              <IconSparkles aria-hidden="true" size={20} stroke={1.75} />
            </ThemeIcon>
            <Text c="indigo" fw={700} size="sm">
              {t("home.eyebrow")}
            </Text>
          </Group>
          {metadata.isLoading ? (
            <Skeleton h={52} radius="md" w="72%" />
          ) : (
            <Title className="hero-title" order={1}>
              {t("home.title")}
            </Title>
          )}
          <Text c="dimmed" lh={1.7} maw={680} size="lg">
            {t("home.description")}
          </Text>
          {productDefinitionRequired ? (
            <Badge color="orange" radius="sm" size="lg" variant="light">
              {t("home.definition_required")}
            </Badge>
          ) : null}
        </Stack>
      </Box>

      <SimpleGrid cols={{ base: 1, md: 2 }} spacing="lg">
        <Card className="surface-card" p="xl" radius="lg" withBorder>
          <Stack gap="md">
            <ThemeIcon color="teal" radius="md" size={42} variant="light">
              <IconCircleCheck aria-hidden="true" size={23} stroke={1.75} />
            </ThemeIcon>
            <Text c="dimmed" fw={600} size="sm">
              {t("home.status_label")}
            </Text>
            <Title order={2} size="h3">
              {t("home.status_ready")}
            </Title>
          </Stack>
        </Card>

        <Card className="surface-card" p="xl" radius="lg" withBorder>
          <Stack gap="md">
            <ThemeIcon color="indigo" radius="md" size={42} variant="light">
              <IconCompass aria-hidden="true" size={23} stroke={1.75} />
            </ThemeIcon>
            <Text c="dimmed" fw={600} size="sm">
              {t("home.next_step_title")}
            </Text>
            <Title order={2} size="h3">
              {t("home.definition_label")}
            </Title>
            <Text c="dimmed" lh={1.65}>
              {t("home.next_step_description")}
            </Text>
          </Stack>
        </Card>
      </SimpleGrid>

      <Card className="workspace-card" p={{ base: "xl", md: 32 }} radius="lg" withBorder>
        <Stack gap="xs">
          <Title order={2} size="h3">
            {t("home.workspace_title")}
          </Title>
          <Text c="dimmed">{t("home.workspace_description")}</Text>
        </Stack>
      </Card>
    </Stack>
  );
}
