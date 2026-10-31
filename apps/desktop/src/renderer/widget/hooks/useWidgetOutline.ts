import { useEffect, useState } from "react";
import { api } from "@/trpc/react";

export const useWidgetOutline = () => {
  const [enabled, setEnabled] = useState(false);

  api.settings.widgetOutlinePreference.useSubscription(undefined, {
    onData: setEnabled,
    onError: (error) => {
      console.error("Widget outline preference subscription error", error);
    },
  });

  useEffect(() => {
    const root = document.getElementById("root");
    root?.classList.toggle("show-widget-outline", enabled);
    return () => root?.classList.remove("show-widget-outline");
  }, [enabled]);
};
