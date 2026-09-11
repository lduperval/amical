import { lazy, Suspense, type LazyExoticComponent } from "react";
import { Sparkles, type LucideIcon } from "lucide-react";
import dynamicIconImports from "lucide-react/dynamicIconImports";

// Loading the entire `icons` export blocks the main layout on every icon
// module, even when there are no remote-config surfaces to display. Cache
// lazy components so a surface loads only the glyph it actually uses.
const glyphs = new Map<string, LazyExoticComponent<LucideIcon>>();

function glyphFor(name: string): LazyExoticComponent<LucideIcon> | undefined {
  if (!Object.hasOwn(dynamicIconImports, name)) return undefined;
  let glyph = glyphs.get(name);
  if (!glyph) {
    glyph = lazy(() =>
      dynamicIconImports[name as keyof typeof dynamicIconImports]().catch(
        (error) => {
          console.warn("Failed to load remote-config icon", { name, error });
          return { default: Sparkles };
        },
      ),
    );
    glyphs.set(name, glyph);
  }
  return glyph;
}

export function RemoteConfigGlyph({
  name,
  fallbackName,
  className,
}: {
  name: string;
  fallbackName: string;
  className?: string;
}) {
  const Glyph = glyphFor(name) ?? glyphFor(fallbackName) ?? Sparkles;
  return (
    <Suspense fallback={<Sparkles className={className} />}>
      <Glyph className={className} />
    </Suspense>
  );
}
