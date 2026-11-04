import { useI18n } from "@/i18n";
import type { ListedModel, ModelCatalog } from "@/types";

export function ModelSelect({
  id,
  value,
  models,
  onChange,
}: {
  id: string;
  value: string | null;
  models: ListedModel[];
  onChange: (model: string | null) => void;
}) {
  const { t } = useI18n();
  const known = new Set(models.map((model) => model.id));
  const extra = value && !known.has(value) ? value : null;

  return (
    <select
      id={id}
      className="h-8 w-full rounded-md border border-input bg-[#0c0e11] px-2 text-[13px] outline-none focus-visible:ring-1 focus-visible:ring-ring"
      value={value ?? ""}
      onChange={(e) => onChange(e.target.value || null)}
    >
      <option value="">{t("cliDefault")}</option>
      {extra && <option value={extra}>{extra}</option>}
      {models.map((model) => (
        <option key={model.id} value={model.id}>
          {model.displayName}
        </option>
      ))}
    </select>
  );
}

export function modelListMessage(catalog: ModelCatalog | null, t: (key: "modelsNoKey" | "modelsUnauthorized" | "modelsFailed") => string) {
  if (!catalog?.errorCode) return null;
  if (catalog.errorCode === "no_key") return t("modelsNoKey");
  if (catalog.errorCode === "unauthorized") return t("modelsUnauthorized");
  return t("modelsFailed");
}
