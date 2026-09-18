import { z } from "zod";

// =============================================================================
// getLinuxIntegrationStatus - Linux only
// =============================================================================
// What the Linux helper can do on the running desktop: which key-injection
// backend it has, which clipboard backend it will use (and whether that one
// steals focus from the target window), and whether the Amical GNOME Shell
// extension answers. The desktop uses it to explain missing pieces to the
// user instead of failing silently. Other helpers answer "method not found".
// =============================================================================

export const GetLinuxIntegrationStatusParamsSchema = z.object({});
export type GetLinuxIntegrationStatusParams = z.infer<
  typeof GetLinuxIntegrationStatusParamsSchema
>;

export const GetLinuxIntegrationStatusResultSchema = z.object({
  /** Selected input method name: clipboard | uinput | xtest | gnome_ext. */
  inputMethod: z.string(),
  /** A key-injection backend can deliver a chord right now. */
  injectionAvailable: z.boolean(),
  /** uinput | wayland-virtual-keyboard | gnome-extension */
  injectionBackend: z.string().optional(),
  /** gnome-extension | data-control | wl-clipboard-cli */
  clipboardBackend: z.string(),
  /** The clipboard backend opens a transient focused window per operation. */
  clipboardStealsFocus: z.boolean(),
  extensionAvailable: z.boolean(),
  extensionVersion: z.number().int().optional(),
  shellVersion: z.string().optional(),
  /** User-facing explanation of what is missing, when something is. */
  message: z.string().optional(),
});
export type GetLinuxIntegrationStatusResult = z.infer<
  typeof GetLinuxIntegrationStatusResultSchema
>;
