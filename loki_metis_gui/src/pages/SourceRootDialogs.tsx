import { Button, Group, Modal, Stack, Text, TextInput } from '@mantine/core';
import { useTranslation } from 'react-i18next';

/** 标识数据根弹窗当前操作的稳定目标与安全别名。 */
interface DialogTarget {
  alias: string;
  id: string;
}

/** 定义数据根重命名、重索引和删除弹窗的共享输入。 */
interface SourceRootDialogsProps {
  clientLabel: string;
  onCloseRemove: () => void;
  onCloseRename: () => void;
  onConfirmRemove: (rootId: string) => void;
  onConfirmRename: (rootId: string, alias: string) => void;
  onRenameDraftChange: (alias: string) => void;
  removePending: boolean;
  removeTarget: DialogTarget | null;
  renameDraft: string;
  renamePending: boolean;
  renameTarget: DialogTarget | null;
}

/** 集中展示数据根重命名与移除确认，避免主页面承担对话框细节。 */
export function SourceRootDialogs({
  clientLabel,
  onCloseRemove,
  onCloseRename,
  onConfirmRemove,
  onConfirmRename,
  onRenameDraftChange,
  removePending,
  removeTarget,
  renameDraft,
  renamePending,
  renameTarget,
}: SourceRootDialogsProps) {
  const { t } = useTranslation();
  return (
    <>
      <Modal
        aria-label={t('sources.dialog.renameTitle')}
        centered
        onClose={onCloseRename}
        opened={renameTarget !== null}
        title={t('sources.dialog.renameTitle')}
        transitionProps={{ duration: 0 }}
      >
        <Stack gap="md">
          <TextInput
            autoFocus
            label={t('sources.dialog.renameLabel')}
            onChange={(event) => onRenameDraftChange(event.currentTarget.value)}
            value={renameDraft}
          />
          <Group justify="flex-end">
            <Button onClick={onCloseRename} variant="subtle">
              {t('common.cancel')}
            </Button>
            <Button
              disabled={
                !renameTarget ||
                renameDraft.trim().length === 0 ||
                renameDraft.trim() === renameTarget.alias
              }
              loading={renamePending}
              onClick={() => renameTarget && onConfirmRename(renameTarget.id, renameDraft)}
            >
              {t('sources.table.rename')}
            </Button>
          </Group>
        </Stack>
      </Modal>
      <Modal
        aria-label={t('sources.dialog.removeTitle')}
        centered
        onClose={onCloseRemove}
        opened={removeTarget !== null}
        title={t('sources.dialog.removeTitle')}
        transitionProps={{ duration: 0 }}
      >
        <Stack gap="md">
          <Text size="sm">
            {t('sources.dialog.removeBody', {
              alias: removeTarget?.alias ?? '',
              client: clientLabel,
            })}
          </Text>
          <Group justify="flex-end">
            <Button onClick={onCloseRemove} variant="subtle">
              {t('common.cancel')}
            </Button>
            <Button
              color="red"
              loading={removePending}
              onClick={() => removeTarget && onConfirmRemove(removeTarget.id)}
            >
              {t('sources.dialog.removeConfirm')}
            </Button>
          </Group>
        </Stack>
      </Modal>
    </>
  );
}
