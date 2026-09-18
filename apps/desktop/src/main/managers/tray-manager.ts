import { app, Tray, Menu, nativeImage } from "electron";
import * as path from "path";
import { logger } from "../logger";
import type { WindowManager } from "../core/window-manager";
import { isLinux, isMacOS, isWindows } from "../../utils/platform";
import { initMainI18n } from "../../i18n/main";

export class TrayManager {
  private static instance: TrayManager | null = null;
  private tray: Tray | null = null;
  private windowManager: WindowManager | null = null;

  private constructor() {}

  static getInstance(): TrayManager {
    if (!TrayManager.instance) {
      TrayManager.instance = new TrayManager();
    }
    return TrayManager.instance;
  }

  async initialize(
    windowManager: WindowManager,
    locale?: string | null,
  ): Promise<void> {
    this.windowManager = windowManager;
    const i18n = await initMainI18n(locale);
    const t = i18n.t.bind(i18n);

    // Create tray icon
    const iconPath = this.getIconPath();
    logger.main.info(`Loading tray icon from: ${iconPath}`);

    const icon = nativeImage.createFromPath(iconPath);

    // Log icon details for debugging
    const size = icon.getSize();
    logger.main.info(
      `Icon loaded - Width: ${size.width}, Height: ${size.height}, Empty: ${icon.isEmpty()}`,
    );

    // On macOS, mark as template image for proper light/dark mode support
    // Use guid to persist menu bar position between app launches
    if (isMacOS()) {
      icon.setTemplateImage(true);
    }
    this.tray = new Tray(icon);

    // Set tooltip
    this.tray.setToolTip(t("tray.tooltip"));

    // Create context menu
    const contextMenu = Menu.buildFromTemplate([
      {
        label: t("tray.openConsole"),
        click: () => this.openConsole("tray menu"),
      },
      { type: "separator" as const },
      ...(isMacOS()
        ? [{ role: "about" as const }]
        : [
            {
              label: t("tray.about"),
              click: () => {
                app.showAboutPanel();
              },
            },
          ]),
      {
        label: t("menu.version", { version: app.getVersion() }),
        enabled: false,
      },
      { type: "separator" as const },
      {
        label: t("tray.quit"),
        click: () => {
          logger.main.info("Quit requested from tray");
          app.quit();
        },
      },
    ]);

    // Set the context menu
    this.tray.setContextMenu(contextMenu);

    // Double-clicking the icon opens the console. Electron only emits
    // "double-click" on macOS and Windows. On Linux the tray is a
    // StatusNotifierItem: GNOME's appindicator support turns a primary-button
    // double-click into the item's Activate call, which Electron reports as
    // "click" (a single primary click opens the menu there, so "click" never
    // fires for it). macOS/Windows keep single clicks for the menu.
    if (isLinux()) {
      this.tray.on("click", () => this.openConsole("tray activation"));
    } else {
      this.tray.on("double-click", () => this.openConsole("tray double-click"));
    }

    logger.main.info("Tray initialized successfully");
  }

  /**
   * Show and focus the normal Amical window, or the onboarding wizard while
   * it is open (same guard as activate / second-instance). Never opens a
   * second main window: createOrShowMainWindow reuses the existing one.
   */
  async openConsole(source: string): Promise<void> {
    logger.main.info("Open console requested", { source });
    if (!this.windowManager) {
      return;
    }
    const onboardingWindow = this.windowManager.getOnboardingWindow();
    if (onboardingWindow && !onboardingWindow.isDestroyed()) {
      onboardingWindow.show();
      onboardingWindow.focus();
      return;
    }
    try {
      await this.windowManager.createOrShowMainWindow();
    } catch (error) {
      logger.main.error("Failed to open console from tray", { source, error });
    }
  }

  private getIconPath(): string {
    // Use appropriate icon based on platform
    const iconName = isWindows()
      ? "icon-256x256.png" // Windows uses standard icon
      : "iconTemplate.png"; // macOS uses template naming convention

    if (app.isPackaged) {
      // When packaged, assets are placed next to the bundled resources path
      return path.join(process.resourcesPath, "assets", iconName);
    }

    // In development, rely on the project root returned by Electron
    // This avoids brittle relative traversals from the transpiled directory structure
    return path.join(app.getAppPath(), "assets", iconName);
  }

  cleanup(): void {
    //! DO NOT MANUALLY DESTROY, THIS RESETS THE TRAY POSITION
    //! EVEN IF IT SHOULDN'T
    /* if (this.tray && !this.tray.isDestroyed()) {
      this.tray.destroy();
      this.tray = null;
      logger.main.info("Tray cleaned up");
    } */
  }
}
