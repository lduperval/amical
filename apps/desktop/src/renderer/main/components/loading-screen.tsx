import { useTranslation } from "react-i18next";

export function LoadingScreen() {
  const { t } = useTranslation();

  return (
    <div
      role="status"
      className="flex items-center justify-center min-h-screen bg-background"
    >
      <div className="flex flex-col items-center gap-4">
        <div className="relative" aria-hidden="true">
          <div className="w-12 h-12 border-4 border-muted rounded-full" />
          <div className="w-12 h-12 border-4 border-foreground border-t-transparent rounded-full animate-spin absolute top-0 left-0" />
        </div>
        <p className="text-sm text-muted-foreground">{t("app.loading")}</p>
      </div>
    </div>
  );
}
