import { Badge, Group, Text, UnstyledButton } from "@mantine/core";
import { useTranslation } from "react-i18next";

/** 展示位选择器：范围由 core 能力提供，界面固定单行最多 6 槽。 */
interface SlotPickerProps {
  value: number;
  min: number;
  max: number;
  onChange: (value: number) => void;
}

/** 单行最多展示的槽位数。 */
const MAX_VISIBLE_SLOTS = 6;

/** 以单行展示可选展示位。 */
export function SlotPicker({ value, min, max, onChange }: SlotPickerProps) {
  const { t } = useTranslation();
  const count = Math.min(MAX_VISIBLE_SLOTS, Math.max(0, max - min + 1));
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
          const column = index + 1;
          return (
            <UnstyledButton
              aria-label={t("monitor.slot.cellAria", { slot, row: 1, column })}
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
