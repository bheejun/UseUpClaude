import type { HTMLAttributes } from "react";
import { cn } from "@/lib/utils";

export function Badge({ className, ...props }: HTMLAttributes<HTMLSpanElement>) {
  return (
    <span
      className={cn(
        "inline-flex items-center rounded border border-border bg-secondary/60 px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wide text-muted-foreground",
        className,
      )}
      {...props}
    />
  );
}
