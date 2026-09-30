import { useMemo, useState } from "react";

import type { MonitorImageFormat, MonitorImagePreview } from "../api/monitor";

/** 图片分类：全部或单一格式。 */
export type ImageCategory = "all" | MonitorImageFormat;

/** 分类选择是纯 UI 状态；格式与计数由本机图库快照提供。 */
export function useImageCategoryFilter(images: MonitorImagePreview[]) {
  const [category, setCategory] = useState<ImageCategory>("all");
  const filteredImages = useMemo(
    () =>
      category === "all" ? images : images.filter((image) => image.format === category),
    [category, images],
  );
  return { category, setCategory, filteredImages };
}
