import { Table, type TableTdProps, type TableThProps } from "@mantine/core";
import type { ReactElement } from "react";

/** 末端对齐的表头：用于数值列与操作列，与 `EndTd` 成对使用，避免逐格重复写对齐。 */
export function EndTh(props: TableThProps): ReactElement {
  return <Table.Th ta="right" {...props} />;
}

/** 末端对齐的单元格：数值与操作列统一右对齐，文字列保持默认左对齐。 */
export function EndTd(props: TableTdProps): ReactElement {
  return <Table.Td ta="right" {...props} />;
}
