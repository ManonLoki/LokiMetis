import { Badge, Button, Group, Paper, Stack, Text, Tooltip } from "@mantine/core";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import type { RootCandidateDto } from "../api/usage";
import { agentClientLabel } from "../state/agent-client";
import { visibleErrorMessage } from "../visible-error";

/** 用可独立并发添加的胶囊条目展示实时候选。 */
export function RootCandidateCapsules({
  candidates,
  onAdd,
}: {
  candidates: RootCandidateDto[];
  onAdd: (candidate: RootCandidateDto) => Promise<void>;
}) {
  const { t } = useTranslation();
  const [pendingIds, setPendingIds] = useState<Set<string>>(() => new Set());
  const [errors, setErrors] = useState<Record<string, string>>({});

  const addCandidate = async (candidate: RootCandidateDto) => {
    setPendingIds((current) => new Set(current).add(candidate.id));
    setErrors((current) => {
      const next = { ...current };
      delete next[candidate.id];
      return next;
    });
    try {
      await onAdd(candidate);
    } catch (error) {
      setErrors((current) => ({
        ...current,
        [candidate.id]: visibleErrorMessage(error),
      }));
    } finally {
      setPendingIds((current) => {
        const next = new Set(current);
        next.delete(candidate.id);
        return next;
      });
    }
  };

  return (
    <Stack gap="xs">
      {candidates.map((candidate) => (
        <Paper key={candidate.id} p="xs" radius="xl" withBorder>
          <Group gap="xs" wrap="nowrap">
            <Badge
              color={
                candidate.client === "codex"
                  ? "blue"
                  : candidate.client === "grokBuildCli"
                    ? "teal"
                    : "violet"
              }
              variant="light"
            >
              {agentClientLabel(candidate.client)}
            </Badge>
            <Tooltip label={candidate.absolutePath} multiline maw={720}>
              <Text className="candidate-capsule-path" size="sm">
                {candidate.absolutePath}
              </Text>
            </Tooltip>
            <Badge color="gray" variant="outline">
              {t(`sources.discovery.evidence.${candidate.evidence}`)}
            </Badge>
            <Badge color="gray" variant="outline">
              {t(`sources.discovery.strategy.${candidate.strategy}`)}
            </Badge>
            <Button
              loading={pendingIds.has(candidate.id)}
              onClick={() => void addCandidate(candidate)}
              radius="xl"
              size="compact-sm"
            >
              {t("sources.discovery.add")}
            </Button>
          </Group>
          {errors[candidate.id] ? (
            <Text c="red" mt={4} px="xs" size="xs">
              {errors[candidate.id]}
            </Text>
          ) : null}
        </Paper>
      ))}
    </Stack>
  );
}
