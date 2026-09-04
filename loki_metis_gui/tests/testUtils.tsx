import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createStore, Provider as JotaiProvider } from "jotai";
import type { ReactElement, ReactNode } from "react";
import { I18nextProvider } from "react-i18next";

import { AppThemeProvider } from "../src/components/AppThemeProvider";
import { appI18n } from "../src/i18n";

/** 为组件测试挂载与生产一致的最小稳定 Provider 集合。 */
export function TestProviders({ children }: { children: ReactNode }): ReactElement {
  const queryClient = new QueryClient({
    defaultOptions: { mutations: { retry: false }, queries: { retry: false } },
  });
  const store = createStore();
  return (
    <I18nextProvider i18n={appI18n}>
      <JotaiProvider store={store}>
        <QueryClientProvider client={queryClient}>
          <AppThemeProvider>{children}</AppThemeProvider>
        </QueryClientProvider>
      </JotaiProvider>
    </I18nextProvider>
  );
}
