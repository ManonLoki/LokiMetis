import "@mantine/core/styles.css";
import "./styles.css";

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { createStore, Provider as JotaiProvider } from "jotai";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { I18nextProvider } from "react-i18next";

import { AppThemeProvider } from "./components/AppThemeProvider";
import { appI18n, initializeI18n } from "./i18n";
import { readSavedInterfaceLanguage } from "./lib/language";
import { router } from "./router";
import { interfaceLanguageAtom } from "./state/interfaceLanguage";

const queryClient = new QueryClient({
  defaultOptions: {
    mutations: { retry: false },
    queries: { retry: false },
  },
});
const appStore = createStore();

/** 初始化语言后挂载唯一 React、Mantine、Query 与 Jotai 根。 */
export async function bootstrapApplication(): Promise<void> {
  await initializeI18n();
  const language = readSavedInterfaceLanguage() ?? "en-US";
  await appI18n.changeLanguage(language);
  appStore.set(interfaceLanguageAtom, language);

  const rootElement = document.getElementById("root");
  if (rootElement === null) throw new Error("missing application root");

  createRoot(rootElement).render(
    <StrictMode>
      <I18nextProvider i18n={appI18n}>
        <JotaiProvider store={appStore}>
          <QueryClientProvider client={queryClient}>
            <AppThemeProvider>
              <RouterProvider router={router} />
            </AppThemeProvider>
          </QueryClientProvider>
        </JotaiProvider>
      </I18nextProvider>
    </StrictMode>,
  );
}
