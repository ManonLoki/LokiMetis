import { useCallback, useEffect, useState, type RefObject } from "react";

import { skinApi, type PreparedSkinImportBatch } from "../api/skins";

/** 定义换肤导入控制器依赖，使宿主查询、动作错误与卸载状态保持单向输入。 */
interface SkinImportControllerOptions {
  enabled: boolean;
  mounted: RefObject<boolean>;
  run: (operation: () => Promise<void>) => Promise<void>;
  showError: (cause: unknown) => void;
}

/** 管理原生选择器、拖放监听、批次取消与进度收敛，不承担页面展示。 */
export function useSkinImportController({
  enabled,
  mounted,
  run,
  showError,
}: SkinImportControllerOptions) {
  const [importBatch, setImportBatch] = useState<PreparedSkinImportBatch | null>(null);
  const [importSelected, setImportSelected] = useState<string[]>([]);
  const [importProgress, setImportProgress] = useState<number | null>(null);

  useEffect(() => {
    if (!enabled) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void skinApi
      .onFileDrop((event) => {
        if (disposed || event.type !== "drop") return;
        setImportProgress(0);
        void run(async () => {
          try {
            const batch = await skinApi.prepareDroppedPaths(event.paths, (progress) => {
              if (disposed) return;
              if (progress.type === "started") setImportProgress(0);
              else
                setImportProgress((value) =>
                  Math.min(100, (value ?? 0) + 100 / Math.max(1, event.paths.length)),
                );
            });
            if (disposed) {
              await skinApi.cancelImport(batch.token);
              return;
            }
            setImportBatch(batch);
            setImportSelected(batch.items.map((item) => item.itemId));
          } finally {
            if (!disposed) setImportProgress(null);
          }
        });
      })
      .then((cleanup) => {
        if (disposed) cleanup();
        else unlisten = cleanup;
      })
      .catch(showError);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [enabled, run, showError]);

  /** 从原生文件选择器预检一个有界 ZIP 批次，并保证所有终态清除进度。 */
  const prepareImport = useCallback(async (): Promise<void> => {
    setImportProgress(0);
    try {
      const batch = await skinApi.prepareImport((progress) => {
        if (!mounted.current) return;
        if (progress.type === "started") setImportProgress(0);
        else setImportProgress((value) => Math.min(95, (value ?? 0) + 12));
      });
      if (!mounted.current) {
        if (batch !== null) await skinApi.cancelImport(batch.token);
        return;
      }
      if (batch === null) return;
      setImportBatch(batch);
      setImportSelected(batch.items.map((item) => item.itemId));
    } finally {
      if (mounted.current) setImportProgress(null);
    }
  }, [mounted]);

  return {
    importBatch,
    importProgress,
    importSelected,
    prepareImport,
    setImportBatch,
    setImportSelected,
  };
}
