import { useI18n } from "@/i18n";
import type { ListedModel, ModelCatalog } from "@/types";

export const EFFORT_LEVELS = ["low", "medium", "high", "xhigh", "max"] as const;
export type EffortLevel = (typeof EFFORT_LEVELS)[number];

const EFFORT_LABEL: Record<
  EffortLevel,
  "effortLow" | "effortMedium" | "effortHigh" | "effortXhigh" | "effortMax"
> = {
  low: "effortLow",
  medium: "effortMedium",
  high: "effortHigh",
  xhigh: "effortXhigh",
  max: "effortMax",
};

export function normalizeEffort(value: string | null | undefined): EffortLevel | null {
  if (!value) return null;
  const level = value.trim().toLowerCase();
  return (EFFORT_LEVELS as readonly string[]).includes(level) ? (level as EffortLevel) : null;
}

export function ModelSelect({
  id,
  value,
  models,
  onChange,
  disabled,
  className,
}: {
  id: string;
  value: string | null;
  models: ListedModel[];
  onChange: (model: string | null) => void;
  disabled?: boolean;
  className?: string;
}) {
  const { t } = useI18n();
  const known = new Set(models.map((model) => model.id));
  const extra = value && !known.has(value) ? value : null;

  return (
    <select
      id={id}
      disabled={disabled}
      className={
        className ??
        "h-8 w-full rounded-md border border-input bg-[#0c0e11] px-2 text-[13px] outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50"
      }
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

export function EffortSelect({
  id,
  value,
  onChange,
  disabled,
  className,
}: {
  id: string;
  value: string | null;
  onChange: (effort: EffortLevel | null) => void;
  disabled?: boolean;
  className?: string;
}) {
  const { t } = useI18n();
  const selected = normalizeEffort(value);

  return (
    <select
      id={id}
      disabled={disabled}
      className={
        className ??
        "h-8 w-[148px] rounded-md border border-input bg-[#0c0e11] px-2 text-[13px] outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50"
      }
      value={selected ?? ""}
      onChange={(e) => onChange(normalizeEffort(e.target.value))}
    >
      <option value="">{t("effortDefault")}</option>
      {EFFORT_LEVELS.map((level) => (
        <option key={level} value={level}>
          {t(EFFORT_LABEL[level])}
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
