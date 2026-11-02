import { z } from "zod";

// =============================================================================
// placeWidgetWindow - Linux only
// =============================================================================
// Native Wayland clients cannot position their own windows, keep them above
// other windows, or show them on every workspace. The Amical GNOME Shell
// extension does it for windows the desktop names by title; this RPC relays
// the request through the helper. The extension remembers the request and
// re-applies it whenever a window with that title appears (Electron creates a
// new toplevel on every show).
// =============================================================================

export const PlaceWidgetWindowParamsSchema = z.object({
  /** Exact window title, e.g. the widget BrowserWindow's `title`. */
  title: z.string().min(1),
  x: z.number().int(),
  y: z.number().int(),
  above: z.boolean().optional(),
  sticky: z.boolean().optional(),
});
export type PlaceWidgetWindowParams = z.infer<
  typeof PlaceWidgetWindowParamsSchema
>;

export const PlaceWidgetWindowResultSchema = z.object({
  success: z.boolean(),
  /** The window existed when the request was applied. */
  found: z.boolean(),
  message: z.string().optional(),
});
export type PlaceWidgetWindowResult = z.infer<
  typeof PlaceWidgetWindowResultSchema
>;
