import {
  ActionIcon,
  Button,
  Group,
  Paper,
  SegmentedControl,
  SimpleGrid,
  Text,
  Tooltip,
  UnstyledButton,
} from "@mantine/core";
import { IconCheck, IconPhoto, IconUpload, IconX } from "@tabler/icons-react";
import { useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import type { MonitorImageCounts, MonitorImagePreview } from "../../api/monitor";
import { useImageCategoryFilter } from "../../pages/useImageCategoryFilter";

/** 从本机图库选择一张图，并可在面板内直传。 */
interface ImagePickerProps {
  images: MonitorImagePreview[];
  counts?: MonitorImageCounts;
  uploadAccept: string;
  value: string;
  disabled?: boolean;
  uploading?: boolean;
  onChange: (value: string) => void;
  onUpload: (file: File) => void;
}

/** 图片选择器：触发按钮打开筛选网格，value 为本机图库 ID。 */
export function ImagePicker({
  images,
  counts,
  uploadAccept,
  value,
  disabled,
  uploading,
  onChange,
  onUpload,
}: ImagePickerProps) {
  const { t } = useTranslation();
  const [opened, setOpened] = useState(false);
  const uploadInputRef = useRef<HTMLInputElement>(null);
  const { category, setCategory, filteredImages } = useImageCategoryFilter(images);
  const labelId = useId();
  const selectedImage = images.find((image) => image.id === value);
  return (
    <div>
      <Group justify="space-between" mb={7}>
        <Text component="span" fw={500} id={labelId} size="sm">
          {t("monitor.picker.field")} <span className="required-mark">*</span>
        </Text>
        {selectedImage ? (
          <Tooltip label={t("monitor.picker.clear")}>
            <ActionIcon
              aria-label={t("monitor.picker.clearAria")}
              color="gray"
              onClick={() => onChange("")}
              size="sm"
              variant="subtle"
            >
              <IconX aria-hidden="true" size={15} stroke={1.75} />
            </ActionIcon>
          </Tooltip>
        ) : null}
      </Group>
      <UnstyledButton
        aria-expanded={opened}
        aria-haspopup="listbox"
        aria-labelledby={labelId}
        className="image-picker-trigger"
        data-empty={!selectedImage || undefined}
        data-testid="image-picker-trigger"
        disabled={disabled}
        onClick={() => setOpened((current) => !current)}
        type="button"
      >
        {selectedImage ? (
          <>
            <img alt={selectedImage.filename} src={selectedImage.image} />
            <span className="image-picker-overlay">
              <IconPhoto aria-hidden="true" size={17} stroke={1.75} />
              {t("monitor.picker.change")}
            </span>
          </>
        ) : (
          <span className="image-picker-empty">
            <IconPhoto aria-hidden="true" size={24} stroke={1.75} />
            {disabled ? t("monitor.picker.loading") : t("monitor.picker.clickChoose")}
          </span>
        )}
      </UnstyledButton>
      {opened ? (
        <Paper mt="sm" p="sm" withBorder>
          <Group justify="space-between" mb="sm">
            <div>
              <Text fw={650} size="sm">
                {t("monitor.picker.chooseTitle")}
              </Text>
              <Text c="dimmed" size="xs">
                {category === "all"
                  ? t("monitor.picker.imagesTotal", { count: images.length })
                  : t("monitor.picker.imagesFiltered", {
                      visible: filteredImages.length,
                      total: images.length,
                    })}
              </Text>
            </div>
            <Group gap="xs" wrap="nowrap">
              <Button
                aria-label={t("monitor.picker.uploadSingleAria")}
                disabled={disabled}
                leftSection={<IconUpload aria-hidden="true" size={15} stroke={1.75} />}
                loading={uploading}
                onClick={() => uploadInputRef.current?.click()}
                size="xs"
                variant="light"
              >
                {uploading
                  ? t("monitor.picker.uploading")
                  : t("monitor.picker.uploadSingle")}
              </Button>
              <input
                accept={uploadAccept}
                data-testid="image-picker-upload"
                disabled={disabled}
                hidden
                onChange={(event) => {
                  const file = event.currentTarget.files?.[0];
                  if (file) onUpload(file);
                  event.currentTarget.value = "";
                }}
                ref={uploadInputRef}
                type="file"
              />
              <ActionIcon
                aria-label={t("monitor.picker.closeAria")}
                color="gray"
                onClick={() => setOpened(false)}
                variant="subtle"
              >
                <IconX aria-hidden="true" size={17} stroke={1.75} />
              </ActionIcon>
            </Group>
          </Group>
          <SegmentedControl
            data={[
              {
                value: "all",
                label: t("monitor.picker.allCount", { count: images.length }),
              },
              { value: "jpeg", label: `JPEG ${counts?.jpeg ?? 0}` },
              { value: "png", label: `PNG ${counts?.png ?? 0}` },
              { value: "gif", label: `GIF ${counts?.gif ?? 0}` },
            ]}
            fullWidth
            mb="sm"
            onChange={(next) => setCategory(next as typeof category)}
            size="xs"
            value={category}
          />
          <div className="image-picker-options" role="listbox">
            {filteredImages.length ? (
              <SimpleGrid cols={4} spacing="xs">
                {filteredImages.map((image) => (
                  <Tooltip key={image.id} label={image.filename}>
                    <UnstyledButton
                      aria-label={t("monitor.picker.selectAria", {
                        filename: image.filename,
                      })}
                      aria-selected={image.id === value}
                      className="image-picker-option"
                      data-selected={image.id === value || undefined}
                      onClick={() => {
                        onChange(image.id);
                        setOpened(false);
                      }}
                      role="option"
                      type="button"
                    >
                      <img alt="" src={image.image} />
                      {image.id === value ? (
                        <span className="image-picker-check">
                          <IconCheck aria-hidden="true" size={14} stroke={2} />
                        </span>
                      ) : null}
                    </UnstyledButton>
                  </Tooltip>
                ))}
              </SimpleGrid>
            ) : (
              <Text c="dimmed" py="xl" size="sm" ta="center">
                {t("monitor.picker.emptyCategory")}
              </Text>
            )}
          </div>
        </Paper>
      ) : null}
    </div>
  );
}
