import {
  isMainPerformanceEvidenceEnabled,
  startMainPerformanceEvidence,
} from "./lib/performanceEvidence";

/** 性能验收先安装早期观测，正常启动则直接加载完整应用。 */
async function bootstrap(): Promise<void> {
  if (isMainPerformanceEvidenceEnabled()) await startMainPerformanceEvidence();
  const { bootstrapApplication } = await import("./appBootstrap");
  await bootstrapApplication();
}

void bootstrap();
