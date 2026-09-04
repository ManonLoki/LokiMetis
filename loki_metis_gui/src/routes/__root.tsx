import { Button, Center, Stack, Text, Title } from "@mantine/core";
import { createRootRoute, useNavigate } from "@tanstack/react-router";
import { IconArrowLeft } from "@tabler/icons-react";
import type { ReactElement } from "react";
import { useTranslation } from "react-i18next";

import { AppShellFrame } from "../components/AppShell";

/** 渲染未知路由的可恢复本地错误页。 */
function NotFoundPage(): ReactElement {
  const { t } = useTranslation();
  const navigate = useNavigate();
  return (
    <Center mih="70dvh">
      <Stack align="center" gap="md">
        <Title order={1}>{t("errors.not_found_title")}</Title>
        <Text c="dimmed">{t("errors.not_found_description")}</Text>
        <Button
          leftSection={<IconArrowLeft aria-hidden="true" size={18} />}
          onClick={() => {
            void navigate({ to: "/" });
          }}
          variant="light"
        >
          {t("errors.back_home")}
        </Button>
      </Stack>
    </Center>
  );
}

/** 渲染路由异常的最小恢复页面，不泄漏底层错误。 */
function RouteErrorPage(): ReactElement {
  const { t } = useTranslation();
  const navigate = useNavigate();
  return (
    <Center mih="70dvh">
      <Stack align="center" gap="md">
        <Title order={1}>{t("errors.unexpected_title")}</Title>
        <Text c="dimmed">{t("errors.unexpected_description")}</Text>
        <Button
          leftSection={<IconArrowLeft aria-hidden="true" size={18} />}
          onClick={() => {
            void navigate({ to: "/" });
          }}
          variant="light"
        >
          {t("errors.back_home")}
        </Button>
      </Stack>
    </Center>
  );
}

export const Route = createRootRoute({
  component: AppShellFrame,
  errorComponent: RouteErrorPage,
  notFoundComponent: NotFoundPage,
});
