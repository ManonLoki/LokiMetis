import {
  Button,
  Center,
  List,
  Loader,
  Modal,
  Paper,
  Stack,
  Text,
  Title,
} from "@mantine/core";
import { IconRefresh } from "@tabler/icons-react";
import type { ReactElement } from "react";
import { useTranslation } from "react-i18next";

import { formatDisplayVersion } from "./displayVersion";
import {
  resolveReleaseNotesLocale,
  selectVisibleReleaseNotes,
  type LocalizedReleaseNoteEntry,
} from "./releaseNotes";

/** 更新日志弹窗所需的本地发布事实和关闭交互。 */
export interface ReleaseNotesDialogTemplateProps {
  opened: boolean;
  releases: readonly LocalizedReleaseNoteEntry[];
  status: "idle" | "loading" | "ready" | "error";
  onRetry: () => void;
  onClose: () => void;
}

/** 按当前 i18n locale 展示最多五个版本、每类最多十条的更新日志。 */
export function ReleaseNotesDialogTemplate({
  opened,
  releases,
  status,
  onRetry,
  onClose,
}: ReleaseNotesDialogTemplateProps): ReactElement {
  const { i18n, t } = useTranslation("brandSupport");
  const locale = resolveReleaseNotesLocale(i18n.resolvedLanguage);
  const visibleReleases = selectVisibleReleaseNotes(releases, locale);

  return (
    <Modal
      onClose={onClose}
      opened={opened}
      size="lg"
      title={t("release_notes.dialog_title")}
    >
      {status === "idle" || status === "loading" ? (
        <Center py="xl">
          <Stack align="center" gap="sm">
            <Loader aria-label={t("release_notes.loading")} size="sm" />
            <Text c="dimmed">{t("release_notes.loading")}</Text>
          </Stack>
        </Center>
      ) : status === "error" ? (
        <Stack align="flex-start" gap="md">
          <Text role="alert">{t("release_notes.load_failed")}</Text>
          <Button
            leftSection={<IconRefresh aria-hidden="true" size={18} />}
            onClick={onRetry}
            variant="default"
          >
            {t("release_notes.retry")}
          </Button>
        </Stack>
      ) : visibleReleases.length === 0 ? (
        <Text c="dimmed">{t("release_notes.empty")}</Text>
      ) : (
        <Stack data-testid="release-notes-list" gap="lg">
          {visibleReleases.map((release) => (
            <Paper key={release.version} p="md" radius="md" withBorder>
              <Stack gap="sm">
                <Title order={3} size="h4">
                  {t("release_notes.entry_title", {
                    date: release.releaseDate,
                    version: formatDisplayVersion(release.version),
                  })}
                </Title>
                <Title order={4} size="h5">
                  {t("release_notes.feature_optimizations")}
                </Title>
                {release.featureOptimizations.length > 0 ? (
                  <List spacing="xs">
                    {release.featureOptimizations.map((item) => (
                      <List.Item key={item}>{item}</List.Item>
                    ))}
                  </List>
                ) : (
                  <Text c="dimmed">{t("release_notes.none")}</Text>
                )}
                <Title order={4} size="h5">
                  {t("release_notes.bug_fixes")}
                </Title>
                {release.bugFixes.length > 0 ? (
                  <List spacing="xs">
                    {release.bugFixes.map((item) => (
                      <List.Item key={item}>{item}</List.Item>
                    ))}
                  </List>
                ) : (
                  <Text c="dimmed">{t("release_notes.none")}</Text>
                )}
              </Stack>
            </Paper>
          ))}
        </Stack>
      )}
    </Modal>
  );
}
