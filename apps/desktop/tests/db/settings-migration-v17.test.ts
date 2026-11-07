import { describe, expect, it } from "vitest";
import {
  CURRENT_SETTINGS_VERSION,
  migrateSettings,
} from "../../src/db/settings-migrations";
import { migrateToV17 } from "../../src/db/settings-migrations/v17";

const completedOnboarding = {
  completedVersion: 1,
  completedAt: "2026-01-01T00:00:00.000Z",
  selectedModelType: "local" as const,
};

describe("migrateToV17", () => {
  it("keeps telemetry on for installs that finished onboarding", () => {
    expect(migrateToV17({ onboarding: completedOnboarding }).telemetry).toEqual(
      { enabled: true },
    );
  });

  it("keeps telemetry on for signed-in installs still in onboarding", () => {
    expect(
      migrateToV17({
        auth: {
          isAuthenticated: true,
          idToken: null,
          refreshToken: null,
          accessToken: null,
          expiresAt: null,
        },
      }).telemetry,
    ).toEqual({ enabled: true });
  });

  it("leaves anonymous installs still in onboarding undecided", () => {
    expect(migrateToV17({ ui: { theme: "dark" } })).toEqual({
      ui: { theme: "dark" },
    });
  });

  it("keeps an explicit choice", () => {
    expect(
      migrateToV17({
        onboarding: completedOnboarding,
        telemetry: { enabled: false },
      }).telemetry,
    ).toEqual({ enabled: false });
  });

  it("is registered as the current migration", () => {
    expect(CURRENT_SETTINGS_VERSION).toBe(17);
    expect(
      migrateSettings({ onboarding: completedOnboarding }, 16).telemetry,
    ).toEqual({ enabled: true });
  });
});
