import {
  Alert,
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
import { IconAlertCircle, IconRefresh } from "@tabler/icons-react";
import type { ReactElement } from "react";
import { useTranslation } from "react-i18next";

import type { InterfaceLanguage } from "../lib/api";
import {
  formatDisplayVersion,
  selectVisibleReleaseNotes,
  type ReleaseNotesDocument,
} from "../lib/releaseNotes";

/** 表示更新日志请求的可观察生命周期。 */
export type ReleaseNotesStatus = "idle" | "loading" | "ready" | "error";

/** 描述本地更新日志弹窗的受控打开状态。 */
export interface ReleaseNotesDialogProps {
  language: InterfaceLanguage;
  opened: boolean;
  releases: ReleaseNotesDocument | undefined;
  status: ReleaseNotesStatus;
  onClose: () => void;
  onRetry: () => void;
}

/** 加载并渲染候选内受双层校验的本地更新日志。 */
export function ReleaseNotesDialogTemplate({
  language,
  opened,
  releases,
  status,
  onClose,
  onRetry,
}: ReleaseNotesDialogProps): ReactElement {
  const { t } = useTranslation();
  const visible =
    releases === undefined ? [] : selectVisibleReleaseNotes(releases, language);

  return (
    <Modal
      centered
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
        <Alert
          icon={<IconAlertCircle aria-hidden="true" size={20} />}
          title={t("release_notes.load_failed_title")}
        >
          <Stack align="flex-start" gap="md">
            <Text>{t("release_notes.load_failed")}</Text>
            <Button
              leftSection={<IconRefresh aria-hidden="true" size={18} />}
              onClick={onRetry}
              variant="light"
            >
              {t("release_notes.retry")}
            </Button>
          </Stack>
        </Alert>
      ) : (
        <Stack data-testid="release-notes-list" gap="lg">
          {visible.map((release) => (
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
