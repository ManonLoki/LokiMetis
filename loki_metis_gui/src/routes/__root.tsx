import { Button, Center, Stack, Text, Title } from "@mantine/core";
import {
  createRootRoute,
  Outlet,
  useNavigate,
  useRouterState,
} from "@tanstack/react-router";
import { IconArrowLeft } from "@tabler/icons-react";
import { useEffect, type ReactElement } from "react";
import { useTranslation } from "react-i18next";

import { AppShellFrame } from "../components/AppShell";
import { isPetOverlayPath, isPetSettingsPath, isPetWindowPath } from "../default-landing";

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
            void navigate({ replace: true, to: "/dashboard" });
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
            void navigate({ replace: true, to: "/dashboard" });
          }}
          variant="light"
        >
          {t("errors.back_home")}
        </Button>
      </Stack>
    </Center>
  );
}

/** 桌宠与桌宠设置窗不挂主壳；其它路由继续使用精简侧栏壳。 */
function RootLayout(): ReactElement {
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const pet = isPetOverlayPath(pathname);
  const petSettings = isPetSettingsPath(pathname);
  const petWindow = isPetWindowPath(pathname);
  useEffect(() => {
    document.documentElement.classList.toggle("pet-window", pet);
    document.documentElement.classList.toggle("pet-settings-window", petSettings);
    return () => {
      document.documentElement.classList.remove("pet-window");
      document.documentElement.classList.remove("pet-settings-window");
    };
  }, [pet, petSettings]);
  if (petWindow) {
    return <Outlet />;
  }
  return <AppShellFrame />;
}

export const Route = createRootRoute({
  component: RootLayout,
  errorComponent: RouteErrorPage,
  notFoundComponent: NotFoundPage,
});
