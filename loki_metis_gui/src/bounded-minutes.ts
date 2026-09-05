/** 所有分钟数间隔字段共用的批准范围：1 分钟至 1,440 分钟（一天）。 */
export const BOUNDED_MINUTES_MIN = 1;
/** 所有分钟数间隔字段共用的批准范围：1 分钟至 1,440 分钟（一天）。 */
export const BOUNDED_MINUTES_MAX = 1_440;

/** 把 Mantine NumberInput 可能给出的字符串输入解析为数值，不做范围裁剪。 */
export function parseBoundedMinutes(input: number | string): number {
  return typeof input === 'number' ? input : Number(input);
}

/** 校验分钟数输入是否落在批准的整数范围内。 */
export function isValidBoundedMinutes(input: number | string): boolean {
  const parsed = parseBoundedMinutes(input);
  return Number.isInteger(parsed) && parsed >= BOUNDED_MINUTES_MIN && parsed <= BOUNDED_MINUTES_MAX;
}

/** 分钟数间隔 NumberInput 共用的边界与格式约束 props。 */
export const boundedMinutesNumberInputProps = {
  allowDecimal: false,
  allowNegative: false,
  clampBehavior: 'none',
  max: BOUNDED_MINUTES_MAX,
  min: BOUNDED_MINUTES_MIN,
} as const;
