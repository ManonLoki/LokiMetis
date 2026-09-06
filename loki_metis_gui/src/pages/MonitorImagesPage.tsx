import {
  Alert,
  ActionIcon,
  Badge,
  Button,
  Card,
  Center,
  Group,
  Loader,
  Menu,
  SegmentedControl,
  SimpleGrid,
  Stack,
  Text,
} from "@mantine/core";
import {
  IconDots,
  IconPhoto,
  IconRefresh,
  IconTrash,
  IconUpload,
} from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  deleteMonitorImage,
  fileBytes,
  getMonitorCapabilities,
  imageUploadAcceptValue,
  listMonitorImages,
  saveMonitorImage,
  type MonitorImagePreview,
} from "../api/monitor";
import { useImageCategoryFilter } from "./useImageCategoryFilter";

/** 图片管理：展示本机图库，支持筛选、批量上传与删除。 */
export function MonitorImagesPage() {
  const { t } = useTranslation();
  const inputRef = useRef<HTMLInputElement>(null);
  const queryClient = useQueryClient();
  const capabilities = useQuery({
    queryFn: getMonitorCapabilities,
    queryKey: ["monitor-capabilities"],
  });
  const images = useQuery({ queryFn: listMonitorImages, queryKey: ["monitor-images"] });
  const upload = useMutation({
    mutationFn: async (files: File[]) => {
      let gallery = images.data;
      for (const file of files) {
        gallery = await saveMonitorImage(file.name, await fileBytes(file));
      }
      return { gallery, count: files.length };
    },
    onSuccess: ({ gallery }) => {
      queryClient.setQueryData(["monitor-images"], gallery);
    },
  });
  const remove = useMutation({
    mutationFn: deleteMonitorImage,
    onSuccess: (gallery) => {
      queryClient.setQueryData(["monitor-images"], gallery);
    },
  });
  const blockingError = capabilities.error ?? images.error;
  const mutationError = upload.error ?? remove.error;
  const imageList = images.data?.images ?? [];
  const counts = images.data?.counts;
  const { category, setCategory, filteredImages } = useImageCategoryFilter(imageList);
  const uploadAccept = imageUploadAcceptValue(capabilities.data?.imageUploadAccept);
  if (blockingError) return <Alert color="red">{String(blockingError)}</Alert>;
  return (
    <Stack data-testid="monitor-images" gap="md">
      <Group
        align="center"
        data-testid="monitor-images-toolbar"
        justify="space-between"
        wrap="nowrap"
      >
        {!images.isPending && imageList.length > 0 ? (
          <Group data-testid="monitor-images-filters" gap="sm" wrap="nowrap">
            <SegmentedControl
              aria-label={t("monitor.images.filterAria")}
              data={[
                {
                  value: "all",
                  label: t("monitor.images.allCount", { count: imageList.length }),
                },
                { value: "jpeg", label: `JPEG ${counts?.jpeg ?? 0}` },
                { value: "png", label: `PNG ${counts?.png ?? 0}` },
                { value: "gif", label: `GIF ${counts?.gif ?? 0}` },
              ]}
              onChange={(value) => setCategory(value as typeof category)}
              size="sm"
              value={category}
            />
            <Text aria-live="polite" c="dimmed" role="status" size="sm">
              {category === "all"
                ? t("monitor.images.imagesTotal", { count: imageList.length })
                : t("monitor.images.imagesFiltered", {
                    visible: filteredImages.length,
                    total: imageList.length,
                  })}
            </Text>
          </Group>
        ) : null}
        <Group data-testid="monitor-images-actions" gap="sm" ml="auto" wrap="nowrap">
          <Button
            leftSection={<IconRefresh aria-hidden="true" size={17} stroke={1.75} />}
            loading={images.isFetching}
            onClick={() => void images.refetch()}
            variant="default"
          >
            {t("monitor.images.refresh")}
          </Button>
          <Button
            leftSection={<IconUpload aria-hidden="true" size={17} stroke={1.75} />}
            loading={upload.isPending}
            onClick={() => inputRef.current?.click()}
          >
            {t("monitor.images.upload")}
          </Button>
          <input
            accept={uploadAccept}
            hidden
            multiple
            onChange={(event) => {
              const files = Array.from(event.currentTarget.files ?? []);
              if (files.length > 0) upload.mutate(files);
              event.currentTarget.value = "";
            }}
            ref={inputRef}
            type="file"
          />
        </Group>
      </Group>
      {mutationError ? <Alert color="red">{String(mutationError)}</Alert> : null}
      {upload.isSuccess ? (
        <Alert color="teal">
          {t("monitor.images.uploaded", { count: upload.data.count })}
        </Alert>
      ) : null}
      {images.isPending ? (
        <Center py={80}>
          <Loader />
        </Center>
      ) : imageList.length ? (
        <>
          {filteredImages.length ? (
            <SimpleGrid cols={{ base: 1, xs: 2, md: 3, xl: 4 }} spacing="md">
              {filteredImages.map((image) => (
                <LocalImageCard
                  image={image}
                  key={image.id}
                  onDelete={() => remove.mutate(image.id)}
                />
              ))}
            </SimpleGrid>
          ) : (
            <Center py={64}>
              <Text c="dimmed">{t("monitor.images.emptyCategory")}</Text>
            </Center>
          )}
        </>
      ) : (
        <Card className="empty-state" withBorder>
          <div className="empty-state-icon">
            <IconPhoto aria-hidden="true" size={30} stroke={1.75} />
          </div>
          <Text fw={650} mt="md">
            {t("monitor.images.emptyTitle")}
          </Text>
          <Text c="dimmed" maw={360} mt={6} size="sm" ta="center">
            {t("monitor.images.emptyDescription")}
          </Text>
          <Button mt="lg" onClick={() => inputRef.current?.click()} variant="light">
            {t("monitor.images.chooseMultiple")}
          </Button>
        </Card>
      )}
    </Stack>
  );
}

