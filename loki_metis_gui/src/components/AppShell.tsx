import { Alert, AppShell, Button, Container, Stack } from "@mantine/core";
import { useQuery } from "@tanstack/react-query";
import { Outlet, useNavigate, useRouterState } from "@tanstack/react-router";
import { IconAlertCircle, IconRefresh } from "@tabler/icons-react";
import { useSetAtom } from "jotai";
import { useEffect, type ReactElement } from "react";
import { useTranslation } from "react-i18next";

import {
  getAppMetadata,
  getSystemLocale,
  setInterfaceLanguage,
  type AppMetadata,
  type InterfaceLanguage,
} from "../lib/api";
import { readSavedInterfaceLanguage } from "../lib/language";
import { appMetadataQuery } from "../lib/queries";
import { interfaceLanguageAtom } from "../state/interfaceLanguage";
import { AppSidebar, APP_SIDEBAR_WIDTHS } from "./AppSidebar";

/** 通过固定 IPC 为壳层预取并验证安装包元数据。 */
export async function preloadShellMetadata(): Promise<AppMetadata> {
  return getAppMetadata();
}

/** 仅在没有已保存偏好时读取宿主规范系统语言。 */
export async function preloadShellLocale(): Promise<InterfaceLanguage> {
  return getSystemLocale();
}

/** 由设置页同步原生菜单语言并返回宿主权威值。 */
export async function syncShellInterfaceLanguage(
  language: InterfaceLanguage,
): Promise<InterfaceLanguage> {
  return setInterfaceLanguage(language);
}

export const applyInterfaceLanguage = syncShellInterfaceLanguage;

/** 渲染固定精简 AppShell，并让应用事实和语言通过真实宿主边界进入界面。 */
export function AppShellFrame(): ReactElement {
  const { i18n, t } = useTranslation();
  const setLanguage = useSetAtom(interfaceLanguageAtom);
  const navigate = useNavigate();
  const activePath = useRouterState({ select: (state) => state.location.pathname });
  const metadata = useQuery({ ...appMetadataQuery, queryFn: preloadShellMetadata });
  const applicationName = metadata.data?.applicationName ?? t("identity.application_name");
  const version = metadata.data?.version ?? "0.1.0";

  useEffect(() => {
    if (metadata.data !== undefined) document.title = metadata.data.title;
  }, [metadata.data]);

  useEffect(() => {
    if (readSavedInterfaceLanguage() !== undefined) return;
    let active = true;
    void preloadShellLocale()
      .then(async (language) => {
        if (!active) return;
        await i18n.changeLanguage(language);
        if (active) setLanguage(language);
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, [i18n, setLanguage]);

  return (
    <AppShell
      data-mode="compact"
      data-navbar-width={APP_SIDEBAR_WIDTHS.compact}
      data-testid="app-shell"
      navbar={{ breakpoint: "xs", width: APP_SIDEBAR_WIDTHS.compact }}
    >
      <AppShell.Navbar p={0}>
        <AppSidebar
          activePath={activePath}
          applicationName={applicationName}
          onNavigate={(path) => {
            void navigate({ to: path });
          }}
          version={version}
        />
      </AppShell.Navbar>
      <AppShell.Main>
        <Container fluid p={{ base: "lg", md: 32 }}>
          <Stack gap="lg">
            {metadata.isError ? (
              <Alert
                icon={<IconAlertCircle aria-hidden="true" size={20} />}
                title={t("errors.metadata_title")}
              >
                <Stack align="flex-start" gap="sm">
                  {t("errors.metadata_description")}
                  <Button
                    leftSection={<IconRefresh aria-hidden="true" size={18} />}
                    onClick={() => {
                      void metadata.refetch();
                    }}
                    size="xs"
                    variant="light"
                  >
                    {t("errors.retry")}
                  </Button>
                </Stack>
              </Alert>
            ) : null}
            <Outlet />
          </Stack>
        </Container>
      </AppShell.Main>
    </AppShell>
  );
}

export const AppLayout = AppShellFrame;
