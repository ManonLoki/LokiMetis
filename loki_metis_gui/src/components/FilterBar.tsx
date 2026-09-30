import type { ReactElement, ReactNode } from "react";

/** 统一的筛选栏：控件在一行内自然换行，状态说明靠右，不再各页各排一套。 */
export function FilterBar({
  children,
  label,
  status,
}: {
  children: ReactNode;
  label: string;
  status?: ReactNode;
}): ReactElement {
  return (
    <div
      aria-label={label}
      className="filter-bar flex flex-wrap items-center gap-x-4 gap-y-3 rounded-(--mantine-radius-lg) border px-3 py-2.5"
      role="group"
    >
      {children}
      {status ? (
        <div className="ml-auto flex flex-wrap items-center gap-2">{status}</div>
      ) : null}
    </div>
  );
}