/** 单张本机图片卡片。 */
interface LocalImageCardProps {
  image: MonitorImagePreview;
  onDelete: () => void;
}

/** 预览、格式徽标、尺寸与删除菜单。 */
function LocalImageCard({ image, onDelete }: LocalImageCardProps) {
  const { t } = useTranslation();
  const [dimensions, setDimensions] = useState<{ width: number; height: number } | null>(
    null,
  );
  return (
    <Card className="image-card" padding={0} withBorder>
      <div className="image-preview">
        <img
          alt={image.filename}
          onLoad={(event) =>
            setDimensions({
              width: event.currentTarget.naturalWidth,
              height: event.currentTarget.naturalHeight,
            })
          }
          src={image.image}
        />
        <Badge className="image-type" color="dark" size="xs" variant="filled">
          {image.format.toUpperCase()}
        </Badge>
      </div>
      <Group justify="space-between" p="sm" wrap="nowrap">
        <div className="min-width-zero">
          <Text fw={600} size="sm" truncate>
            {dimensions
              ? `${dimensions.width} × ${dimensions.height}`
              : t("monitor.images.reading")}
          </Text>
          <Text c="dimmed" mt={3} size="xs">
            {t("monitor.images.localLibrary")}
          </Text>
        </div>
        <Menu position="bottom-end" shadow="md">
          <Menu.Target>
            <ActionIcon
              aria-label={t("monitor.images.actions", { name: image.filename })}
              color="gray"
              variant="subtle"
            >
              <IconDots aria-hidden="true" size={18} stroke={1.75} />
            </ActionIcon>
          </Menu.Target>
          <Menu.Dropdown>
            <Menu.Item
              color="red"
              leftSection={<IconTrash aria-hidden="true" size={16} stroke={1.75} />}
              onClick={onDelete}
            >
              {t("monitor.images.delete")}
            </Menu.Item>
          </Menu.Dropdown>
        </Menu>
      </Group>
    </Card>
  );
}
