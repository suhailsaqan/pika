import type { PluginRuntime } from "openclaw/plugin-sdk";

let runtime: PluginRuntime | null = null;

export function setPikachatRuntime(r: PluginRuntime): void {
  runtime = r;
}

export function getPikachatRuntime(): PluginRuntime {
  if (!runtime) {
    throw new Error("pikachat runtime not set (plugin not registered?)");
  }
  return runtime;
}
