import { Alert, Button, Group, Paper, Stack, Text, Title } from "@mantine/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useRef } from "react";
import { useTranslation } from "react-i18next";

import { deleteMonitorImage, listMonitorImages, saveMonitorImage } from "../api/monitor";

/** 图片管理：本机图片库，不依赖局域网设备。 */
export function MonitorImagesPage() {
  const { t } = useTranslation();
  const inputRef = useRef<HTMLInputElement>(null);
  const queryClient = useQueryClient();
  const images = useQuery({ queryFn: listMonitorImages, queryKey: ["monitor-images"] });
  const upload = useMutation({
    mutationFn: async (file: File) => {
      const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
      return saveMonitorImage(file.name, bytes);
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["monitor-images"] });
    },
  });
  const remove = useMutation({
    mutationFn: deleteMonitorImage,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["monitor-images"] });
    },
  });
  return (
    <Stack data-testid="monitor-images" gap="md">
      <Paper p="lg" radius="lg" withBorder>
        <Stack gap="sm">
          <Group justify="space-between">
            <div>
              <Title order={3}>{t("monitor.images.title")}</Title>
              <Text c="dimmed" size="sm">
                {t("monitor.images.description")}
              </Text>
            </div>
            <Button onClick={() => inputRef.current?.click()} size="xs">
              {t("monitor.images.upload")}
            </Button>
          </Group>
          <input
            hidden
            onChange={(event) => {
              const file = event.currentTarget.files?.[0];
              if (file) upload.mutate(file);
              event.currentTarget.value = "";
            }}
            ref={inputRef}
            type="file"
          />
          {images.error ? <Alert color="red">{String(images.error)}</Alert> : null}
          {(images.data ?? []).length === 0 ? (
            <Text c="dimmed">{t("monitor.images.empty")}</Text>
          ) : (
            (images.data ?? []).map((item) => (
              <Group justify="space-between" key={item.id}>
                <Text>{item.filename}</Text>
                <Button
                  color="red"
                  onClick={() => remove.mutate(item.id)}
                  size="xs"
                  variant="light"
                >
                  {t("monitor.images.delete")}
                </Button>
              </Group>
            ))
          )}
        </Stack>
      </Paper>
    </Stack>
  );
}
