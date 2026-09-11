import React, { Suspense } from "react";
import { createRoot } from "react-dom/client";
import { I18nextProvider } from "react-i18next";
import "@/styles/globals.css";
import { ThemeProvider } from "@/components/theme-provider";
import { Toaster } from "@/components/ui/sonner";
import { initializeRendererI18n } from "@/renderer/lib/initialize-i18n";
import {
  captureRendererException,
  initializeRendererPostHog,
} from "@/renderer/lib/posthog";
import { RendererErrorBoundary } from "@/renderer/lib/renderer-error-boundary";
import { LoadingScreen } from "./components/loading-screen";

// Lazy import the main content
const Content = React.lazy(() => import("./content"));

// Extend Console interface to include original methods
declare global {
  interface Console {
    original: {
      log: (...data: unknown[]) => void;
      info: (...data: unknown[]) => void;
      warn: (...data: unknown[]) => void;
      error: (...data: unknown[]) => void;
      debug: (...data: unknown[]) => void;
    };
  }
}

// Main window scoped logger setup with guards
const mainWindowLogger = window.electronAPI?.log?.scope?.("mainWindow");

// Store original console methods with proper binding
const originalConsole = {
  log: console.log.bind(console),
  info: console.info.bind(console),
  warn: console.warn.bind(console),
  error: console.error.bind(console),
  debug: console.debug.bind(console),
};

// Proxy console methods to use BOTH original console AND main window logger
console.log = (...args: unknown[]) => {
  originalConsole.log(...args); // Show in dev console
  mainWindowLogger?.info?.(...args); // Send via IPC if available
};
console.info = (...args: unknown[]) => {
  originalConsole.info(...args);
  mainWindowLogger?.info?.(...args);
};
console.warn = (...args: unknown[]) => {
  originalConsole.warn(...args);
  mainWindowLogger?.warn?.(...args);
};
console.error = (...args: unknown[]) => {
  originalConsole.error(...args);
  mainWindowLogger?.error?.(...args);
};
console.debug = (...args: unknown[]) => {
  originalConsole.debug(...args);
  mainWindowLogger?.debug?.(...args);
};

// Keep original methods available if needed
console.original = originalConsole;

// Main App component with Suspense
const App: React.FC = () => {
  return (
    <ThemeProvider>
      <Suspense fallback={<LoadingScreen />}>
        <Content />
      </Suspense>
      <Toaster />
    </ThemeProvider>
  );
};

// Add vibrancy class on macOS so CSS can make sidebar transparent
if (window.electronAPI?.platform === "darwin") {
  document.documentElement.classList.add("vibrancy");
}

// Render the app
const container = document.getElementById("root");
if (container) {
  const root = createRoot(container);
  const bootstrap = async () => {
    await initializeRendererPostHog("main");
    const i18n = await initializeRendererI18n();
    root.render(
      <I18nextProvider i18n={i18n}>
        <RendererErrorBoundary surface="main">
          <App />
        </RendererErrorBoundary>
      </I18nextProvider>,
    );
  };

  void bootstrap().catch((error) => {
    console.error("Failed to initialize i18n", error);
    captureRendererException(error, {
      error_context: "renderer_bootstrap_failed",
      surface: "main",
    });
  });
}
