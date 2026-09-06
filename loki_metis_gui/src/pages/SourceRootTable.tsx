import { Badge, Button, Group, Paper, Table, Text } from "@mantine/core";
import { useTranslation } from "react-i18next";

import type { SourceRootDto } from "../api/usage";
import { formatObservedAt } from "../usage-format";
import { sourceDiscoveryLabel } from "../i18n/backend-labels";
import { displayRootAlias } from "../root-label";

/** 展示已授权数据根的索引结果表，并暴露启用、重新索引、重命名、移除操作。 */
// “受控组件”：这个组件自己不持有任何业务状态（没有 useState 管理数据），
// 所有数据通过 props 传入，所有操作通过 onXxx 回调交还给父组件
// （SourcesPage.tsx）处理，父组件决定操作是否正在进行中（xxxPending）、
// 操作成功后如何刷新数据。这样表格本身是纯展示、易测试的，
// 复杂的 mutation/查询逻辑集中在一处，不会分散在多个组件里。
export function SourceRootTable({
  onRemove,
  onRename,
  onReindex,
  onToggleEnabled,
  currentRootId,
  removePending,
  reindexPendingRootId,
  renamePending,
  roots,
  scanRunning,
  showPrimary = false,
  allowRenameRemove = true,
  togglePending,
}: {
  roots: SourceRootDto[];
  currentRootId: string | null;
  scanRunning: boolean;
  togglePending: boolean;
  renamePending: boolean;
  removePending: boolean;
  reindexPendingRootId: string | null;
  showPrimary?: boolean;
  allowRenameRemove?: boolean;
  onToggleEnabled: (rootId: string, nextEnabled: boolean) => void;
  onRename: (rootId: string, currentAlias: string) => void;
  onReindex: (rootId: string) => void;
  onRemove: (rootId: string, currentAlias: string) => void;
}) {
  const { t } = useTranslation();
  return (
    <Paper className="table-panel" radius="lg" withBorder>
      <Table.ScrollContainer minWidth={1192}>
        <Table
          className="sources-table"
          highlightOnHover
          style={{ tableLayout: "fixed" }}
          verticalSpacing="md"
        >
          <colgroup>
            <col className="source-alias-column" />
            <col className="source-method-column" />
            <col className="source-number-column" />
            <col className="source-number-column" />
            <col className="source-number-column" />
            <col className="source-number-column" />
            <col className="source-date-column" />
            <col className="source-actions-column" />
          </colgroup>
          <Table.Thead>
            <Table.Tr>
              <Table.Th>{t("sources.table.alias")}</Table.Th>
              <Table.Th>{t("sources.table.discovery")}</Table.Th>
              <Table.Th ta="right">{t("sources.table.files")}</Table.Th>
              <Table.Th ta="right">{t("sources.table.skipped")}</Table.Th>
              <Table.Th ta="right">{t("sources.table.errors")}</Table.Th>
              <Table.Th ta="right">{t("sources.table.duplicates")}</Table.Th>
              <Table.Th>{t("sources.table.lastScan")}</Table.Th>
              <Table.Th className="source-actions-column" ta="center">
                {t("sources.table.actions")}
              </Table.Th>
            </Table.Tr>
          </Table.Thead>
          <Table.Tbody>
            {roots.map((root) => {
              const displayAlias = displayRootAlias(root, roots);
              const isCurrentlyIndexing = currentRootId === root.id;
              const activationState =
                root.activationState === "indexing" && !isCurrentlyIndexing
                  ? "confirmedUnindexed"
                  : root.activationState;
              return (
                <Table.Tr key={root.id}>
                  <Table.Td>
                    <Group gap="xs">
                      <Text fw={700}>{displayAlias}</Text>
                      <Badge color={root.enabled ? "green" : "gray"} variant="dot">
                        {root.enabled
                          ? t("sources.table.enabled")
                          : t("sources.table.disabled")}
                      </Badge>
                      {showPrimary && root.isPrimary ? (
                        <Badge color="red" variant="light">
                          {t("sources.table.primary")}
                        </Badge>
                      ) : null}
                      {isCurrentlyIndexing || activationState !== "ready" ? (
                        <Badge
                          color={activationState === "validationFailed" ? "red" : "blue"}
                          variant="light"
                        >
                          {t(
                            `sources.table.activation.${isCurrentlyIndexing ? "indexing" : activationState}`,
                          )}
                        </Badge>
                      ) : null}
                    </Group>
                  </Table.Td>
                  <Table.Td>{sourceDiscoveryLabel(t, root.discoveryCode)}</Table.Td>
                  <Table.Td ta="right">{root.fileCount}</Table.Td>
                  <Table.Td ta="right">{root.skippedCount}</Table.Td>
                  <Table.Td ta="right">{root.errorCount}</Table.Td>
                  <Table.Td ta="right">{root.duplicateCount}</Table.Td>
                  <Table.Td>{formatObservedAt(root.lastScanAtEpochMs)}</Table.Td>
                  <Table.Td className="source-actions-column" ta="center">
                    <Group className="source-actions-group" gap="xs" wrap="nowrap">
                      <Button
                        aria-label={t("sources.table.toggleAria", {
                          action: root.enabled
                            ? t("sources.table.disable")
                            : t("sources.table.enable"),
                          alias: displayAlias,
                        })}
                        disabled={scanRunning}
                        loading={togglePending}
                        onClick={() => onToggleEnabled(root.id, !root.enabled)}
                        size="compact-sm"
                        variant="subtle"
                      >
                        {root.enabled
                          ? t("sources.table.disable")
                          : t("sources.table.enable")}
                      </Button>
                      <Button
                        aria-label={t("sources.table.reindexAria", { alias: displayAlias })}
                        disabled={scanRunning || !root.enabled}
                        loading={reindexPendingRootId === root.id}
                        onClick={() => onReindex(root.id)}
                        size="compact-sm"
                        variant="subtle"
                      >
                        {t("sources.table.reindex")}
                      </Button>
                      {allowRenameRemove ? (
                        <>
                          <Button
                            aria-label={t("sources.table.renameAria", {
                              alias: displayAlias,
                            })}
                            disabled={scanRunning}
                            loading={renamePending}
                            onClick={() => onRename(root.id, root.alias)}
                            size="compact-sm"
                            variant="subtle"
                          >
                            {t("sources.table.rename")}
                          </Button>
                          <Button
                            aria-label={t("sources.table.removeAria", {
                              alias: displayAlias,
                            })}
                            color="red"
                            disabled={scanRunning}
                            loading={removePending}
                            onClick={() => onRemove(root.id, root.alias)}
                            size="compact-sm"
                            variant="subtle"
                          >
                            {t("sources.table.remove")}
                          </Button>
                        </>
                      ) : null}
                    </Group>
                  </Table.Td>
                </Table.Tr>
              );
            })}
          </Table.Tbody>
        </Table>
      </Table.ScrollContainer>
      {roots.length === 0 ? (
        <Text c="dimmed" p="xl" ta="center">
          {t("sources.table.empty")}
        </Text>
      ) : null}
    </Paper>
  );
}
