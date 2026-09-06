import { Badge, Group, Text, UnstyledButton } from "@mantine/core";
import { useTranslation } from "react-i18next";

/** 显示位置选择器固定每行六格，超过一行时据此生成可访问行列信息。 */
const SLOT_GRID_COLUMNS = 6;

/** 展示位选择器：可选范围完全由 core 能力提供。 */
interface SlotPickerProps {
  value: number;
  min: number;
  max: number;
  onChange: (value: number) => void;
}

/** 按每行六格展示 core 提供的全部可选展示位。 */
export function SlotPicker({ value, min, max, onChange }: SlotPickerProps) {
  const { t } = useTranslation();
  const count = Math.max(0, max - min + 1);
  const slots = Array.from({ length: count }, (_, index) => min + index);
  return (
    <div className="slot-picker">
      <Group align="flex-start" justify="space-between" mb="sm">
        <div>
          <Text fw={650}>{t("monitor.slot.title")}</Text>
          <Text c="dimmed" mt={3} size="sm">
            {t("monitor.slot.description")}
          </Text>
        </div>
        <Badge color="violet" size="lg" variant="light">
          {t("monitor.slot.position", { slot: value })}
        </Badge>
      </Group>
      <div aria-label={t("monitor.slot.groupAria")} className="slot-grid" role="group">
        {slots.map((slot, index) => {
          const row = Math.floor(index / SLOT_GRID_COLUMNS) + 1;
          const column = (index % SLOT_GRID_COLUMNS) + 1;
          return (
            <UnstyledButton
              aria-label={t("monitor.slot.cellAria", { slot, row, column })}
              aria-pressed={slot === value}
              className="slot-cell"
              data-selected={slot === value || undefined}
              key={slot}
              onClick={() => onChange(slot)}
              type="button"
            >
              {slot}
            </UnstyledButton>
          );
        })}
      </div>
    </div>
  );
}
